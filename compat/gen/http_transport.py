"""HTTP境界のPython実測。通信先はループバックのみ。JSONをstdoutへ出す。

実行: 移行元の.venv/bin/python -B compat/gen/http_transport.py
encodingだけ: 同じコマンドに --encoding を付ける。
ソース・既存fixtureには書き込まない。
"""

import gzip
import json
import os
import re
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch
import warnings
import zlib
from compression import zstd
from datetime import datetime
from urllib.parse import parse_qs, urlsplit

SOURCE = Path(os.environ.get(
    "JUMEAUX_PYTHON_ROOT",
    Path.home() / "git/github.com/tadashi-aikawa/jumeaux",
))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from jumeaux import executor  # noqa: E402
from jumeaux.models import Request, Response, QueryCustomization  # noqa: E402
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


def prepared():
    cases = []
    for name, endpoint, req in [
        ("empty-query", {}, {"path": "/"}),
        ("unicode-path", {}, {"path": "/日本 語"}),
        ("unreserved-percent", {}, {"path": "/%7E%41%2F%zz"}),
        ("valid-percent", {}, {"path": "/%7E%41%2F"}),
        ("percent-uppercase", {}, {"path": "/%e6%97%a5/%2f", "qs": {"x": ["%ab"]}}),
        ("path-backreference", {"path": {"before": r"/old/(\w+)", "after": r"/new/\1/\1"}}, {"path": "/old/api"}),
        ("query-cp932", {}, {"path": "/", "qs": {"key": ["日本", "", "+ &"]}, "url_encoding": "cp932"}),
        ("query-first-case", {"query": {"overwrite": {"key/i": ["new"]}, "remove": ["DROP/i"]}}, {"path": "/", "qs": {"Key": ["1"], "KEY": ["2"], "drop": ["x"]}}),
        ("query-original-keys", {"query": {"overwrite": {"NEW": ["a"], "new/i": ["b"]}, "remove": ["new/i"]}}, {"path": "/"}),
        ("query-datetime", {"query": {"overwrite": {"time": ["$DATETIME(fixed)(3600)"]}}}, {"path": "/"}),
        ("query-datetime-error", {"query": {"overwrite": {"time": ["$DATETIME(fixed)(invalid)"]}}}, {"path": "/"}),
        ("header-case-order", {"headers": {"X-Key": "one", "x-key": "two"}}, {"path": "/", "headers": {"X-Key": "request"}}),
        ("header-overwrite", {"headers": {"X-Key": "one", "User-Agent": "custom"}}, {"path": "/", "headers": {"X-Key": "request"}}),
        ("form-nested", {}, {"path": "/", "method": "POST", "form": {"dict": {"a": 1}, "list": [["x", "y"], None], "bool": False}}),
        ("json-ascii", {}, {"path": "/", "method": "POST", "json": {"text": "日本😀\u007f", "integer": 18446744073709551617, "float": 1e20, "small": 1e-7}}),
        ("body-priority", {}, {"path": "/", "method": "POST", "raw": "raw", "form": {"x": 1}, "json": {"y": 2}}),
        ("get-ignores-body", {}, {"path": "/", "method": "GET", "raw": "raw", "json": {"x": 1}}),
        ("content-length", {}, {"path": "/", "method": "POST", "raw": "日本", "headers": {"Content-Length": "100"}}),
    ]:
        endpoint = {"name": "one", "host": "http://localhost/base", **endpoint}
        request = Request.from_dict(req)
        try:
            path = re.sub(endpoint["path"]["before"], endpoint["path"]["after"], request.path) if "path" in endpoint else request.path
            custom = QueryCustomization.from_dict(endpoint["query"]) if "query" in endpoint else None
            query = executor.create_query_string(request.qs, TOption(custom), request.url_encoding)
            url = endpoint["host"] + path + "?" + query
            headers = executor.merge_headers(TDict(endpoint.get("headers", {})), request.headers)
            kwargs = {"data": request.raw.get() or request.form.get(), "json": request.json.get()} if request.method.value == "POST" else {}
            ready = requests.Request(request.method.value, url, headers=headers, **kwargs).prepare()
            body = ready.body.encode() if isinstance(ready.body, str) else ready.body or b""
            selected = {k.lower() for k in headers} | {"content-type"}
            actual_headers = {k.lower(): v for k, v in ready.headers.items() if k.lower() in selected}
            if actual_headers.get("user-agent", "").startswith("jumeaux/"):
                actual_headers["user-agent"] = "jumeaux/<version>"
            expected = {"value": {"original_url": url, "url": ready.url, "headers": actual_headers, "body_hex": body.hex()}}
        except Exception as error:
            expected = {"error": type(error).__name__}
        cases.append({"name": name, "input": {"endpoint": endpoint, "request": req}, "expected": expected})
    return cases


def timestamps():
    return [{"input": value, "expected": datetime.fromisoformat(value).isoformat()} for value in
            ("2026-11-08T00:57:56.433948+09:00", "2026-11-08T00:57:56.000000+09:00",
             "2026-11-08T00:57:56.001000+00:00", "2026-11-08T00:57:56.000000-05:00")]


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


def rust_probe(binary, one, reqs, retries=2, timeout=3, **kwargs):
    result = subprocess.run([binary], input=json.dumps({"one": one, "requests": reqs,
                            "max_retries": retries, "timeout": timeout, **kwargs}),
                            text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise AssertionError(result.stderr)
    return json.loads(result.stdout)


def normalized_records(records):
    return [{**r, "headers": sorted([k, re.sub(r"^jumeaux/[^ ]+$", "jumeaux/<version>", v) if k == "user-agent" else v]
                                    for k, v in r["headers"])} for r in records]


def assert_records(actual, expected, name):
    assert normalized_records(actual) == normalized_records(expected), (name, normalized_records(actual), normalized_records(expected))


def transport(rust=None):
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
                            if k.lower() != "host"],
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
            if self.path == "/retry-after-empty":
                status, headers = 503, {"Retry-After": ""}
            if self.path == "/status-503":
                status = 503
            if self.path == "/cookie/set":
                headers["Set-Cookie"] = "shared=yes; Path=/"
            if self.path == "/gzip":
                body = gzip.compress(b"expanded", mtime=0)
                headers["Content-Encoding"] = "gzip"
            if self.path in {"/gzip-upper", "/gzip-trailing", "/gzip-truncated", "/gzip-multi", "/gzip-corrupt", "/redirect-bad-gzip"}:
                body = gzip.compress(b"expanded", mtime=0)
                headers["Content-Encoding"] = "GZIP" if self.path == "/gzip-upper" else "gzip"
                if self.path == "/gzip-trailing":
                    body += b"\x00\x00junk"
                if self.path == "/gzip-truncated":
                    body = body[:-6]
                if self.path == "/gzip-multi":
                    body += gzip.compress(b"second", mtime=0)
                if self.path in {"/gzip-corrupt", "/redirect-bad-gzip"}:
                    body = b"not gzip"
                if self.path == "/redirect-bad-gzip":
                    status, headers["Location"] = 302, "/done"
            if self.path in {"/zstd", "/zstd-truncated", "/zstd-cut-last-byte", "/zstd-cut-header", "/zstd-corrupt"}:
                body, headers["Content-Encoding"] = zstd.compress(b"expanded"), "zstd"
                if self.path == "/zstd-truncated":
                    body = body[:-5]
                if self.path == "/zstd-cut-last-byte":
                    body = body[:-1]
                if self.path == "/zstd-cut-header":
                    body = body[:5]
                if self.path == "/zstd-corrupt":
                    body = b"not zstd"
            if self.path in {"/deflate", "/raw-deflate"}:
                body = zlib.compress(b"expanded")
                if self.path == "/raw-deflate":
                    body = body[2:-4]
                headers["Content-Encoding"] = "deflate"
            if self.path == "/empty-gzip":
                body, headers = b"", {"Content-Encoding": "gzip"}
            self.send_response(status)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body) + (5 if self.path == "/truncated" else 0)))
            for key, value in headers.items():
                self.send_header(key, value)
            if self.path == "/cookie/many":
                for cookie in ("a=1; Path=/", "b=2; Path=/", "deep=3; Path=/cookie", "a=updated; Path=/"):
                    self.send_header("Set-Cookie", cookie)
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
                ("deflate", "GET", "/deflate", 2, {}),
                ("raw-deflate", "GET", "/raw-deflate", 2, {}),
                ("empty-gzip", "GET", "/empty-gzip", 2, {}),
                *[(path, "GET", "/" + path, 2, {}) for path in
                  ("gzip-upper", "gzip-trailing", "gzip-truncated", "gzip-multi", "gzip-corrupt", "zstd", "zstd-truncated", "zstd-cut-last-byte", "zstd-cut-header", "zstd-corrupt", "redirect-bad-gzip", "retry-after-empty")],
                ("status-503", "GET", "/status-503", 2, {}),
                ("retry-after-get", "GET", "/retry-after", 2, {}),
                ("retry-after-post", "POST", "/retry-after", 2, {}),
                ("cookie-shared", "GET", "/cookie/read", 2, {}),
                ("cookie-order", "GET", "/cookie/read", 2, {}),
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
                    if name in {"cookie-shared", "cookie-order"}:
                        session.get(base + ("/cookie/set" if name == "cookie-shared" else "/cookie/many"),
                                    headers=executor.merge_headers(TDict({}), TDict({})))
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
                captured = normalized_records(records)
                cases.append({"name": name, "input": {"method": method, "path": path,
                              "max_retries": retries, "timeout": timeout, **kwargs},
                              "expected": {**expected, "records": captured}})
                if rust:
                    records.clear()
                    url = urlsplit(path)
                    req = {"method": method, "path": url.path, "headers": {"X-Test": "request"}}
                    if url.query:
                        req.update(qs=parse_qs(url.query, keep_blank_values=True, encoding="cp932"), url_encoding="cp932")
                    if "data" in kwargs:
                        req["raw" if isinstance(kwargs["data"], str) else "form"] = kwargs["data"]
                    if "json" in kwargs:
                        req["json"] = kwargs["json"]
                    steps = [{"path": "/cookie/set" if name == "cookie-shared" else "/cookie/many"}, req] if name in {"cookie-shared", "cookie-order"} else [req]
                    endpoint = {"name": "one", "host": base, "headers": {"X-Test": "endpoint"}}
                    if name in {"cookie-shared", "cookie-order"}:
                        endpoint["headers"] = {}
                    actual = rust_probe(rust, endpoint, steps, retries, timeout)["results"][-1]
                    comparable = {"error": actual["error"]} if "error" in actual else {
                        "status_code": actual["status_code"], "body_hex": actual["body_hex"],
                        "path": actual["url"].removeprefix(base)}
                    assert comparable == {k: v for k, v in expected.items() if k != "challenge"}, (name, actual, expected)
                    assert_records(records, captured, name)

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
                            if rust:
                                actual = rust_probe(rust, {"name": "one", "host": "http://fixture.invalid"},
                                                    [{"path": "/probe", "method": method}], retries, 0.2, injected=name)
                                assert actual["results"][0]["error"] == expected["error"], (name, actual, expected)
                                assert actual["attempts"] == expected["attempts"], (name, actual, expected)
        finally:
            server.shutdown()
            thread.join()
    if rust:
        golden = json.loads((Path(__file__).resolve().parents[1] / "fixtures/http-transport.json").read_text())
        assert cases == golden, "Python観測値が保存済みHTTP fixtureと異なる"
    return cases


if __name__ == "__main__":
    if "--rust" in sys.argv:
        binary = str(Path(sys.argv[sys.argv.index("--rust") + 1]).resolve())
        cases = transport(binary)
        print(f"HTTP transport: {len(cases)}/{len(cases)} Python一致")
    else:
        cases = timestamps() if "--time" in sys.argv else prepared() if "--prepare" in sys.argv else encodings() if "--encoding" in sys.argv else transport()
        print(json.dumps(cases, ensure_ascii=False, indent=2))
