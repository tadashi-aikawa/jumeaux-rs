"""HTTP部品の受入。Python入力アドオンと記録用サーバーを使い、Rust転送を検証する。"""
import argparse
import base64
from collections import Counter
from contextlib import contextmanager
import io
import json
import os
from pathlib import Path
import select
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "compat/gen"))
from http_transport import rust_probe, transport  # noqa: E402
from jumeaux.addons import AddOnExecutor  # noqa: E402
from jumeaux.commands.server.main import MyServerHandler  # noqa: E402
from jumeaux.domain.config.service import create_config  # noqa: E402
from jumeaux.models import Log2ReqsAddOnPayload, Reqs2ReqsAddOnPayload  # noqa: E402
from owlmixin import TList, TOption  # noqa: E402
import requests  # noqa: E402
from compare import configured_headers, request_headers, load  # noqa: E402

HEADER_RULES = load(ROOT / "compat/harness/comparison.json")["request_header_rules"]


@contextmanager
def serve(handler, tls=None):
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        if tls:
            server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server
        finally:
            server.shutdown()
            thread.join()


def record(handler, body):
    return {"method": handler.command, "url": "http://localhost:8000" + handler.path,
            "headers": list(handler.headers.items()), "body_base64": base64.b64encode(body).decode()}


def normalized(rows, configured):
    result = Counter()
    for row in rows:
        headers = request_headers(row, HEADER_RULES, configured)
        result[json.dumps({**row, "headers": headers}, sort_keys=True)] += 1
    return result


def baselines(binary):
    count, requests_count = 0, 0
    for baseline in sorted((ROOT / "compat/python-baseline").iterdir()):
        if not (baseline / "api.jsonl").is_file():
            continue
        expected = [json.loads(line) for line in (baseline / "api.jsonl").read_text().splitlines()]
        if not expected:
            continue  # encoding_hellは入力設定のエラーでHTTPへ到達しない。
        records = []

        class Receiver(MyServerHandler):
            protocol_version = "HTTP/1.1"

            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(baseline), **kwargs)

            def log_message(self, *args):
                pass

            def do_GET(self):
                records.append(record(self, b""))
                super().do_GET()

            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("content-length", "0")))
                records.append(record(self, body))
                original = self.rfile
                self.rfile = io.BytesIO(body)
                try:
                    super().do_POST()
                finally:
                    self.rfile = original

        config = create_config(TList([str(baseline / "config.yml")]), TOption(None))
        addons = AddOnExecutor(config.addons)
        reqs = addons.apply_log2reqs(Log2ReqsAddOnPayload.from_dict({"file": str(baseline / "requests")}))
        reqs = addons.apply_reqs2reqs(Reqs2ReqsAddOnPayload.from_dict({"requests": reqs}), config).requests.to_dicts()
        report = json.loads((baseline / "report.json").read_text(encoding=config.output.encoding))
        headers = configured_headers([report], HEADER_RULES)
        one, other = config.one.to_dict(), config.other.to_dict()
        for row in expected:
            row.pop("occurrence")
        with serve(Receiver) as server:
            for endpoint in (one, other):
                endpoint["host"] = endpoint["host"].replace("http://localhost:8000", f"http://127.0.0.1:{server.server_port}")
            repeats = 2 if baseline.name.startswith("retry_") else 1
            actual = rust_probe(binary, one, reqs * repeats, config.max_retries, other=other, action="pair")
            assert all("error" not in result for result in actual["results"]), (baseline.name, actual)
        assert normalized(records, headers) == normalized(expected, headers), (baseline.name, normalized(records, headers) - normalized(expected, headers), normalized(expected, headers) - normalized(records, headers))
        count += 1
        requests_count += len(records)
    print(f"baseline api.jsonl: {count}/{count}ケース、{requests_count}/{requests_count}リクエスト一致", flush=True)


def connection_contracts(binary):
    records = []

    class Receiver(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            records.append(self.path)
            self.send_response(200)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"ok")

        def do_POST(self):
            self.rfile.read(int(self.headers.get("Content-Length", "0")))
            self.do_GET()

        def do_CONNECT(self):
            host, port = self.path.rsplit(":", 1)
            assert host in {"127.0.0.1", "localhost"}, self.path
            records.append("CONNECT " + self.path)
            with socket.create_connection((host, int(port)), timeout=3) as upstream:
                self.send_response(200)
                self.end_headers()
                while True:
                    ready, _, _ = select.select([self.connection, upstream], [], [], 3)
                    if not ready:
                        break
                    for source in ready:
                        data = source.recv(65536)
                        if not data:
                            return
                        (upstream if source is self.connection else self.connection).sendall(data)

    base_env = {k: v for k, v in os.environ.items() if not k.lower().endswith("_proxy") and k not in {"REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE"}}
    count = 0
    with serve(Receiver) as proxy:
        proxy_address = f"127.0.0.1:{proxy.server_port}"
        endpoint = {"name": "one", "host": "http://fixture.invalid", "proxy": proxy_address}
        actual = rust_probe(binary, endpoint, [{"path": "/via-proxy"}], injected="dns")
        assert actual["results"][0]["status_code"] == 200, actual
        assert records[-1] == "http://fixture.invalid/via-proxy"
        count += 1
        for variable in ("HTTP_PROXY", "http_proxy", "ALL_PROXY"):
            with patch.dict(os.environ, {**base_env, variable: "http://" + proxy_address}, clear=True):
                actual = rust_probe(binary, {"name": "one", "host": "http://fixture.invalid"}, [{"path": "/env"}], trust_env=True, injected="dns")
                assert actual["results"][0]["status_code"] == 200, (variable, actual)
            count += 1
        with serve(Receiver) as direct:
            host = f"http://127.0.0.1:{direct.server_port}"
            with patch.dict(os.environ, {**base_env, "HTTP_PROXY": "http://" + proxy_address, "NO_PROXY": "127.0.0.1"}, clear=True):
                actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": "/bypass"}], trust_env=True)
                assert actual["results"][0]["status_code"] == 200, actual
                assert records[-1] == "/bypass", records
                count += 1
                actual = rust_probe(binary, {"name": "one", "host": host, "proxy": proxy_address}, [{"path": "/explicit"}], trust_env=True)
                assert actual["results"][0]["status_code"] == 200, actual
                assert records[-1] == host + "/explicit", records
                count += 1
            for bypass, target in [("127.0.0.0/8", host), ("*", host),
                                   (f"localhost:{direct.server_port}", host.replace("127.0.0.1", "localhost")),
                                   ("LOCALHOST", host.replace("127.0.0.1", "localhost")),
                                   (".localhost", host.replace("127.0.0.1", "localhost"))]:
                with patch.dict(os.environ, {**base_env, "HTTP_PROXY": "http://" + proxy_address, "NO_PROXY": bypass}, clear=True):
                    actual = rust_probe(binary, {"name": "one", "host": target}, [{"path": "/bypass-pattern"}], trust_env=True)
                    assert actual["results"][0]["status_code"] == 200, actual
                    assert records[-1] == "/bypass-pattern", records
                count += 1
            with patch.dict(os.environ, {**base_env, "REQUESTS_CA_BUNDLE": "/nonexistent-ca.pem"}, clear=True):
                actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": "/http-invalid-ca"}], trust_env=True)
                assert actual["results"][0]["status_code"] == 200, actual
                count += 1
                actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": "/mixed-invalid-ca"}],
                                    other={"name": "other", "host": "https://localhost:1"}, trust_env=True, action="challenge")
                assert actual["results"][0]["error"] == "OSError", actual
                assert records[-1] == "/mixed-invalid-ca", records
                count += 1
        with patch.dict(os.environ, {**base_env, "HTTP_PROXY": "http://127.0.0.1:9", "http_proxy": "http://" + proxy_address}, clear=True):
            actual = rust_probe(binary, {"name": "one", "host": "http://fixture.invalid"}, [{"path": "/lowercase"}], trust_env=True, injected="dns")
            assert actual["results"][0]["status_code"] == 200, actual
            count += 1
        with patch.dict(os.environ, {**base_env, "HTTP_PROXY": "http://" + proxy_address, "REQUEST_METHOD": "GET"}, clear=True):
            actual = rust_probe(binary, {"name": "one", "host": "http://fixture.invalid"}, [{"path": "/cgi"}], trust_env=True, injected="dns")
            assert actual["results"][0]["error"] == "ConnectionError", actual
            count += 1

        with tempfile.TemporaryDirectory(prefix="http-tls-", dir=ROOT / "target") as directory:
            key, cert = Path(directory) / "key.pem", Path(directory) / "cert.pem"
            ca_key, ca, csr = Path(directory) / "ca-key.pem", Path(directory) / "ca.pem", Path(directory) / "server.csr"
            for command in [
                ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(ca_key), "-out", str(ca), "-subj", "/CN=HTTP fixture CA", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-days", "1"],
                ["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", str(key), "-out", str(csr), "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost", "-addext", "extendedKeyUsage=serverAuth"],
                ["openssl", "x509", "-req", "-in", str(csr), "-CA", str(ca), "-CAkey", str(ca_key), "-CAcreateserial", "-out", str(cert), "-days", "1", "-copy_extensions", "copy"],
            ]:
                subprocess.run(command, capture_output=True, check=True)
            tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            tls.load_cert_chain(cert, key)
            with serve(Receiver, tls) as https:
                host = f"https://localhost:{https.server_port}"
                endpoint = {"name": "one", "host": host}
                for trusted in (False, True):
                    with requests.Session() as session:
                        session.trust_env = False
                        try:
                            result = session.get(host + "/tls", verify=str(ca) if trusted else True, timeout=3)
                            assert trusted and result.status_code == 200
                        except requests.exceptions.SSLError:
                            assert not trusted
                    actual = rust_probe(binary, endpoint, [{"path": "/tls"}], ca_bundle=str(ca) if trusted else None)
                    assert (actual["results"][0].get("status_code") == 200) == trusted, actual
                    count += 1
                mismatch = {"name": "one", "host": host.replace("localhost", "127.0.0.1")}
                actual = rust_probe(binary, mismatch, [{"path": "/wrong-host"}], ca_bundle=str(ca), action="challenge")
                assert actual["results"][0]["status"] == "failure", actual
                count += 1
                for variable in ("REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE"):
                    with patch.dict(os.environ, {**base_env, variable: str(ca)}, clear=True):
                        actual = rust_probe(binary, endpoint, [{"path": "/ca-env"}], trust_env=True)
                        assert actual["results"][0]["status_code"] == 200, actual
                    count += 1
                with patch.dict(os.environ, {**base_env, "HTTPS_PROXY": "http://" + proxy_address}, clear=True):
                    actual = rust_probe(binary, endpoint, [{"path": "/tunnel"}], trust_env=True, ca_bundle=str(ca))
                    assert actual["results"][0]["status_code"] == 200, actual
                    assert "CONNECT " + host.removeprefix("https://") in records
                    count += 1
                with serve(Receiver, tls) as secure_proxy:
                    endpoint = {**endpoint, "proxy": f"localhost:{secure_proxy.server_port}"}
                    actual = rust_probe(binary, endpoint, [{"path": "/secure-proxy"}], ca_bundle=str(ca))
                    assert actual["results"][0]["status_code"] == 200, actual
                    count += 1
            # Apple固有の有効期間・EKU制限を持ち込まないことを固定する。
            subprocess.run(["openssl", "req", "-new", "-key", str(key), "-out", str(csr),
                            "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost"], capture_output=True, check=True)
            for days in (3650, 365):
                subprocess.run(["openssl", "x509", "-req", "-in", str(csr), "-CA", str(ca), "-CAkey", str(ca_key),
                                "-out", str(cert), "-days", str(days), "-copy_extensions", "copy"], capture_output=True, check=True)
                tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                tls.load_cert_chain(cert, key)
                with serve(Receiver, tls) as https:
                    host = f"https://localhost:{https.server_port}"
                    with requests.Session() as session:
                        session.trust_env = False
                        assert session.get(host, verify=str(ca), timeout=3).status_code == 200
                    actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": "/no-eku"}], ca_bundle=str(ca))
                    assert actual["results"][0]["status_code"] == 200, actual
                    count += 1
                    with patch.dict(os.environ, {**base_env, "REQUESTS_CA_BUNDLE": str(ca)}, clear=True):
                        actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": "/notify"}], action="notify")
                        assert actual["results"][0]["ok"], actual
                    count += 1
    print(f"proxy・TLS: {count}/{count}成功", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=ROOT / "target/debug/examples/http_probe", type=Path)
    parser.add_argument("--phase", choices=["transport", "baseline", "connection"])
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    if args.phase in (None, "transport"):
        cases = transport(binary)
        print(f"HTTP transport: {len(cases)}/{len(cases)} Python一致", flush=True)
    if args.phase in (None, "baseline"):
        baselines(binary)
    if args.phase in (None, "connection"):
        connection_contracts(binary)
