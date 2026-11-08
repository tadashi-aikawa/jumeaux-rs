"""CA bundleの例外分類と通知のステータス判定をローカルで採取・比較する。"""
import argparse
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import tempfile
from http.server import BaseHTTPRequestHandler
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "compat/harness"))
from http_transport import requests, rust_probe, failure_trial  # noqa: E402
from http_acceptance import serve  # noqa: E402
from jumeaux.models import Notifier  # noqa: E402
from jumeaux.notification_handlers import create_notification_handler  # noqa: E402


def cases(binary=None):
    result, records = [], []

    class Receiver(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers["Content-Length"]))
            records.append({"headers": {k.lower(): v for k, v in self.headers.items() if k.lower() not in {"host", "user-agent"}},
                            "body_hex": raw.hex(), "user_agent": self.headers.get("User-Agent")})
            status = int(self.path.rsplit("/", 1)[1])
            body = b"" if status == 204 else b"reply"
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    with tempfile.TemporaryDirectory(prefix="http-review2-", dir=ROOT / "target") as directory:
        directory = Path(directory)
        key, cert = directory / "key.pem", directory / "cert.pem"
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(key),
                        "-out", str(cert), "-subj", "/CN=localhost", "-days", "1"], capture_output=True, check=True)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(cert, key)
        with serve(Receiver, tls) as server:
            host = f"https://localhost:{server.server_port}"
            for label, data in [("missing", None), ("unreadable", b"data"), ("empty", b""),
                                ("not-pem", b"not pem"), ("bad-certificate", b"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n")]:
                bundle = directory / (label + ".pem")
                if data is not None:
                    bundle.write_bytes(data)
                if label == "unreadable":
                    bundle.chmod(0)
                try:
                    with requests.Session() as session:
                        session.trust_env = False
                        try:
                            session.get(host, verify=str(bundle), timeout=3)
                            raise AssertionError("invalid bundle accepted")
                        except (requests.RequestException, OSError) as error:
                            trial = failure_trial(error, "GET")
                            expected = {"send_error": type(error).__name__,
                                        "challenge": "failure" if "value" in trial else trial["error"]}
                    case = {"name": "ca-" + label, "input": {"bundle": label}, "expected": expected}
                    if binary:
                        endpoint = {"name": "one", "host": host}
                        sent = rust_probe(binary, endpoint, [{"path": "/"}], ca_bundle=str(bundle))["results"][0]
                        trial = rust_probe(binary, endpoint, [{"path": "/"}], ca_bundle=str(bundle), action="challenge")["results"][0]
                        actual = {"send_error": sent["error"], "challenge": trial.get("error", trial.get("status"))}
                        assert actual == expected, (label, actual, expected)
                    result.append(case)
                finally:
                    if label == "unreadable":
                        bundle.chmod(0o600)

    with serve(Receiver) as server:
        host = f"http://127.0.0.1:{server.server_port}"
        for version in (1, 2):
            notifier = {"type": "slack", "version": version, "channel": "#local"}
            for status in (200, 201, 204, 302, 399, 400, 500):
                path = f"/notify/{status}"
                records.clear()
                with patch.dict(os.environ, {"SLACK_INCOMING_WEBHOOKS_URL": host + path}, clear=True):
                    error = create_notification_handler(Notifier.from_dict(notifier)).notify("status fixture").get()
                    expected = {"ok": error is None, "notification_error": error}
                    python_record = records[-1]
                    python_record.pop("user_agent")
                    if binary:
                        actual = rust_probe(binary, {"name": "one", "host": host}, [{"path": path}], action="notify",
                                            notifier=notifier, message="status fixture")["results"][0]
                        assert actual == expected, (version, status, actual, expected)
                        rust_record = records[-1]
                        assert rust_record.pop("user_agent").startswith("jumeaux/"), rust_record
                        assert rust_record == python_record, (version, status, rust_record, python_record)
                case = {"name": f"notify-v{version}-{status}", "input": {"notifier": notifier, "status": status},
                        "expected": {**expected, "record": python_record}}
                result.append(case)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path)
    args = parser.parse_args()
    observed = cases(str(args.rust.resolve()) if args.rust else None)
    if args.rust:
        golden = json.loads((ROOT / "compat/fixtures/http-review2.json").read_text())
        assert observed == golden, "Python観測値が保存済みfixtureと異なる"
        print(f"HTTP review2: {len(observed)}/{len(observed)} Python一致")
    else:
        print(json.dumps(observed, ensure_ascii=False, indent=2))
