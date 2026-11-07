"""HTTP境界のPython実測。通信先はループバックのみ。JSONをstdoutへ出す。

実行: 移行元の.venv/bin/python -B compat/gen/http_transport.py
encodingだけ: 同じコマンドに --encoding を付ける。
ソース・既存fixtureには書き込まない。
"""

import gzip
import json
import os
from pathlib import Path
import socket
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch
import warnings

SOURCE = Path(os.environ.get(
    "JUMEAUX_PYTHON_ROOT",
    Path.home() / "git/github.com/tadashi-aikawa/jumeaux",
))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from jumeaux import executor  # noqa: E402
from jumeaux.models import Request, Response  # noqa: E402
from owlmixin import TDict, TOption  # noqa: E402
import requests  # noqa: E402
from requests.adapters import HTTPAdapter  # noqa: E402
from requests.structures import CaseInsensitiveDict  # noqa: E402
from requests.utils import get_encoding_from_headers  # noqa: E402

warnings.filterwarnings("ignore")


def encodings():
    cases = []
    for name, ct, body, default in [
        ("binary", "application/octet-stream; charset=UTF-8", b"abc", "cp932"),
        ("json", "application/json", b"{}", "cp932"),
        ("json-meta", "application/json", b'<meta charset="cp932">', None),
        ("latin1-skipped", "text/plain; charset=ISO-8859-1", b"abc", "cp932"),
        ("latin1-lowercase-kept", "text/plain; charset=iso-8859-1", b"abc", "cp932"),
        ("meta", "text/html", b'<meta charset="Shift_JIS">', "utf-8"),
        ("meta-priority", "text/html", b'<?xml encoding="ascii"?><meta charset="cp932">', "utf-8"),
        ("xml", "application/xml", b'<?xml encoding="EUC-JP"?><r/>', "utf-8"),
        ("default", None, b"abc", "cp932"),
        ("ascii", None, b"abc", None),
        ("utf8", None, "日本語の文字コードを調べます".encode(), None),
        ("cp932", None, "日本語の文字コードを調べます".encode("cp932"), None),
        ("bom", None, b"\xef\xbb\xbfabc", None),
        ("empty", None, b"", None),
        ("empty-text", "text/plain", b"", None),
        ("empty-json", "application/json", b"", None),
        ("empty-default", None, b"", "utf-8"),
        ("duplicate-charset", "text/plain; charset=ascii; charset=cp932", b"abc", None),
        ("quoted-key", "application/xml; 'charset'= 'cp932'", b"abc", None),
        ("text-only-in-parameter", "application/xml; note=text", b"abc", "cp932"),
        ("json-only-in-parameter", "application/xml; note=application/json", b"abc", "cp932"),
        ("empty-charset", "application/json; charset=", b"abc", "cp932"),
        ("uppercase-mime", "APPLICATION/JSON", b"abc", "cp932"),
    ]:
        res = requests.Response()
        res.headers = CaseInsensitiveDict({"content-type": ct} if ct else {})
        res._content = body
        res.encoding = get_encoding_from_headers(res.headers)
        cases.append({
            "name": name,
            "input": {"content_type": ct, "body_hex": body.hex(), "default": default},
            "expected": {"value": Response._decide_encoding(res, TOption(default))},
        })
    return cases


def failure_trial(error, method):
    """実際に観測した例外をchallenge境界へ渡して捕捉の有無を確かめる。"""
    arg = {
        "seq": 1, "number_of_request": 1, "key": "fixture", "session": object(),
        "req": Request.from_dict({"path": "/probe", "method": method}),
        "host_one": "http://localhost", "host_other": "http://localhost",
        "headers_one": {}, "headers_other": {}, "res_dir": "unused",
        "judge_response_header": False, "ignore_response_header_keys": [],
    }
    with patch.object(executor, "concurrent_request", side_effect=error):
        try:
            trial = executor.challenge(arg)
        except Exception as raised:
            return {"error": type(raised).__name__}
    trial.pop("request_time")
    return {"value": trial}


def transport():
    records = []

    class Receiver(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def do_POST(self):
            self.do_GET()

        def do_GET(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            records.append({
                "method": self.command, "path": self.path,
                "headers": [[k.lower(), v] for k, v in self.headers.items()
                            if k.lower() in {"content-type", "x-test", "cookie", "user-agent"}],
                "body_hex": body.hex(),
            })
            if self.path == "/disconnect":
                self.close_connection = True
                return
            if self.path == "/read-timeout":
                time.sleep(0.6)
                self.close_connection = True
                return
            status, headers, body = 200, {}, b"ok"
            if self.path.startswith("/redirect/"):
                status = int(self.path.rsplit("/", 1)[1])
                headers["Location"] = "/done"
            if self.path == "/loop":
                status, headers = 302, {"Location": "/loop"}
            if self.path == "/retry-after":
                status, headers = 503, {"Retry-After": "0"}
            if self.path == "/status-503":
                status = 503
            if self.path == "/cookie/set":
                headers["Set-Cookie"] = "shared=yes; Path=/"
            if self.path == "/gzip":
                body = gzip.compress(b"expanded", mtime=0)
                headers["Content-Encoding"] = "gzip"
            self.send_response(status)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body) + (5 if self.path == "/truncated" else 0)))
            for key, value in headers.items():
                self.send_header(key, value)
            self.end_headers()
            if self.path == "/body-timeout":
                time.sleep(0.6)
                self.close_connection = True
                return
            self.wfile.write(body)
            if self.path == "/truncated":
                self.close_connection = True

    cases = []
    with ThreadingHTTPServer(("127.0.0.1", 0), Receiver) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f"http://127.0.0.1:{server.server_port}"
        try:
            probes = [
                ("get", "GET", "/probe?q=%93%FA%96%7B&empty=", 2, {}),
                ("raw", "POST", "/probe", 2, {"data": "日本語"}),
                ("form", "POST", "/probe", 2, {"data": {"a": ["x y", "日本"], "b": True, "skip": None}}),
                ("json", "POST", "/probe", 2, {"json": {"text": "日本", "n": 18446744073709551617}}),
                ("data-before-json", "POST", "/probe", 2, {"data": "raw", "json": {"x": 1}}),
                ("empty-data-json", "POST", "/probe", 2, {"data": {}, "json": {"x": 1}}),
                ("empty-post", "POST", "/probe", 2, {}),
                ("gzip", "GET", "/gzip", 2, {}),
                ("status-503", "GET", "/status-503", 2, {}),
                ("retry-after-get", "GET", "/retry-after", 2, {}),
                ("retry-after-post", "POST", "/retry-after", 2, {}),
                ("cookie-shared", "GET", "/cookie/read", 2, {}),
            ]
            for status in (301, 302, 303, 307, 308):
                probes.append((f"redirect-post-{status}", "POST", f"/redirect/{status}", 2, {"data": "raw"}))
            for method in ("GET", "POST"):
                for path in ("disconnect", "read-timeout", "body-timeout", "truncated"):
                    for retries in (0, 2):
                        probes.append((f"{path}-{method.lower()}-{retries}", method, f"/{path}", retries, {}))
            probes.append(("redirect-loop", "GET", "/loop", 2, {}))
            for name, method, path, retries, kwargs in probes:
                timeout = 0.2 if "timeout" in path else 3.0
                with requests.Session() as session:
                    session.trust_env = False
                    session.mount("http://", HTTPAdapter(max_retries=retries))
                    records.clear()
                    if name == "cookie-shared":
                        session.get(base + "/cookie/set")
                        session.close()  # executor.http_getも毎回closeするがCookieは残る。
                    try:
                        res = session.request(
                            method, base + path, timeout=timeout,
                            headers=executor.merge_headers(TDict({"X-Test": "endpoint"}), TDict({"X-Test": "request"})),
                            **kwargs,
                        )
                        expected = {"status_code": res.status_code, "body_hex": res.content.hex(),
                                    "path": res.url.removeprefix(base)}
                    except requests.RequestException as error:
                        expected = {"error": type(error).__name__, "challenge": failure_trial(error, method)}
                # 自動User-Agentは版違いを比較しない。明示X-TestとContent-Type等を保持。
                captured = [{**r, "headers": [h for h in r["headers"] if h[0] != "user-agent"]} for r in records]
                cases.append({"name": name, "input": {"method": method, "path": path,
                              "max_retries": retries, "timeout": timeout, **kwargs},
                              "expected": {**expected, "records": captured}})

            # DNSとconnect timeoutはローカルのソケット入口で注入。外部DNSにも送らない。
            for name, error in [("dns", socket.gaierror(-2, "fixture")),
                                ("connect-timeout", socket.timeout("fixture")),
                                ("connection-refused", ConnectionRefusedError(61, "fixture"))]:
                for method in ("GET", "POST"):
                    for retries in (0, 2):
                        with requests.Session() as session:
                            session.trust_env = False
                            session.mount("http://", HTTPAdapter(max_retries=retries))
                            with patch("urllib3.util.connection.create_connection", side_effect=error) as connect:
                                try:
                                    session.request(method, base + "/probe", timeout=0.2)
                                    raise AssertionError("expected connection failure")
                                except requests.RequestException as raised:
                                    expected = {"error": type(raised).__name__, "attempts": connect.call_count,
                                                "challenge": failure_trial(raised, method)}
                            cases.append({"name": f"{name}-{method.lower()}-{retries}",
                                          "input": {"method": method, "max_retries": retries, "injected": name},
                                          "expected": expected})
        finally:
            server.shutdown()
            thread.join()
    return cases


if __name__ == "__main__":
    print(json.dumps(encodings() if "--encoding" in sys.argv else transport(), ensure_ascii=False, indent=2))
