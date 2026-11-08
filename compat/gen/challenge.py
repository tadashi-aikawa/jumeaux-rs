"""基準Pythonの保存アドオン・key・prop bytes・challengeを実測する。JSONファイルは読まない。"""
from datetime import datetime, timedelta
import importlib
import json
import os
from pathlib import Path
import sys
import tempfile
from unittest.mock import patch
import warnings

SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons
from jumeaux import executor
from jumeaux.models import (Response, Trial, StoreCriterionAddOnPayload, StoreCriterionAddOnReference,
    DumpAddOnPayload, DidChallengeAddOnPayload, DidChallengeAddOnReference, to_json)
warnings.filterwarnings("ignore")
cases = []


def response(text='{"z":1,"a":"日本"}', kind="json", encoding="utf-8"):
    return {"body_hex": text.encode(encoding or "utf-8", errors="replace").hex(), "encoding": encoding,
            "type": kind, "headers": {"Content-Type": "application/" + kind}, "url": "http://localhost/a",
            "status_code": 200, "elapsed": 0.123456, "elapsed_sec": 0.12}


def load_response(data):
    data = dict(data)
    data["body"] = bytes.fromhex(data.pop("body_hex"))
    data["elapsed"] = timedelta(seconds=data["elapsed"])
    return Response.from_dict(data)


def capture(name, data, run):
    try:
        expected = {"ok": run()}
    except Exception as error:
        expected = {"error": type(error).__name__}
    cases.append({"name": name, "input": data, "expected": expected})


for when in [None, [], ["False"], ["False", "status == 'different'"], ["True", "1 / 0"],
             ["req.name == 'named' and req.path == '/a' and req.qs.q == ['1']"],
             ["req.method == 'GET' and req.raw is none and req.url_encoding == 'utf8'"],
             ["res_one.encoding == 'utf-8' and res_one.headers['content-TYPE'] == 'application/json'"],
             ["res_one.text is undefined"], ["1 / 0"], ["''"], ["'yes'"]]:
    for stored in [False, True]:
        config = {} if when is None else {"when_any": when}
        data = {"layer": "store", "config": config, "stored": stored, "response": response()}
        def run():
            mod = importlib.import_module("jumeaux.addons.store_criterion.free")
            ref = StoreCriterionAddOnReference.from_dict({"status": "different",
                "req": {"name": "named", "path": "/a", "qs": {"q": ["1"]}, "headers": {}},
                "res_one": load_response(data["response"]), "res_other": load_response(data["response"])})
            return mod.Executor(config).exec(StoreCriterionAddOnPayload.from_dict({"stored": stored}), ref).stored
        capture("store-" + str(len(cases)), data, run)


def dump(name, steps, res, body=None, encoding="original"):
    data = {"layer": "dump", "steps": steps, "response": res,
            "body_hex": res["body_hex"] if body is None else body.hex(),
            "encoding": res["encoding"] if encoding == "original" else encoding}
    def run():
        payload = DumpAddOnPayload.from_dict({"response": load_response(res), "body": bytes.fromhex(data["body_hex"]), "encoding": data["encoding"]})
        for step in steps:
            mod = importlib.import_module("jumeaux.addons.dump." + step["name"])
            payload = mod.Executor(step.get("config", {})).exec(payload)
        return {"body_hex": payload.body.hex(), "encoding": payload.encoding.get()}
    capture(name, data, run)


for kind, text in [("json", '{"z": [1E2,-0,-0.0,1e-7,1e20,900719925474099312345,null],"a":"日本"}'),
                   ("xml", '<root z="1"><a>日本</a><empty/></root>'),
                   ("html", '<html><head><title>日本</title></head><body><p>hello</p></body></html>')]:
    for encoding in ["utf-8", "cp932", "utf-16", None]:
        dump(f"dump-{kind}-{encoding}", [{"name": kind}], response(text, kind, encoding))
    dump(f"dump-{kind}-skip", [{"name": kind}], response("bad", "plain", None))
    dump(f"dump-{kind}-force", [{"name": kind, "config": {"force": True}}], response(text, "plain"))
    dump(f"dump-{kind}-invalid", [{"name": kind}], response("<bad", kind))
    dump(f"dump-{kind}-chain", [{"name": "encoding", "config": {"encoding": "utf-16"}}, {"name": kind}], response(text, kind))
dump("dump-encoding-none", [{"name": "encoding", "config": {"encoding": "utf-8"}}], response("hi", encoding=None))
dump("dump-encoding-invalid", [{"name": "encoding", "config": {"encoding": "ascii"}}], response("日本"))
dump("dump-encoding-decode-error", [{"name": "encoding", "config": {"encoding": "utf-8"}}], response(), b"\xff")
dump("dump-json-replace", [{"name": "json"}], response(), b'{"a":"\xff"}')
dump("dump-json-payload", [{"name": "json"}], response(), b'{"different":true}')
for index, text in enumerate(["", "<", "<html", "<head", "<p", "<img", '<p attr="', '<p attr="x"', "<!--", "<!DOCTYPE", "<?xml", "</p"]):
    dump(f"dump-html-incomplete-{index}", [{"name": "html"}], response(text, "html"))

trial = {"seq": 3, "name": "named", "tags": ["base"], "headers": {}, "queries": {}, "method": "GET", "path": "/a",
         "request_time": "2000-01-01T00:00:00+09:00", "status": "different", "diffs_by_cognition": {},
         "one": {"url": "http://localhost/a", "type": "json", "file": "one/(3)named"},
         "other": {"url": "http://localhost/b", "type": "json"}}
for conditions in [[], [{"tag": "always"}], [{"tag": "{{ trial.name }}", "when": "trial.status == 'different'"}],
                   [{"tag": "no", "when": "False"}], [{"tag": "yes", "when": ""}],
                   [{"tag": "base"}, {"tag": "{{ trial.tags|length }}"}],
                   [{"tag": "{{ res_one_props.n }}", "when": "res_one.encoding == 'utf-8' and res_other_props is none"}],
                   [{"tag": "saved", "when": "trial.one.file == 'one/(3)named' and res_one.text is undefined"}],
                   [{"tag": "oops", "when": "1 / 0"}]]:
    data = {"layer": "tag", "config": {"conditions": conditions}, "trial": trial, "response": response(), "props": {"n": 100.0}}
    def run():
        from jumeaux.addons.did_challenge.tag import Executor
        ref = DidChallengeAddOnReference.from_dict({"res_one": load_response(data["response"]), "res_other": load_response(data["response"]), "res_one_props": data["props"]})
        return Executor(data["config"]).exec(DidChallengeAddOnPayload.from_dict({"trial": trial}), ref).trial.to_dict()
    capture("tag-" + str(len(cases)), data, run)

for low, high, unit in [(0, 0, 0.5), (0.125, 0.125, 0.7), (1, 3, 0.25), (3, 1, 0.25), (-1, -1, 0.5)]:
    data = {"layer": "sleep", "config": {"min": float(low), "max": float(high)}, "unit": unit}
    def run():
        from jumeaux.addons.did_challenge.sleep import Executor
        seconds = low + (high - low) * unit
        def sleep_check(sec):
            if sec < 0:
                raise ValueError("sleep length must be non-negative")
        with patch("random.uniform", return_value=seconds), patch("time.sleep", side_effect=sleep_check) as sleep:
            Executor(data["config"]).exec(DidChallengeAddOnPayload.from_dict({"trial": trial}), None)
            return sleep.call_args.args[0]
    capture("sleep-" + str(len(cases)), data, run)

for text in ['{"z":[1E2,-0,-0.0,1e-7,1e20,1e16,1e-4,900719925474099312345],"a":"日本"}',
             '{"null":null,"list":[null,{"a":null,"b":1}],"empty":[]}', '[]', '{}', 'false', '0', '"x"']:
    capture("props-" + str(len(cases)), {"layer": "props", "text": text}, lambda: to_json(json.loads(text)).encode("utf-8", errors="replace").hex())

for now in ["2000-01-01T00:00:00+09:00", "2000-01-01T00:00:00.000001+09:00", "2026-11-08T12:34:56.123456+00:00", "1999-12-31T23:59:59.990000-05:30"]:
    for args in ["", "Namespace(files=['日本'], threads=2)"]:
        def run():
            with patch.object(executor, "now", return_value=datetime.fromisoformat(now)):
                return executor.hash_from_args(args)
        capture("key-" + str(len(cases)), {"layer": "key", "now": now, "args": args}, run)

# 本物のchallengeで保存ファイル名・Trialを採る。HTTPだけをローカル値へ差し替える。
for seq, name in [(1, None), (12, "名前 (a).json"), (2, ""), (4, "nested/name")]:
    data = {"layer": "filename", "seq": seq, "name": name}
    def run():
        import requests
        from jumeaux.models import Addons
        raw = requests.Response()
        raw.status_code = 200
        raw._content = b'{"a":1}'
        raw.encoding = "utf-8"
        raw.headers["Content-Type"] = "application/json"
        raw.url = "http://localhost/a"
        raw.elapsed = timedelta(0)
        addons = Addons.from_dict({"log2reqs": {"name": "plain"}, "res2dict": [{"name": "json"}], "store_criterion": [{"name": "free"}]})
        with tempfile.TemporaryDirectory() as temp:
            for side in ["one", "other", "one-props", "other-props"]:
                (Path(temp) / "key" / side).mkdir(parents=True)
            with patch.object(executor, "global_addon_executor", jumeaux.addons.AddOnExecutor(addons), create=True), patch.object(executor, "concurrent_request", return_value=(raw, raw)), patch.object(executor, "now", return_value=datetime.fromisoformat("2000-01-01T00:00:00+09:00")):
                result = executor.challenge({"seq": seq, "number_of_request": 1, "key": "key", "session": object(), "req": {"name": name, "path": "/a"}, "host_one": "http://localhost", "host_other": "http://localhost", "headers_one": {}, "headers_other": {}, "res_dir": temp, "judge_response_header": False, "ignore_response_header_keys": []})
            return {"trial": result, "files": sorted(str(p.relative_to(Path(temp) / "key")) for p in (Path(temp) / "key").rglob("*") if p.is_file())}
    capture("filename-" + str(seq), data, run)

def integrated(name, addons, one, other, req=None, headers=False):
    data = {"layer": "integrated", "addons": {"log2reqs": {"name": "plain"}, **addons},
            "one": one, "other": other, "request": req or {"path": "/a", "name": "integration"}, "headers": headers}
    def run():
        import requests
        from jumeaux.models import Addons
        def raw(res):
            result = requests.Response()
            result.status_code = res["status_code"]
            result._content = bytes.fromhex(res["body_hex"])
            result.encoding = res["encoding"]
            result.headers.update(res["headers"])
            result.url = res["url"]
            result.elapsed = timedelta(seconds=res["elapsed"])
            return result
        with tempfile.TemporaryDirectory() as temp:
            for side in ["one", "other", "one-props", "other-props"]:
                (Path(temp) / "key" / side).mkdir(parents=True)
            with patch.object(executor, "global_addon_executor", jumeaux.addons.AddOnExecutor(Addons.from_dict(data["addons"])), create=True), patch.object(executor, "concurrent_request", return_value=(raw(one), raw(other))), patch.object(executor, "now", return_value=datetime.fromisoformat("2000-01-01T00:00:00+09:00")):
                result = executor.challenge({"seq": 1, "number_of_request": 1, "key": "key", "session": object(), "req": data["request"], "host_one": "http://localhost", "host_other": "http://localhost", "headers_one": {}, "headers_other": {}, "res_dir": temp, "judge_response_header": headers, "ignore_response_header_keys": []})
            return {"trial": result, "files": {str(p.relative_to(Path(temp) / "key")): p.read_bytes().hex() for p in sorted((Path(temp) / "key").rglob("*")) if p.is_file()}}
    capture(name, data, run)


common = {"res2dict": [{"name": "json"}], "store_criterion": [{"name": "free"}], "dump": [{"name": "json"}]}
integrated("integration-typed-reference", {**common,
    "res2res": [{"name": "type", "config": {"conditions": [{"type": "json", "when": "True"}]}}],
    "judgement": [{"name": "same", "config": {"when_any": ["res_one.type == 'json' and res_other.type == 'json'"]}}],
    "store_criterion": [{"name": "free", "config": {"when_any": ["status == 'same' and res_one.type == 'json' and req.method == 'GET' and req.raw is none"]}}],
    "dump": [{"name": "json"}, {"name": "encoding", "config": {"encoding": "utf-16"}}],
    "did_challenge": [{"name": "tag", "config": {"conditions": [
        {"when": "trial.one.file is defined and res_one.type == 'json' and res_one_props.a == 100", "tag": "{{trial.name}}:{{res_one_props.a}}"},
        {"tag": "{{trial.tags|length}}"}]}}, {"name": "tag", "config": {"conditions": [{"tag": "{{trial.tags|length}}"}]}}]},
    response('{"a":1E2}', "plain"), response('{"a":200}', "plain"),
    {"path": "/a", "name": "POST 日本", "method": "POST", "raw": "x", "headers": {"X-Test": "v"}, "qs": {"q": ["x", "y"]}}, True)
integrated("integration-deduplicate-tags", {**common,
    "res2res": [{"name": "json_sort", "config": {"footprints_tag": "sorted", "items": [{"when": "True", "targets": [{"path": "root"}]}]}}],
    "did_challenge": [{"name": "tag", "config": {"conditions": [{"tag": "sorted"}]}}]},
    response('[3,2,1]'), response('[2,1,3]'))
integrated("integration-no-store-skips-dump", {"store_criterion": [], "dump": [{"name": "json", "config": {"force": True}}]}, response("bad", "plain"), response("bad", "plain"))
integrated("integration-unparsed-body", {"store_criterion": [{"name": "free"}]}, response("hello", "plain"), response("world", "plain"))
integrated("integration-prop-null-and-numbers", common, response('{"n":[null,1E2,-0,-0.0],"a":null}'), response('{"n":[null,1E2,-0,-0.0],"a":null}'))
integrated("integration-scalar-prop-error", common, response("42"), response("42"))

for case in cases:
    if case["expected"] == {"error": "ZeroDivisionError"}:
        case["rust_expected"] = {"error": "TypeError"}
        case["difference"] = "docs/compat-spec.md: 未定義値・異種比較の承認済み例外種別の差異"
print(json.dumps(cases, ensure_ascii=False, indent=2))
