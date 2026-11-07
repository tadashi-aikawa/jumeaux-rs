"""Python版の実関数を呼ぶfixture生成。移行元には書き込まない。"""

import argparse
from datetime import datetime, timedelta, timezone
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True

import jumeaux.addons  # noqa: E402,F401 -- 循環importの入口
from deepdiff import DeepDiff  # noqa: E402
from owlmixin import TList, TOption  # noqa: E402
from requests import Response as RequestsResponse  # noqa: E402
from requests.structures import CaseInsensitiveDict  # noqa: E402
from requests.utils import get_encoding_from_headers  # noqa: E402
from jumeaux import executor, utils  # noqa: E402
from jumeaux.addons.dump.encoding import Executor as DumpEncoding  # noqa: E402
from jumeaux.addons.dump.html import Executor as DumpHTML  # noqa: E402
from jumeaux.addons.log2reqs.plain import Executor as Plain, guess_url_encoding  # noqa: E402
from jumeaux.addons.res2dict.html import html_to_dict  # noqa: E402
from jumeaux.addons.res2dict.xml import Executor as XML  # noqa: E402
from jumeaux.domain.config.vo import MergedArgs  # noqa: E402
from jumeaux.models import DumpAddOnPayload, Log2ReqsAddOnPayload, Res2DictAddOnPayload, Response  # noqa: E402


def imported(relative):
    spec = importlib.util.spec_from_file_location("compat_source_" + relative.replace("/", "_"), SOURCE / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def params(function):
    mark = next(mark for mark in function.pytestmark if mark.name == "parametrize")
    return mark.args[1]


def outcome(function):
    try:
        return {"value": function()}
    except Exception as error:
        # 言語固有のスタックやメモリアドレスでなく、エラーの種類を契約にする。
        return {"error": type(error).__name__}


def response(body=b"abc", encoding="utf-8", kind="plain", status=200):
    return Response.from_dict({
        "body": body, "encoding": encoding, "type": kind,
        "headers": {"content-type": "text/" + kind}, "url": "http://localhost/test",
        "status_code": status, "elapsed": timedelta(0), "elapsed_sec": 0.0,
    })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=ROOT / "compat/fixtures")
    args = parser.parse_args()
    fixtures = {}

    def add(group, name, data, function, expected=None, reason=None):
        observed = outcome(function)
        row = {"name": name, "input": data, "expected": observed if reason is None else expected}
        if reason:
            assert observed != expected, name + ": Python不一致が再現していない"
            row.update(python_mismatch=True, reason=reason, python_observed=observed)
        fixtures.setdefault(group, []).append(row)

    def diff(one, other):
        result = DeepDiff(one, other)
        groups = {"changed": ("type_changes", "values_changed"), "added": ("dictionary_item_added", "iterable_item_added"), "removed": ("dictionary_item_removed", "iterable_item_removed")}
        return {name: sorted(utils.to_jumeaux_xpath(path) for kind in kinds for path in result.get(kind, {})) for name, kinds in groups.items()}

    pairs = [
        ("insert", [1, 2, 3], [0, 1, 2, 3]),
        ("delete", [1, 2, 3], [1, 3]),
        ("duplicates", [1, 1, 2], [1, 2, 2]),
        ("dict-array", [{"a": 1}, {"a": 2}], [{"a": 0}, {"a": 1}, {"a": 2}]),
        ("nested-array", [[1, 2], [3]], [[1, 0, 2], [3]]),
        ("replace-dict", {"a": 1}, {"b": 2}),
        ("int-float", {"n": 1}, {"n": 1.0}),
        ("bool-int", {"n": True}, {"n": 1}),
        ("dict-add-remove", {"a": 1, "b": 2}, {"b": 2, "c": 3}),
        ("string-insert", ["a", "b"], ["x", "a", "b"]),
        ("root-array", [], [1]),
        ("equal", {"a": [None, 1]}, {"a": [None, 1]}),
    ]
    for name, one, other in pairs:
        add("diff", name, {"operation": "diff", "one": one, "other": other}, lambda: diff(one, other))
    for key in ("root", "myroot", "a[0]", "a.b", "a'b", "a<b>"):
        data = {key: 42}
        path = "root<" + repr(key) + ">"
        add("diff", "lookup-" + key, {"operation": "get_by_diff_key", "dict": data, "path": path, "segments": [key]}, lambda: utils.get_by_diff_key(data, path), {"value": 42}, "差分パスの文字列置換でキー名が壊れる。元のキーを1要素として参照する。")
    add("diff", "lookup-normal", {"operation": "get_by_diff_key", "dict": {"items": [{"id": 7}]}, "path": "root<'items'><0><'id'>"}, lambda: utils.get_by_diff_key({"items": [{"id": 7}]}, "root<'items'><0><'id'>"))

    html_cases = [
        ("completion", "<p>hello"), ("empty-tags", "<p><br><img src='x'><input disabled></p>"),
        ("mixed-text", "<p>a<b>b</b>c</p>"), ("repeated", "<ul><li>a</li><li>b</li></ul>"),
        ("script", "<script>if (1 < 2) x = '&';</script>"),
        ("table-completion", "<table><tr><td>a</td></tr></table>"),
        ("unit-html", imported("tests/addons/res2dict/test_html.py").NORMAL_BODY),
    ]
    for name, body in html_cases:
        add("markup", name, {"format": "html", "body": body}, lambda: html_to_dict(body))
    xml_cases = [
        ("mixed-text", '<r a="1">before<c/>after</r>'),
        ("namespace", '<r xmlns="urn:default" xmlns:p="urn:test"><p:c p:a="1">x</p:c></r>'),
        ("cdata", "<r><![CDATA[a<b>&c]]></r>"),
        ("entity", "<r>&amp;&lt;&#65;&#x3042;</r>"),
        ("internal-entity", '<!DOCTYPE r [<!ENTITY x "hello">]><r>&x;</r>'),
        ("empty-repeat", '<r><c/><c a="1"/></r>'),
        ("unit-xml", imported("tests/addons/res2dict/test_xml.py").NORMAL_BODY),
    ]
    for name, body in xml_cases:
        add("markup", name, {"format": "xml", "body": body}, lambda: XML({}).exec(Res2DictAddOnPayload.from_dict({"response": response(body.encode(), kind="xml")})).result.get())
    for kind, body, expression in [
        ("html", "<p>a<b>b</b>c</p>", "dict_one.html.body.p.b['##value'] == 'b'"),
        ("xml", '<r a="1">before<c/>after</r>', "dict_one.r.c is none and dict_one.r['#text'] == 'beforeafter'"),
    ]:
        def converted_condition():
            result = html_to_dict(body) if kind == "html" else XML({}).exec(Res2DictAddOnPayload.from_dict({"response": response(body.encode(), kind="xml")})).result.get()
            return {"dict": result, "condition": utils.when_filter(expression, {"dict_one": result})}
        add("markup", kind + "-condition", {"format": kind, "body": body, "when": expression}, converted_condition)

    unit_utils = imported("tests/test_utils.py")
    for index, (_, expression) in enumerate(params(unit_utils.TestWhenFilter.test_normal)):
        data = unit_utils.TestWhenFilter.data
        add("template", f"unit-when-{index}", {"operation": "when", "expression": expression, "data": data}, lambda: utils.when_filter(expression, data))
    for expression in ("missing", "missing.name", "'False'", "none is none", "x.0|int == 2", "x|length == 2"):
        add("template", expression, {"operation": "when", "expression": expression, "data": {"x": ["2", "3"]}}, lambda: utils.when_filter(expression, {"x": ["2", "3"]}))
    for fmt in ("hello {{ missing }}", "{{ x.split(',')|join('/') }}", "{{ False }} {{ None }}", "{{ equals_without_host('https://a/x','https://b/x') }}"):
        add("template", fmt, {"operation": "render", "template": fmt, "data": {"x": "a,b"}}, lambda: utils.jinja2_format(fmt, {"x": "a,b"}))
    for status in (200, 201, 404):
        res = response("日本語".encode(), status=status)
        expression = "response.ok and response.text == '日本語'"
        add("template", f"response-properties-{status}", {"operation": "response_when", "expression": expression, "text": "日本語", "status_code": status}, lambda: utils.when_filter(expression, {"response": res}))
    fixed = datetime(2000, 1, 2, 3, 4, 5, tzinfo=timezone(timedelta(hours=9)))
    for dsl in ("$DATETIME(%Y-%m-%d)(-86400)", "$DATETIME(%Y-%m-%d %H:%M:%S)(86400)", "$DATETIME(%Y)(bad)", "literal"):
        with patch.object(utils, "now", return_value=fixed):
            add("template", dsl, {"operation": "datetime", "value": dsl, "now": fixed.isoformat()}, lambda: utils.parse_datetime_dsl(dsl))

    for index, (_, target, pattern) in enumerate(params(unit_utils.TestExactMatch.test_normal)):
        add("regex", f"unit-exact-{index}", {"operation": "exact_match", "target": target, "pattern": pattern}, lambda: utils.exact_match(target, pattern))
    add("regex", "exact-trailing-newline", {"operation": "exact_match", "target": "123\n", "pattern": r"\d+"}, lambda: utils.exact_match("123\n", r"\d+"))
    add("regex", "reg-trailing-newline", {"operation": "reg", "target": "123\n", "pattern": r"\d+"}, lambda: utils.when_filter("value|reg(pattern)", {"value": "123\n", "pattern": r"\d+"}))
    for name, pattern, replacement, target in [
        ("path-custom", r"([^-]+)-(\d).json", r"\1/case-\2.json", "same-1.json"),
        ("named-group", r"(?P<name>\w+)", r"\g<name>/", "abc"),
        ("lookahead", r"a(?=b)", "x", "ab ac"),
        ("backreference", r"(a)\1", "x", "aa ab"),
        ("invalid", "[", "x", "abc"),
    ]:
        add("regex", name, {"operation": "sub", "pattern": pattern, "replacement": replacement, "target": target}, lambda: re.sub(pattern, replacement, target))

    unit_models = imported("tests/test_models.py")
    for index, (_, headers, text, content, encoding, apparent, default, _) in enumerate(params(unit_models.TestModels.test_from_requests_decide_encoding)):
        res = SimpleNamespace(headers=headers, content=content, encoding=encoding, apparent_encoding=apparent)
        add("encoding", f"unit-decide-{index}", {"operation": "decide_encoding", "headers": headers, "body_hex": content.hex(), "encoding": encoding, "apparent_encoding": apparent, "default": default}, lambda: Response._decide_encoding(res, TOption(default)))
    for index, (content, _) in enumerate(params(unit_models.TestModels.test_get_encodings_from_content)):
        from jumeaux.models import get_encodings_from_content
        add("encoding", f"unit-content-{index}", {"operation": "content_encodings", "body_hex": content.hex()}, lambda: get_encodings_from_content(content))
    for name, ct, body, default in [
        ("explicit-latin1-skipped", "text/plain; charset=ISO-8859-1", b"abc", "cp932"),
        ("json-default-ignored", "application/json", b'{"a":1}', "cp932"),
        ("binary-precedence", "application/octet-stream; charset=UTF-8", b"abc", "cp932"),
        ("meta-before-default", "text/html", b'<meta charset="Shift_JIS">', "utf-8"),
        ("xml-before-default", "application/xml", b'<?xml version="1.0" encoding="EUC-JP"?><r/>', "utf-8"),
        ("apparent-utf8-bom", "application/xml", b'\xef\xbb\xbf<r>abc</r>', None),
    ]:
        res = RequestsResponse()
        res.headers = CaseInsensitiveDict({"content-type": ct})
        res._content = body
        res.encoding = get_encoding_from_headers(res.headers)
        add("encoding", name, {"operation": "requests_encoding", "content_type": ct, "body_hex": body.hex(), "default": default}, lambda: Response._decide_encoding(res, TOption(default)))
    for encoding in ("utf-8", "utf-8-sig", "utf-16", "shift_jis", "cp932", "euc-jp", "iso2022_jp"):
        body = "日本語".encode(encoding)
        add("encoding", "response-" + encoding, {"operation": "response_text", "body_hex": body.hex(), "encoding": encoding}, lambda: response(body, encoding).text)
    for name, body, encoding in [("invalid-response", b"\xff", "utf-8"), ("unknown-codec", b"abc", "no-such-codec")]:
        add("encoding", name, {"operation": "response_text", "body_hex": body.hex(), "encoding": encoding}, lambda: response(body, encoding).text)
    for name, body, source, dest in [
        ("dump-success", "日本語".encode(), "utf-8", "cp932"),
        ("dump-decode-fail", b"\xff", "utf-8", "cp932"),
        ("dump-encode-fail", "😀".encode(), "utf-8", "cp932"),
        ("dump-utf16", "日本語".encode(), "utf-8", "utf-16"),
    ]:
        payload = DumpAddOnPayload.from_dict({"response": response(body, source), "body": body, "encoding": source})
        add("encoding", name, {"operation": "dump_encoding", "body_hex": body.hex(), "source": source, "destination": dest}, lambda: DumpEncoding({"encoding": dest}).exec(payload).body.hex())
    for name, query, encodings in [("url-sjis", "q=%93%8c%8b%9e", ["utf-8", "sjis"]), ("url-fallback", "q=%ff", ["utf-8"]), ("url-order", "q=abc", ["euc-jp", "utf-8"])]:
        add("encoding", name, {"operation": "guess_url_encoding", "query": query, "candidates": encodings}, lambda: guess_url_encoding(query, TList(encodings)).get())
    body = b"<p>\xff</p>"
    payload = DumpAddOnPayload.from_dict({"response": response(body, "ascii", "html"), "body": body, "encoding": "ascii"})
    add("encoding", "dump-replacement-question-mark", {"operation": "dump_html", "body_hex": body.hex(), "encoding": "ascii"}, lambda: DumpHTML({}).exec(payload).body.hex())
    with tempfile.TemporaryDirectory(prefix="jumeaux-fixtures-") as directory:
        file = Path(directory) / "requests"
        for name, body, encoding in [("input-decode-fail", b"\xff", "utf-8"), ("input-sjis", "/x?q=東京".encode("cp932"), "cp932")]:
            file.write_bytes(body)
            add("encoding", name, {"operation": "plain_input", "body_hex": body.hex(), "encoding": encoding}, lambda: Plain({"encoding": encoding}).exec(Log2ReqsAddOnPayload.from_dict({"file": str(file)})).to_dicts())

    # retryの実処理を、送信直前の__runだけ差し替えて観測する。
    report = json.loads((ROOT / "compat/python-baseline/post/report.json").read_text())
    settings = {"judge_response_header": True, "ignore_response_header_keys": ["X-Ignore"]}
    captured = {}
    def capture(config, reqs, *_):
        captured.update(trials=[{"name": req.name.get(), "raw": req.raw.get(), "form": req.form.get(), "json": req.json.get()} for req in reqs], header_settings={"judge_response_header": config.judge_response_header, "ignore_response_header_keys": list(config.ignore_response_header_keys)})
    with patch.object(executor, "__run", side_effect=capture):
        executor.retry(args=MergedArgs.from_dict({}), report=str(ROOT / "compat/python-baseline/post/report.json"))
    fixtures["retry"] = [{
        "name": "retry-reconstruction", "input": {"report": report, "original_config": settings},
        "expected": {"value": {"trials": [{field: trial.get(field) for field in ("name", "raw", "form", "json")} for trial in report["trials"]], "header_settings": settings}},
        "python_mismatch": True, "python_observed": {"value": captured},
        "reason": "retryでrawとヘッダ判定設定が欠落する。設定の保存位置・旧reportへの補完方法は未決。期待値は再構築後の意味上の契約であり新reportのschemaは定めない。",
    }]
    for group, rows in fixtures.items():
        path = args.output / ("retry.json" if group == "retry" else group + "/cases.json")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(rows, ensure_ascii=False, indent=2) + "\n")
        print(f"{group}: {len(rows)} cases")


if __name__ == "__main__":
    main()
