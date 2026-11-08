"""Python基準の応答変換・judgementを実行し、入出力fixtureをstdoutへ採取する。

既存JSONファイルは読まない。基準リポジトリも変更しない。
"""
import copy
from datetime import timedelta
import importlib
import json
import math
import os
from pathlib import Path
import re
import sys
import warnings

SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from jumeaux.models import Response, Request, Res2ResAddOnPayload, Res2DictAddOnPayload, JudgementAddOnPayload, JudgementAddOnReference  # noqa: E402
from jumeaux import utils  # noqa: E402
from deepdiff import DeepDiff  # noqa: E402
warnings.filterwarnings("ignore")

cases = []


def response(text="{}", kind="json", encoding="utf-8", headers=None):
    return {"body_hex": text.encode(encoding or "utf-8", errors="replace").hex(), "type": kind,
            "encoding": encoding, "headers": headers if headers is not None else {"Content-Type": "application/" + kind},
            "url": "http://example.test/test", "status_code": 200, "elapsed": 0.123456, "elapsed_sec": 0.12}


def load_response(data):
    values = dict(data)
    values["body"] = bytes.fromhex(values.pop("body_hex"))
    values["elapsed"] = timedelta(seconds=values["elapsed"])
    return Response.from_dict(values)


def dump_response(res):
    return {"body_hex": res.body.hex(), "type": res.type, "encoding": res.encoding.get(),
            "headers": dict(res.headers), "url": res.url, "status_code": res.status_code,
            "elapsed": res.elapsed.total_seconds(), "elapsed_sec": res.elapsed_sec}


def portable(value):
    """Rustで表せないPython観測値も標準JSONのタグとして保存する。"""
    if isinstance(value, float) and not math.isfinite(value):
        return {"nonfinite": "NaN" if math.isnan(value) else "Infinity" if value > 0 else "-Infinity"}
    if isinstance(value, str) and any(0xd800 <= ord(c) <= 0xdfff for c in value):
        return {"surrogate_codepoints": [ord(c) for c in value]}
    if isinstance(value, dict):
        return {key: portable(item) for key, item in value.items()}
    if isinstance(value, list):
        return [portable(item) for item in value]
    return value


def capture(name, layer, addon, config, payload, reference=None, summary=None):
    data = {"layer": layer, "addon": addon, "config": config, "payload": payload}
    if reference is not None:
        data["reference"] = reference
    if summary:
        data["summary"] = summary
    try:
        executor = importlib.import_module(f"jumeaux.addons.{layer}.{addon}").Executor(config) if layer in ("res2res", "res2dict", "judgement") else None
        source = copy.deepcopy(payload)
        if layer == "when":
            value = utils.when_filter(config["expression"], {"x": json.loads(source["text"])})
        elif layer == "diff":
            diff = DeepDiff(json.loads(source["text"]), 0)
            groups = {"changed": ("type_changes", "values_changed"), "added": ("dictionary_item_added", "iterable_item_added"), "removed": ("dictionary_item_removed", "iterable_item_removed")}
            value = {name: sorted(utils.to_jumeaux_xpath(path) for kind in kinds for path in diff.get(kind, {})) for name, kinds in groups.items()}
        elif layer == "res2res":
            source["response"] = load_response(source["response"])
            result = executor.exec(Res2ResAddOnPayload.from_dict(source))
            value = {"response": dump_response(result.response), "req": result.req.to_dict(force_value=True), "tags": result.tags}
        elif layer == "res2dict":
            source["response"] = load_response(source["response"])
            result = executor.exec(Res2DictAddOnPayload.from_dict(source))
            value = {"response": dump_response(result.response)}
            if not result.result.is_none():
                value["result"] = result.result.get()
                if summary == "depth":
                    leaf, depth = value["result"], 0
                    while isinstance(leaf, list):
                        depth += 1
                        leaf = leaf[0]
                    value["result"] = {"depth": depth, "leaf": leaf}
        else:
            ref = copy.deepcopy(reference)
            for key in ["res_one", "res_other"]:
                ref[key] = load_response(ref[key])
            result = executor.exec(JudgementAddOnPayload.from_dict(source), JudgementAddOnReference.from_dict(ref))
            value = result.to_dict(force_value=True)
        # tuple-keyなどJSON保存時の失敗も境界契約に含める。
        json.dumps(value)
        expected = {"value": portable(value)}
    except Exception as error:
        expected = {"error": type(error).__name__}
    cases.append({"name": name, "input": data, "expected": expected})
    return cases[-1]


for kind, text in [("json", '{"z":null,"a":[2,1,null]}'), ("xml", '<root><a>1</a><a>2</a><empty/></root>'), ("html", '<html><body><p id="x"> hello </p></body></html>')]:
    for response_kind in [kind, "plain", "problem+" + kind, kind.upper()]:
        for force in [False, True]:
            for existing in [None, {}, [], {"kept": 1}]:
                capture(f"{kind}-{response_kind}-force{force}-existing{existing}", "res2dict", kind,
                        {"force": force}, {"response": response(text, response_kind), "result": existing})
    capture(kind + "-invalid", "res2dict", kind, {"force": True}, {"response": response("", kind)})
    capture(kind + "-type-overrides-content-type", "res2dict", kind, {}, {"response": response(text, kind, headers={"Content-Type": "text/plain"})})
    capture(kind + "-content-type-does-not-override-type", "res2dict", kind, {}, {"response": response(text, "plain", headers={"Content-Type": "application/" + kind})})

for literal in ["null", "false", "0", '"abc"', '""', "[]", "{}", "[18446744073709551617,1.5]", "{", "NaN"]:
    case = capture("json-root-" + literal, "res2dict", "json", {}, {"response": response(literal)})
    if literal == "NaN":
        case.update(rust_expected={"error": "ValueError"}, difference="compat-spec.md: 非有限数は入口でValueError")

block_config = {"header_regexp": r"^\[(.*)\]$", "record_regexp": r"^(\w+)=(.*)$"}
for name, text, config in [
    ("basic", "[one]\na=1\nb=2\n\n[two]\na=3", block_config),
    ("crlf", "[one]\r\na=1\r\n\r\n[two]\r\na=2\r\n", block_config),
    ("unicode-lines", "[one]\u2028a=1\u2029\u2029[two]\u0085b=2", block_config),
    ("no-header", "a=1\n", block_config),
    ("overwrite", "[one]\na=1\na=2\n\n[one]\nb=3", block_config),
    ("empty-record", "[empty]\n\n[one]\na=\nignored\n \n", block_config),
    ("no-blank-between-headers", "[one]\na=1\n[two]\nb=2", block_config),
    ("multiple-matches", "[one][two]\na=1 b=2", {"header_regexp": r"\[(.*?)\]", "record_regexp": r"(\w)=(\w)"}),
    ("one-group", "[one]\nabc\nz", {**block_config, "record_regexp": r"^(\w+)$"}),
    ("no-group", "[one]\nabc\nz", {**block_config, "record_regexp": r"^\w+$"}),
    ("optional-group", "[one]\na", {**block_config, "record_regexp": r"^(\w)(?:=(\w))?$"}),
    ("tuple-header", "[one:two]\na=1", {**block_config, "header_regexp": r"^\[(.*):(.*)\]$"}),
    ("empty-match", "[one]\na", {**block_config, "record_regexp": r"^"}),
    ("lookbehind", "[one]\n:a=1", {**block_config, "record_regexp": r"(?<=:)(\w)=(\w)"}),
    ("empty-then-nonempty", "[one]\na", {**block_config, "record_regexp": r"^|."}),
    ("nonempty-then-empty", "[one]\na", {**block_config, "record_regexp": r".*"}),
]:
    capture("block-" + name, "res2dict", "block", config, {"response": response(text, "plain")})
for force in [False, True]:
    capture("block-existing-" + str(force), "res2dict", "block", {**block_config, "force": force}, {"response": response("[one]\na=1", "plain"), "result": {}})


def res_payload(text, kind="json", **kwargs):
    return {"response": response(text, kind, **kwargs), "req": {"name": "req", "path": "/test", "qs": {"q": ["x"]}}, "tags": ["before"]}


def sort_config(targets=None, when="True", tag="sorted"):
    return {"items": [{"when": when, "targets": targets if targets is not None else [{"path": ".*"}]}], "footprints_tag": tag}


for name, text, config in [
    ("numbers", "[3,1,2]", sort_config()),
    ("nested", '{"items":[{"id":2,"values":[3,1]},{"id":1,"values":[4,2]}]}', sort_config()),
    ("multi-keys-stable", '[{"a":1,"b":2,"id":"first"},{"a":1,"b":1},{"a":1,"b":2,"id":"last"}]', sort_config([{"path": "root", "sort_keys": ["a", "b"]}])),
    ("empty-keys", '[{"a":2},{"a":1}]', sort_config([{"path": "root", "sort_keys": []}])),
    ("first-target-wins", '[{"a":2,"b":1},{"a":1,"b":2}]', sort_config([{"path": "root", "sort_keys": ["a"]}, {"path": "root", "sort_keys": ["b"]}])),
    ("when-false", "[3, 1]", sort_config(when="False")),
    ("when-request", "[3,1]", sort_config(when="path == '/test' and name == 'req' and qs.q[0] == 'x'")),
    ("unchanged", "[1,2]", sort_config()),
    ("objects-default", '[{"z":0},{"a":1},{"a":0}]', sort_config()),
    ("unicode", '["🦉","é","あ","a"]', sort_config()),
    ("mixed-numbers", "[2.5,true,1,false,0,1.5]", sort_config()),
    ("large-numbers", "[18446744073709551617,18446744073709551616,2]", sort_config()),
    ("float-integer-order", "[9007199254740993,9007199254740992.0,-1.5,-1,-2]", sort_config()),
    ("nulls", "[null,null]", sort_config()),
    ("mixed-string-number", '[1,"a"]', sort_config()),
    ("missing-key", '[{"a":1},{}]', sort_config([{"path": "root", "sort_keys": ["a"]}])),
    ("non-object-key", "[1,2]", sort_config([{"path": "root", "sort_keys": ["a"]}])),
    ("equal-dict-keys", '[{"a":{"x":1}},{"a":{"x":1}}]', sort_config([{"path": "root", "sort_keys": ["a"]}])),
    ("unequal-dict-keys", '[{"a":{"x":2}},{"a":{"x":1}}]', sort_config([{"path": "root", "sort_keys": ["a"]}])),
    ("string-root", '"cab"', sort_config()),
    ("number-root", "1", sort_config()),
    ("null-root", "null", sort_config()),
    ("scalar-when-false", "1", sort_config(when="False")),
    ("invalid-json", "{", sort_config()),
    ("path-raw-quote", '{"a\'b":[2,1]}', sort_config([{"path": "root<'a'b'>"}])),
    ("numbers-spelling", "[1e2,1.2300]", sort_config()),
    ("negative-zero", '[-0,{"a":-0},[-0],-0.0]', sort_config(when="False")),
    ("numeric-object-sort", '[{"v":2},{"v":1e20},{"v":1e2},{"v":100.0000}]', sort_config()),
    ("empty-items", "[3,1]", {"items": []}),
]:
    capture("sort-" + name, "res2res", "json_sort", config, res_payload(text))
for name, payload in [
    ("skip-non-json", res_payload("invalid", "plain")),
    ("no-encoding", res_payload("[2,1]", encoding=None)),
    ("cp932", res_payload('["あ","a"]', encoding="cp932")),
    ("utf16", res_payload("[2,1]", encoding="utf-16")),
]:
    capture("sort-" + name, "res2res", "json_sort", sort_config(), payload)
capture("sort-sequential", "res2res", "json_sort", {"items": [
    {"when": "True", "targets": [{"path": "root", "sort_keys": ["a"]}]},
    {"when": "True", "targets": [{"path": "root", "sort_keys": ["b"]}]},
]}, res_payload('[{"a":2,"b":1},{"a":1,"b":2}]'))

for name, conditions in [
    ("first", [{"type": "xml", "when": "True"}, {"type": "html", "when": "True"}]),
    ("fallback", [{"type": "xml", "when": "False"}, {"type": "html"}]),
    ("none", [{"type": "xml", "when": "False"}]),
    ("empty", []),
    ("empty-when", [{"type": "xml", "when": ""}]),
    ("request-response", [{"type": "xml", "when": "req.path == '/test' and res.status_code == 200 and res.type == 'json' and res.encoding == 'utf-8'"}]),
    ("no-properties", [{"type": "xml", "when": "res.text is undefined and res.ok is undefined"}]),
    ("body-bytes", [{"type": "xml", "when": "res.body == b'{}'"}]),
    ("elapsed", [{"type": "xml", "when": "res.elapsed.total_seconds() > 0"}]),
    ("headers", [{"type": "xml", "when": "res.headers['Content-Type'] == 'application/json'"}]),
    ("headers-mixed-case", [{"type": "xml", "when": "res.headers['cOnTeNt-TyPe'] == 'application/json'"}]),
]:
    capture("type-" + name, "res2res", "type", {"conditions": conditions}, res_payload("{}"))

reference = {"name": "trial", "path": "/same", "qs": {}, "headers": {},
             "res_one": response('{"a":1,"gone":3}'), "res_other": response('{"a":2,"new":4}'),
             "dict_one": {"a": 1, "gone": 3}, "dict_other": {"a": 2, "new": 4}}
unknown = {"added": ["root<'new'>"], "changed": ["root<'a'>"], "removed": ["root<'gone'>"]}
payload = {"diffs_by_cognition": {"unknown": unknown}, "regard_as_same_body": False, "regard_as_same_header": False}
all_cases = {kind: [{"path": ".*"}] for kind in ["added", "changed", "removed"]}


def ignores(conditions, title="known"):
    return {"ignores": [{"title": title, "conditions": conditions}]}


for name, config in [
    ("all", ignores([all_cases])),
    ("when-false", ignores([{**all_cases, "when": "False"}])),
    ("only-added", ignores([{"added": [{"path": ".*", "when": "one is none and other == 4"}]}])),
    ("values", ignores([{"changed": [{"path": ".*", "when": "one == 1 and other == 2"}], "removed": [{"path": ".*", "when": "one == 3 and other is none"}]}])),
    ("case-when-false", ignores([{"changed": [{"path": ".*", "when": "False"}]}])),
    ("condition-context", ignores([{**all_cases, "when": "req.name == 'trial' and req.path == '/same' and res_one.ok and res_other.status_code == 200 and dict_one.get().a == 1 and not dict_other.is_none()"}])),
    ("duplicate-conditions", ignores([all_cases, all_cases])),
    ("priority", {"ignores": [{"title": "first", "conditions": [all_cases]}, {"title": "second", "conditions": [all_cases]}]}),
    ("same-title", {"ignores": [{"title": "known", "conditions": [{"added": [{"path": ".*"}]}]}, {"title": "known", "conditions": [{"changed": [{"path": ".*"}]}]}]}),
    ("reserved-unknown-title", ignores([all_cases], "unknown")),
    ("no-condition", ignores([])),
    ("empty-condition", ignores([{}])),
    ("no-ignores", {"ignores": []}),
    ("regex-short-circuit", ignores([{"changed": [{"path": "not-matched", "when": "missing.value == 1"}]}])),
]:
    capture("ignore-" + name, "judgement", "ignore", config, payload, reference)

for name, diffs, body_same in [("unset", None, False), ("empty", {}, False), ("empty-skip", {}, True), ("empty-unknown", {"unknown": {"added": [], "changed": [], "removed": []}}, False), ("already-same", {"unknown": unknown}, True)]:
    for addon, config in [("ignore", ignores([all_cases])), ("same", {"when_any": ["True"]})]:
        source = {"regard_as_same_body": body_same, "regard_as_same_header": True}
        if diffs is not None:
            source["diffs_by_cognition"] = diffs
        capture(addon + "-" + name, "judgement", addon, config, source, reference)

for name, expressions in [
    ("false", ["False"]), ("no-expressions", []), ("first-match", ["True", "missing.value == 1"]),
    ("fallback", ["False", "req.path == '/same'"]),
    ("response", ["res_one.text != res_other.text and res_one.encoding.get() == 'utf-8'"]),
    ("option", ["dict_one.get().a == 1 and dict_other.get_or({}).a == 2"]),
    ("option-value", ["dict_one.value.a == 1 and dict_other.value.a == 2"]),
    ("encoding-value", ["res_one.encoding.value == 'utf-8' and res_other.encoding.value == 'utf-8'"]),
    ("headers", ["res_one.headers['cOnTeNt-TyPe'] == 'application/json'"]),
    ("numeric", ["1"]), ("empty-string", ["''"]),
]:
    capture("same-" + name, "judgement", "same", {"when_any": expressions}, payload, reference)

for name, expression, ref in [
    ("any", "dict_one.any()", reference),
    ("map-option", "dict_one.map(dict_other.get_or).get().a == 2", reference),
    ("map-string", "res_one.encoding.map('utf-8'.startswith).get()", reference),
    ("map-attr", "res_one.encoding.map({'utf-8': True}|attr('get')).get()", reference),
    ("flat-map", "dict_one.flat_map(dict_other.map).is_none()", {**reference, "dict_other": None}),
]:
    capture("same-option-method-" + name, "judgement", "same", {"when_any": [expression]}, payload, ref)
    capture("ignore-option-method-" + name, "judgement", "ignore", ignores([{**all_cases, "when": expression}]), payload, ref)

for key, path in [("a.b", "root<'a.b'>"), ("root", "root<'root'>"), ("rooted", "root<'rooted'>"), ("a[0]", "root<'a[0]'>"), ("a'b", 'root<"a\'b">'), ('a\\b', "root<'a\\\\b'>")]:
    ref = {**reference, "dict_one": {key: 1}, "dict_other": {key: 2}}
    source = {**payload, "diffs_by_cognition": {"unknown": {"added": [], "changed": [path], "removed": []}}}
    case = capture("ignore-special-key-" + key, "judgement", "ignore", ignores([{"changed": [{"path": re.escape(path), "when": "one == 1 and other == 2"}]}]), source, ref)
    rust = {"value": {**source, "diffs_by_cognition": {"known": {"added": [], "changed": [path], "removed": []}}, "regard_as_same_body": True}}
    assert rust != case["expected"]
    case.update(rust_expected=rust, difference="compat-spec.md: 差分パスの値取得で特殊キーを辿る既存不具合の修正")

source = {**payload, "diffs_by_cognition": {"unknown": {"added": [], "changed": ["root"], "removed": []}}}
case = capture("ignore-root-alone", "judgement", "ignore", ignores([{"changed": [{"path": "root", "when": "one.a == 1 and other.a == 2"}]}]), source, reference)
case.update(rust_expected={"value": {**source, "diffs_by_cognition": {"known": {"added": [], "changed": ["root"], "removed": []}}, "regard_as_same_body": True}}, difference="compat-spec.md: root単独は入力全体を返す")

capture("same-option-value-none", "judgement", "same", {"when_any": ["dict_one.value is none and res_one.encoding.value is none"]}, payload,
        {**reference, "dict_one": None, "res_one": response(encoding=None)})

for token in ["NaN", "Infinity", "-Infinity", "1e400", "-1e400"]:
    for text in [token, '{"a":[' + token + ']}']:
        for layer, addon, config, source in [
            ("res2res", "json_sort", sort_config(when="False"), res_payload(text)),
            ("res2dict", "json", {}, {"response": response(text)}),
            ("when", "when", {"expression": "x == x"}, {"text": text}),
            ("diff", "diff", {}, {"text": text}),
        ]:
            case = capture(f"nonfinite-{layer}-{text}", layer, addon, config, source)
            case.update(rust_expected={"error": "ValueError"}, difference="compat-spec.md: 非有限数は入口でValueError")

for layer, addon, config, source in [
    ("res2res", "json_sort", sort_config(when="False"), res_payload('["\\ud800"]')),
    ("res2dict", "json", {}, {"response": response('["\\ud800"]')}),
]:
    case = capture("surrogate-" + layer, layer, addon, config, source)
    case.update(rust_expected={"error": "JSONDecodeError"}, difference="compat-spec.md: lone surrogateはRustで表せない入力")

for depth in [129, 200]:
    text = "[" * depth + "0" + "]" * depth
    capture(f"deep-json-{depth}", "res2dict", "json", {}, {"response": response(text)}, summary="depth")
    capture(f"deep-sort-{depth}", "res2res", "json_sort", sort_config(), res_payload(text))

for token in ["nan", "inf", "infinity"]:
    for text in [token, '{"a":[' + token + ']}']:
        capture("invalid-number-json-" + text, "res2dict", "json", {}, {"response": response(text)})
        capture("invalid-number-sort-" + text, "res2res", "json_sort", sort_config(), res_payload(text))

assert len({case["name"] for case in cases}) == len(cases)
print(json.dumps(cases, ensure_ascii=False, indent=2))
