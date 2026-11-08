"""入力系アドオン・通知のPython実装を直接実行して比較基準を採取する。"""
import importlib
import json
import os
from pathlib import Path
import random
import sys
import tempfile
import io
import math
from contextlib import redirect_stderr
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401 -- 循環importの入口
from jumeaux.models import Log2ReqsAddOnPayload, Reqs2ReqsAddOnPayload, Notifier  # noqa: E402
from jumeaux.domain.config.vo import Config  # noqa: E402
from jumeaux.notification_handlers import create_notification_handler  # noqa: E402
from jumeaux.logger import init_logger  # noqa: E402

BASE = {"one": {"name": "one", "host": "http://one"}, "other": {"name": "other", "host": "http://other"}, "output": {"response_dir": "responses"}, "addons": {"log2reqs": {"name": "plain"}}, "title": "empty test", "notifiers": {"local": {"type": "slack", "channel": "#local"}}}
CASES = []


def add(name, layer, addon, config=None, **values):
    CASES.append({"name": name, "input": {"layer": layer, "addon": addon, "config": config or {}, **values}})


def log(name, addon, text, config=None, codec="utf8"):
    add(name, "log2reqs", addon, config, body_hex=text.encode(codec).hex())


for addon, text in [("plain", "/a?x=1\n"), ("csv", "name,GET,/a,x=1,H=one\n"), ("json", '[{"path":"/あ","json":{"n":18446744073709551617,"drop":null}}]'), ("yaml", '- path: /あ\n  qs: {a: [one, two]}\n')]:
    log(f"{addon}-default", addon, text)
    log(f"{addon}-empty", addon, "")
    log(f"{addon}-bom", addon, text, codec="utf-8-sig")
    for codec in ["utf-8-sig", "utf-16", "utf-32", "cp932", "euc-jp"]:
        log(f"{addon}-{codec}", addon, text, {"encoding": codec}, codec)
    add(f"{addon}-invalid-bytes", "log2reqs", addon, body_hex="ff")
for keep in [False, True]:
    log(f"plain-query-{keep}", "plain", "/x?a=&a=1&flag&b=%FF&z=a+b?ignored=1\n \t\n\n/a\r/b\r\n", {"keep_blank": keep})
    log(f"csv-query-{keep}", "csv", "a,GET,/x,a=&a=1&flag,H=&H=2&J=3\n", {"keep_blank": keep})
log("plain-guess", "plain", "/x?q=%82%A0\n/no-query\n/empty?\n", {"candidate_for_url_encodings": ["utf-8", "cp932"]})
log("plain-guess-invalid", "plain", "/x?q=%FF\n", {"candidate_for_url_encodings": ["utf-8"]})
log("plain-unknown-codec-no-query", "plain", "/no-query\n", {"candidate_for_url_encodings": ["unknown"]})
log("plain-unknown-codec-query", "plain", "/x?a=1\n", {"candidate_for_url_encodings": ["unknown"]})
for dialect, delim in [("excel", ","), ("excel-tab", "\t"), ("unix", ",")]:
    log(f"csv-{dialect}", "csv", delim.join(['"a""b\nc"', "POST", "/x", "q=x", "H=a&H=b"]) + "\n\n", {"dialect": dialect})
for label, text in [("short", "n,GET,/x\n"), ("missing-path", "n,GET\n"), ("too-many", "n,GET,/x,,,extra\n"), ("unclosed-quote", '"n,GET,/x'), ("quote-tail", '"n"tail,GET,/x\n'), ("blank-row", "\n\r\n"), ("empty-row", ",,\n"), ("bad-method", "n,PUT,/x\n"), ("spaces", "n, GET,/x\n")]:
    log("csv-"+label, "csv", text)
log("csv-unknown-dialect", "csv", "", {"dialect": "unknown"})
for addon in ["json", "yaml"]:
    for label, text in [("list-empty", "[]"), ("null", "null"), ("object", "{}"), ("missing-path", "[{}]"), ("null-path", '[{"path":null}]'), ("unknown-field", '[{"path":"/","unknown":1}]'), ("invalid-method", '[{"path":"/","method":"PUT"}]'), ("duplicates", '[{"path":"/first","path":"/last"}]')]:
        log(f"{addon}-{label}", addon, text)
log("yaml-merge", "yaml", '- &base {path: /a, qs: {x: [one]}}\n- <<: *base\n  path: /b\n')

REQS = [{"path": "/a", "name": "A", "qs": {"id": ["101"]}}, {"path": "/b", "qs": {"id": ["11"]}}, {"path": "/c", "qs": {"id": ["1"]}}]
for size in [-9223372036854775808, -5, -1, 0, 1, 3, 99]:
    add(f"head-{size}", "reqs2reqs", "head", {"size": size}, requests=REQS)
for times in [-5, 0, 1, 2, 4]:
    add(f"repeat-{times}", "reqs2reqs", "repeat", {"times": times}, requests=REQS)
for when in ["path == '/a'", "name is undefined", "missing", "missing is undefined", "missing.child", "missing | default(false)", "[]", "[1]", "0", "1", "None", "qs.id.0|int > 10"]:
    add("filter-"+when, "reqs2reqs", "filter", {"when": when}, requests=REQS)
for location in ["head", "tail"]:
    add("add-"+location, "reqs2reqs", "add", {"location": location, "reqs": [{"path": "/added"}]}, requests=REQS)
add("add-default", "reqs2reqs", "add", {"reqs": []}, requests=REQS)
for label, conditions in [
    ("first", [{"when": "qs.id.0|int > 100", "name": "first {{ path }}"}, {"when": "qs.id.0|int > 10", "name": "second"}, {"name": "last"}]),
    ("unchanged", [{"when": "false", "name": "no"}]), ("empty", [{"name": ""}]),
    ("missing", [{"name": "{{ missing }}"}]), ("missing-when", [{"when": "missing", "name": "no"}]),
    ("undefined-length", [{"name": "{{ name }} ({{ name|length }} bytes)"}])]:
    add("rename-"+label, "reqs2reqs", "rename", {"conditions": conditions}, requests=REQS)
add("replace-sequential", "reqs2reqs", "replace", {"items": [{"queries": {"id": ["999"], "new": ["1"]}, "headers": {"X": "a"}}, {"when": "qs.id == ['999']", "queries": {"new": ["2"]}, "headers": {"X": "b"}}, {"when": "false", "queries": {"no": ["no"]}}]}, requests=REQS)
add("replace-datetime", "reqs2reqs", "replace", {"items": [{"queries": {"t": ["$DATETIME(%Y-%m-%dT%H:%M:%S)(-60)", "literal"]}, "headers": {"X": "$DATETIME(%Y)(0)"}}]}, requests=REQS, now="2020-01-02T03:04:05+09:00")
for seed in [0, 1, 42, 4294967296, 18446744073709551615]:
    for count in [0, 1, 3, 20, 1000]:
        add(f"shuffle-{seed}-{count}", "reqs2reqs", "shuffle", count=count, seed=seed)
for addon, cfg in [("head", {"size": 2}), ("repeat", {"times": 4}), ("filter", {"when": "missing"}), ("add", {"reqs": []}), ("rename", {"conditions": []}), ("replace", {"items": []}), ("empty_guard", {})]:
    add(addon+"-empty", "reqs2reqs", addon, cfg, requests=[])
add("guard-not-empty", "reqs2reqs", "empty_guard", {"notifies": [{"notifier": "missing", "message": "never"}]}, requests=REQS)
add("guard-notifies", "reqs2reqs", "empty_guard", {"notifies": [{"notifier": "local", "message": "{{ title }} notify! {{ description is none }}"}]}, requests=[])
add("guard-nested-none", "reqs2reqs", "empty_guard", {"notifies": [{"notifier": "local", "message": "{{ one.proxy is none }} {{ output.logger is none }} {{ addons.log2reqs.config is none }} {{ notifiers.local.icon_url is none }}"}]}, requests=[])

for addon in ["json", "yaml"]:
    for label, request in [
        ("case", {"Path": "/a", "urlEncoding": "cp932"}),
        ("hyphen", {"Path": "/a", "url-encoding": "cp932"}),
        ("collision", {"Path": "/first", "path": "/last", "urlEncoding": "cp932", "url-encoding": "utf8"}),
        ("reverse-collision", {"path": "/first", "Path": "/last"}),
        ("trim", {"<Path>": "/a", "-urlEncoding-": "cp932"}),
        ("nested", {"Path": "/a", "Qs": {"Keep-Key": ["v"]}, "Headers": {"X-Key": "v"}, "Json": {"Keep-Key": {"Path": "v"}}, "Form": {"Other-Key": "v"}}),
        ("unknown", {"Path": "/a", "Unknown-Key": "v"}),
    ]:
        log(f"{addon}-normalize-{label}", addon, json.dumps([request]))

for container in ["json", "form"]:
    for label, body in [
        ("outside-64", "{n: 18446744073709551617, m: -9223372036854775809}"),
        ("limits-128", "{n: 340282366920938463463374607431768211455, m: -170141183460469231731687303715884105728}"),
        ("nested-large-integers", "{values: [18446744073709551617, {m: -9223372036854775809}]}"),
        ("explicit-large-integers", "{n: !!int 18446744073709551617, m: !!int -9223372036854775809}"),
    ]:
        log(f"yaml-{container}-{label}", "yaml", f"- path: /a\n  {container}: {body}\n")
for label, request in [
    ("path", "- path: !foo /a\n"),
    ("json-value", "- path: /a\n  json: {value: !foo x}\n"),
    ("form-map", "- path: /a\n  form: !foo {value: x}\n"),
    ("json-sequence", "- path: /a\n  json: {value: !foo [x]}\n"),
    ("quoted", '- path: /a\n  json: {value: !foo "x"}\n'),
]:
    log(f"yaml-custom-tag-{label}", "yaml", request)

for container in ["json", "form"]:
    for label, token in [("yes", "yes"), ("octal", "010"), ("underscore", "1_000"), ("sexagesimal", "12:30"), ("date", "2020-01-01"), ("inf", ".inf"), ("negative-inf", "-.Inf"), ("nan", ".nan")]:
        log(f"yaml-{container}-{label}", "yaml", f"- path: /a\n  {container}: {{value: {token}}}\n")
        CASES[-1]["input"]["scalar"] = container
        CASES[-1]["rust_expected"] = {"error": "ValueError"} if "inf" in label or label == "nan" else {"value": {"type": "str", "value": token}}
        CASES[-1]["difference"] = "docs/compat-spec.md: YAML 1.1本文の解釈差と非有限数の拒否"
log("yaml-nested-nonfinite", "yaml", "- path: /a\n  json: {nested: [{value: .nan}]}\n")
CASES[-1]["input"]["scalar"] = "json"  # 非有限値は下記の観測要約でJSON化する
CASES[-1]["input"]["nested_nonfinite"] = True
CASES[-1]["rust_expected"] = {"error": "ValueError"}
CASES[-1]["difference"] = "docs/compat-spec.md: JSONで表現できない数値を拒否"
log("yaml-quoted-nonfinite", "yaml", '- path: /a\n  json: {value: ".inf", other: ".nan"}\n')
for label, text, extra in [
    ("python-tag", "- path: !!python/str a\n", {"path": "a"}),
    ("query-octal", "- path: /a\n  qs: {a: [010]}\n", {"qs": {"a": ["010"]}}),
    ("header-on", "- path: /a\n  headers: {on: x}\n", {"headers": {"on": "x"}}),
]:
    log("yaml-permissive-"+label, "yaml", text)
    CASES[-1]["rust_expected"] = {"value": [{"method": "GET", "path": "/a", "qs": {}, "headers": {}, "url_encoding": "utf-8", **extra}]}
    CASES[-1]["difference"] = "docs/compat-spec.md: YAML入力でRustが広く受理する差異"

for version in [2, 3]:
    add(f"guard-cli-v{version}-missing-webhook", "guard_cli", "empty_guard", {"notifies": [{"notifier": "local", "message": "hello"}]}, version=version)

RECORDS = []


class Receiver(BaseHTTPRequestHandler):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        RECORDS.append({"payload": json.loads(raw), "raw": raw.decode(), "content_type": self.headers.get("Content-Type")})
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, *_):
        pass


def outcome(case, webhook):
    p = case["input"]
    if p["layer"] == "guard_cli":
        from copy import deepcopy
        config = deepcopy(BASE)
        config["notifiers"]["local"]["version"] = p["version"]
        stderr = io.StringIO()
        with patch.dict(os.environ, {}, clear=True), redirect_stderr(stderr):
            init_logger(0)
            try:
                importlib.import_module("jumeaux.addons.reqs2reqs.empty_guard").Executor(p["config"]).exec(Reqs2ReqsAddOnPayload.from_dict({"requests": []}), Config.from_dict(config))
            except SystemExit as error:
                return {"value": {"exit_code": error.code, "stderr": stderr.getvalue()}}
    executor = importlib.import_module(f"jumeaux.addons.{p['layer']}.{p['addon']}").Executor(p["config"])
    if p["layer"] == "log2reqs":
        with tempfile.NamedTemporaryFile() as file:
            file.write(bytes.fromhex(p["body_hex"]))
            file.flush()
            requests = executor.exec(Log2ReqsAddOnPayload.from_dict({"file": file.name})).to_dicts()
            if "scalar" in p:
                value = requests[0][p["scalar"]]["nested"][0]["value"] if p.get("nested_nonfinite") else requests[0][p["scalar"]]["value"]
                return {"value": {"type": type(value).__name__, "value": repr(value) if isinstance(value, float) and not math.isfinite(value) else str(value) if type(value).__name__ == "date" else value}}
            return {"value": requests}
    requests = p.get("requests", [{"path": f"/{i}"} for i in range(p.get("count", 0))])
    payload = Reqs2ReqsAddOnPayload.from_dict({"requests": requests})
    if "seed" in p:
        random.seed(p["seed"])
    clock = datetime.fromisoformat(p.get("now", "2020-01-02T03:04:05+09:00")).replace(tzinfo=None)
    with patch("jumeaux.utils.datetime") as dt:
        dt.now.return_value = clock
        try:
            requests = executor.exec(payload, Config.from_dict(BASE)).requests.to_dicts()
            return {"value": [r["path"] for r in requests] if p["addon"] == "shuffle" else requests}
        except SystemExit as e:
            return {"value": {"exit_code": e.code, "notifications": [r["payload"] for r in RECORDS]}}


def main():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    webhook = f"http://127.0.0.1:{server.server_port}/webhook"
    try:
        with patch.dict(os.environ, {"SLACK_INCOMING_WEBHOOKS_URL": webhook}, clear=True):
            for case in CASES:
                RECORDS.clear()
                try:
                    case["expected"] = outcome(case, webhook)
                except Exception as error:
                    case["expected"] = {"error": type(error).__name__}
            for version, extras in [(1, {}), (1, {"icon_emoji": "owl", "icon_url": "https://example.test/icon"}), (2, {}), (2, {"use_blocks": True})]:
                for message in (["hello あ", ""] if not extras.get("use_blocks") else ['[{"type":"section","text":{"type":"plain_text","text":"あ"},"drop":null}]', '[]', 'null', '[null]', '[[null],{"a":null}]']):
                    for ts in [None, "123.456", ""]:
                        RECORDS.clear()
                        if ts is None:
                            os.environ.pop("SLACK_THREAD_TS", None)
                        else:
                            os.environ["SLACK_THREAD_TS"] = ts
                        notifier = {"type": "slack", "version": version, "channel": "#local", **extras}
                        try:
                            error = create_notification_handler(Notifier.from_dict(notifier)).notify(message).get()
                            expected = {"value": {**RECORDS[0], "error": error}}
                        except Exception as error:
                            expected = {"error": type(error).__name__}
                        CASES.append({"name": f"slack-{len(CASES)}", "input": {"layer": "notification", "notifier": notifier, "message": message, "thread_ts": ts}, "expected": expected})
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    output = ROOT / "compat/fixtures/inputs.json"
    output.write_text("[\n" + ",\n".join("  " + json.dumps(case, ensure_ascii=False) for case in CASES) + "\n]\n")
    print(f"{len(CASES)} fixtures: {output}")


if __name__ == "__main__":
    main()
