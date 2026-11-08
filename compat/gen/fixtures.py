"""Python版の実関数を呼ぶfixture生成。移行元には書き込まない。"""

import argparse
from datetime import datetime, timedelta, timezone
import importlib.util
from itertools import product
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import urllib.parse
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
from jumeaux.addons.dump.html import pretty as pretty_html  # noqa: E402
from jumeaux.addons.dump.xml import pretty as pretty_xml  # noqa: E402
from jumeaux.addons.log2reqs.plain import Executor as Plain, guess_url_encoding  # noqa: E402
from jumeaux.addons.res2dict.html import html_to_dict  # noqa: E402
from jumeaux.addons.res2dict.xml import Executor as XML  # noqa: E402
from jumeaux.domain.config.vo import MergedArgs  # noqa: E402
from jumeaux.models import DumpAddOnPayload, Log2ReqsAddOnPayload, Res2DictAddOnPayload, Request, Response  # noqa: E402


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


def serializable_html_to_dict(body):
    value = html_to_dict(body)
    # plaintextではHTMLToDictParserの__parent__が残り循環する。
    # one-props保存時のエラーまで観測し、循環オブジェクトをfixtureへ流さない。
    json.dumps(value)
    return value


def response(body=b"abc", encoding="utf-8", kind="plain", status=200):
    return Response.from_dict({
        "body": body, "encoding": encoding, "type": kind,
        "headers": {"content-type": "text/" + kind}, "url": "http://localhost/test",
        "status_code": status, "elapsed": timedelta(0), "elapsed_sec": 0.0,
    })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=ROOT / "compat/fixtures")
    parser.add_argument("--diff-matrix", action="store_true", help="配列の組み合わせ検証用fixtureを追加する")
    args = parser.parse_args()
    fixtures = {}

    def add(group, name, data, function, expected=None, reason=None, rust_expected=None, difference=None):
        observed = outcome(function)
        row = {"name": name, "input": data, "expected": observed if reason is None else expected}
        if reason:
            assert observed != expected, name + ": Python不一致が再現していない"
            row.update(python_mismatch=True, reason=reason, python_observed=observed)
        if rust_expected is not None:
            assert observed != rust_expected, name + ": Rust差異の指定がPythonと同じ"
            assert difference, name + ": 差異の説明が必要"
            row.update(rust_expected=rust_expected, difference=difference)
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
        ("insert-middle", [1, 2, 3], [1, 0, 2, 3]),
        ("delete-duplicates", [1, 1, 2, 1], [1, 2]),
        ("reorder", [1, 2, 3], [3, 1, 2]),
        ("replace-and-insert", [1, 2, 3], [1, 4, 5, 3]),
        ("shifted-replacement", [1, 2, 3, 4, 5], [0, 1, 2, 8, 9, 4, 5]),
        ("dict-array-delete", [{"id": 1}, {"id": 2}], [{"id": 2}]),
        ("nested-array-insert", [[1], [2]], [[0], [1], [2]]),
        ("mixed-array", [1, {"a": 1}, 2], [0, 1, {"a": 1}, 2]),
        ("array-int-float", [1, 2], [1.0, 2]),
        ("array-bool-int", [True, 2], [1, 2]),
        ("array-null", [None, 1], [0, None, 1]),
        ("dict-sparse-overlap", {"a": 1, "b": 2}, {"a": 1, "c": 3, "d": 4}),
        ("dict-threshold", {"a": 1, "b": 2}, {"a": 1, "c": 3}),
        ("empty-dict-one-key", {}, {"a": 1}),
        ("empty-dict-two-keys", {}, {"a": 1, "b": 2}),
        ("remove-dict-two-keys", {"a": 1, "b": 2}, {}),
        ("private-key", {"__meta": 1, "a": 1}, {"__meta": 2, "a": 1}),
        ("nested-path", {"items": [{"id": 1}]}, {"items": [{"id": 2}]}),
        ("sorted-paths", list(range(12)), [42] * 12),
        ("root-type", 1, "1"),
        ("root-float", 1, 1.0),
        ("null-type", None, {}),
        ("large-integers", [9007199254740992], [9007199254740993]),
        ("large-int-float", [9007199254740993], [9007199254740992.0]),
        ("float-negative-zero", -0.0, 0.0),
        ("beyond-u64-array", [2**64], [2**64 + 1]),
        ("beyond-u64-dict", {"a": 2**70}, {"a": 2**70 + 1}),
        ("below-i64-array", [-2**63 - 1], [-2**63 - 2]),
        ("bigint-float-equal-array", [2**200], [float(2**200)]),
        ("bigint-float-neighbor-array", [2**200 + 1], [float(2**200)]),
        ("bigint-float-type-dict", {"a": 2**200}, {"a": float(2**200)}),
        ("bigint-negative-float-array", [-2**200], [-float(2**200)]),
        ("bigint-beyond-f64", [10**400], [10**400 + 1]),
    ]
    for name, one, other in pairs:
        add("diff", name, {"operation": "diff", "one": one, "other": other}, lambda: diff(one, other))
    if args.diff_matrix:
        for label, values, max_length in [
            ("scalar", [0, 1, 2], 3),
            ("mixed", [None, True, 0, 1, 1.0, "a", {"a": 1}, [1]], 2),
        ]:
            arrays = [list(items) for length in range(max_length + 1) for items in product(values, repeat=length)]
            for index, (one, other) in enumerate(product(arrays, repeat=2)):
                add("diff", f"matrix-{label}-{index}", {"operation": "diff", "one": one, "other": other}, lambda: diff(one, other))
    for key in ("root", "myroot", "a[0]", "a.b", "a'b", "a<b>"):
        data = {key: 42}
        path = "root<" + repr(key) + ">"
        add("diff", "lookup-" + key, {"operation": "get_by_diff_key", "dict": data, "path": path, "segments": [key]}, lambda: utils.get_by_diff_key(data, path), {"value": 42}, "差分パスの文字列置換でキー名が壊れる。元のキーを1要素として参照する。")
    add("diff", "lookup-normal", {"operation": "get_by_diff_key", "dict": {"items": [{"id": 7}]}, "path": "root<'items'><0><'id'>"}, lambda: utils.get_by_diff_key({"items": [{"id": 7}]}, "root<'items'><0><'id'>"))
    for name, data, path in [
        ("missing", {"a": 1}, "root<'missing'>"),
        ("out-of-bounds", {"a": []}, "root<'a'><0>"),
        ("null", {"a": None}, "root<'a'>"),
        ("numeric-key", {"0": 42}, "root<'0'>"),
    ]:
        add("diff", "lookup-" + name, {"operation": "get_by_diff_key", "dict": data, "path": path}, lambda: utils.get_by_diff_key(data, path))
    # キー内部のブラケットは構文ではない。生成側でも壊さず値取得と往復させる。
    for key in ("root", "myroot", "a[0]", "a.b", "a'b", "a<b>"):
        one, other = {key: 1}, {key: 2}
        expected = {"value": {"changed": ["root<" + repr(key) + ">"], "added": [], "removed": []}}
        add("diff", "path-" + key, {"operation": "diff", "one": one, "other": other}, lambda: diff(one, other), expected,
            "パス生成でもキー内部のブラケットを保持する。値取得と往復可能にする。" if "[" in key else None)
    for name, key in [("both-quotes", "a'\"b"), ("backslash", "a\\b"), ("newline", "a\nb")]:
        one, other = {key: 1}, {key: 2}
        quote = '"' if "'" in key else "'"
        escaped = key.replace("\\", "\\\\").replace(quote, "\\" + quote).replace("\n", "\\n")
        path = "root<" + quote + escaped + quote + ">"
        expected = {"value": {"changed": [path], "added": [], "removed": []}}
        add("diff", "path-" + name, {"operation": "diff", "one": one, "other": other}, lambda: diff(one, other), expected,
            "キーの引用符・バックスラッシュ・制御文字をエスケープし、パス生成と値取得を往復可能にする。")

    html_cases = [
        ("completion", "<p>hello"), ("empty-tags", "<p><br><img src='x'><input disabled></p>"),
        ("mixed-text", "<p>a<b>b</b>c</p>"), ("repeated", "<ul><li>a</li><li>b</li></ul>"),
        ("script", "<script>if (1 < 2) x = '&';</script>"),
        ("table-completion", "<table><tr><td>a</td></tr></table>"),
        ("unit-html", imported("tests/addons/res2dict/test_html.py").NORMAL_BODY),
        ("attributes", '<DIV Z="  z  " class="a   b" title="&quot;x&quot; &amp; y" empty="" blank="  ">x&nbsp;&lt;&gt;</DIV>'),
        ("pre-textarea", '<pre>  a\n b <b>x</b></pre><textarea>  a\n b &amp; c</textarea>'),
        ("comments", '<!--outside--><p>a<!--inside-->b</p>'),
        ("whitespace", '<p> \n a   b\t c \r\n </p><div></div><p> </p>'),
        ("explicit-empty", '<html><head></head><body></body></html>'),
        ("explicit-tbody", '<table><tbody><tr><td>x</td></tr></tbody></table>'),
        ("implicit-paragraph", '<p>first<p>second'),
        ("plain-text", 'hello &amp; world'),
        ("empty", ''),
        ("comment-only", '<!--alone-->'),
        ("table-foster-parenting", '<table>before<tr><td>x</td></tr>after</table>'),
        ("misnested-formatting", '<p><b>bold<i>both</b>italic</i></p>'),
        ("tables-with-and-without-tbody", '<table><tbody><tr><td>a</td></tr></tbody></table><table><tr><td>b</td></tr></table>'),
        ("head-and-body", '<title>Title</title><p>Body</p>'),
        ("pre-leading-newline", '<pre>\n  a\n</pre>'),
        ("unicode-whitespace", '<p>\u00a0</p><p>a\u0085</p>'),
        ("script-style-entities", '<script>var x="&amp;";</script><style>a>b { content: "&amp;"; }</style>'),
        ("quoted-attribute", '<p title="a &quot;quote&quot; and &apos;apostrophe&apos;">&notin; &#x1f600;</p>'),
        ("noscript", '<noscript><p>fallback</p></noscript>'),
        ("body-noscript", '<body><noscript><b>fallback</b></noscript></body>'),
        ("head-noscript", '<head><noscript><p>fallback</p></noscript></head>'),
        ("paragraph-block", '<p>a<div>b</div>c</p>'),
        ("paragraph-list", '<p>a<ul><li>x</li></ul>b</p>'),
        ("stray-p-end", '<div>a</p>b</div>'),
        ("stray-br-end", '<div>a</br>b</div>'),
        ("paragraph-table", '<!DOCTYPE html><p>a<table><tr><td>x</td></tr></table></p>'),
        ("formatting-across-paragraphs", '<p><b>x</p><p>y</p>'),
        ("after-body", '<html><body><p>a</p></body><div>b</div>tail</html>'),
        ("after-html", '<html><body>a</body></html><p>discard</p>tail'),
        ("self-closing-div", '<div/>after<p>x</p>'),
        ("self-closing-script", '<script src="x" /><p>after</p>'),
        ("body-template", '<body><template><p>x</p></template></body>'),
        ("comment-padding", '<p>a<!--  comment  -->b</p>'),
        ("empty-comment", '<p>a<!---->b</p>'),
        ("svg-case", '<svg viewBox="0 0 10 10"><linearGradient/></svg>'),
        ("mathml-case", '<math definitionURL="x"><mi>x</mi></math>'),
        ("nested-headings", '<h1>a<h2>b</h2>c</h1>'),
        ("nested-form", '<form id="a"><form id="b">x</form>y</form>'),
        ("table-form", '<table><form><input name="x"></form></table>'),
        ("orphan-table-elements", '<tr><td>x</td></tr>'),
        ("image-alias", '<image src="x">after'),
        ("isindex", '<isindex prompt="x">after'),
        ("nested-nobr", '<nobr>a<nobr>b</nobr>c</nobr>'),
        ("second-body", '<body id="first">a<body class="second">b</body>'),
        ("plaintext", '<plaintext>a<b>b</b>'),
        ("head-template", '<template><p>x</p></template>'),
        ("implicit-head-noscript", '<title>t</title><noscript><link rel="stylesheet" href="x"></noscript><p>x</p>'),
        ("head-ended-by-text", '<title>t</title>body<noscript><b>x</b></noscript>'),
        ("head-ended-by-element", '<title>t</title><p>x</p><noscript><b>y</b></noscript>'),
        ("quirks-paragraph-table", '<p>a<table><tr><td>x</td></tr></table></p>'),
        ("transitional-paragraph-table", '<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN"><p>a<table><tr><td>x</td></tr></table></p>'),
        ("body-end-in-element", '<body><div>a</body>b</div>'),
        ("self-closing-head-title", '<head/><title>t</title>'),
    ]
    for name, body in html_cases:
        data = {"format": "html", "body": body}
        if name == "plaintext":
            data["serialize"] = True
        add("markup", name, data, lambda: serializable_html_to_dict(body))
    xml_cases = [
        ("mixed-text", '<r a="1">before<c/>after</r>'),
        ("namespace", '<r xmlns="urn:default" xmlns:p="urn:test"><p:c p:a="1">x</p:c></r>'),
        ("cdata", "<r><![CDATA[a<b>&c]]></r>"),
        ("entity", "<r>&amp;&lt;&#65;&#x3042;</r>"),
        ("internal-entity", '<!DOCTYPE r [<!ENTITY x "hello">]><r>&x;</r>'),
        ("empty-repeat", '<r><c/><c a="1"/></r>'),
        ("unit-xml", imported("tests/addons/res2dict/test_xml.py").NORMAL_BODY),
        ("whitespace", '<r> \n <c>  a   b\t c  </c> \n </r>'),
        ("comments-pi", '<?test x?><r>a<!--comment-->b<?test y?>c</r>'),
        ("cdata-mixed", '<r>a<![CDATA[<b>]]>c<d/>e</r>'),
        ("attribute-entities", '<r a="&quot;&apos;&lt;&gt;&amp;&#10;&#9;" b="a\n b\t c"/>'),
        ("namespace-rebinding", '<r xmlns:p="urn:a"><p:c/><c xmlns:p="urn:b" p:a="1"/><p:d/></r>'),
        ("xml-space", '<r xml:space="preserve">  a  </r>'),
        ("empty", ''),
        ("unclosed", '<r><c></r>'),
        ("duplicate-attribute", '<r a="1" a="2"/>'),
        ("undefined-entity", '<r>&missing;</r>'),
        ("multiple-roots", '<a/><b/>'),
        ("external-entity", '<!DOCTYPE r [<!ENTITY x SYSTEM "file:///jumeaux-fixture-must-not-read">]><r>&x;</r>'),
        ("nested-entity", '<!DOCTYPE r [<!ENTITY a "A"><!ENTITY b "&a;&amp;B">]><r>&b;</r>'),
        ("attribute-internal-entity", '<!DOCTYPE r [<!ENTITY x "hello">]><r a="&x;">&x;</r>'),
        ("markup-entity", '<!DOCTYPE r [<!ENTITY x "<c>hello</c>">]><r>&x;</r>'),
        ("dtd-default-attribute", '<!DOCTYPE r [<!ATTLIST r a CDATA "default">]><r/>'),
        ("unbound-prefix", '<p:r/>'),
        ("invalid-name", '<1root/>'),
        ("invalid-character", '<r>\u0001</r>'),
        ("invalid-comment", '<r><!--a--b--></r>'),
        ("unclosed-root", '<r>'),
        ("carriage-return", '<r a="&#13;">a&#13;b\r\nc\rd</r>'),
        ("bom-declaration", '\ufeff<?xml version="1.0"?><r/>'),
        ("space-before-declaration", ' <?xml version="1.0"?><r/>'),
        ("external-dtd", '<!DOCTYPE html SYSTEM "fixture.dtd"><html>&nbsp;</html>'),
        ("public-dtd", '<!DOCTYPE html PUBLIC "fixture" "fixture.dtd"><html>&nbsp;</html>'),
        ("parameter-declaration", '<!DOCTYPE r [<!ENTITY % p "unused">]><r/>'),
        ("dtd-comment", '<!DOCTYPE r [<!-- <!ENTITY x "fake"> -->]><r>&x;</r>'),
        ("dtd-pi", '<!DOCTYPE r [<?test <!ENTITY x "fake"> ?>]><r>&x;</r>'),
        ("dtd-quoted", '<!DOCTYPE r [<!ATTLIST r a CDATA #IMPLIED><!ENTITY y \'<!ENTITY x "fake">\'>]><r>&x;</r>'),
        ("dtd-fixed-attribute", '<!DOCTYPE r [<!ATTLIST r a CDATA #FIXED "fixed">]><r/>'),
        ("non-ascii-element", '<日本語>本文</日本語>'),
        ("entity-amp-number", '<!DOCTYPE r [<!ENTITY x "&#38;amp;">]><r a="&x;">&x;</r>'),
        ("external-dtd-attribute", '<!DOCTYPE r SYSTEM "fixture.dtd"><r a="before&missing;after"/>'),
        ("invalid-dtd-default", '<!DOCTYPE r [<!ATTLIST r a CDATA "<x>">]><r/>'),
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

    for kind, cases, pretty in [("html", html_cases, pretty_html), ("xml", xml_cases, pretty_xml)]:
        for name, body in cases:
            add("markup", "dump-" + name, {"operation": "pretty", "format": kind, "body": body}, lambda: pretty(body))

    # 大きな展開結果は長さで比較する。入力もパラメーター化してfixtureの肥大化を避ける。
    for name, attrs, text, padding in [
        ("below-threshold", 700, 0, 0),
        ("attributes-over-threshold", 900, 0, 0),
        ("mixed-over-threshold", 450, 450, 0),
        ("text-over-threshold", 0, 900, 0),
        ("large-low-amplification", 900, 0, 150000),
    ]:
        body = '<!DOCTYPE r [<!ENTITY x "' + 'x' * 10000 + '">]><!--' + 'p' * padding + '--><r>' + '<c a="&x;"/>' * attrs + '&x;' * text + '</r>'
        for operation in ["dict", "pretty"]:
            def expansion_summary():
                if operation == "pretty":
                    value = pretty_xml(body)
                    return {"invalid": value.startswith("Include invalid token"), "length": len(value)}
                value = XML({}).exec(Res2DictAddOnPayload.from_dict({"response": response(body.encode(), kind="xml")})).result.get()["r"] or {}
                return {"attribute_chars": sum(len(c["@a"]) for c in value.get("c", [])), "text_chars": len(value.get("#text", ""))}
            add("markup", "expansion-" + name, {"format": "xml", "operation": operation, "summary": "expansion", "attributes": attrs, "text": text, "padding": padding}, expansion_summary)

    for kind, operation, depth in [("xml", "dict", 1000), ("xml", "pretty", 1000), ("xml", "dict", 1001), ("xml", "pretty", 1001), ("html", "pretty", 1000)]:
        tag = "r" if kind == "xml" else "div"
        body = f"<{tag}>" * depth + "x" + f"</{tag}>" * depth
        def depth_summary():
            if operation == "pretty":
                value = pretty_xml(body) if kind == "xml" else pretty_html(body)
                return {"invalid": value.startswith("Include invalid token"), "length": len(value)}
            value = XML({}).exec(Res2DictAddOnPayload.from_dict({"response": response(body.encode(), kind="xml")})).result.get()
            measured = 0
            while isinstance(value, dict) and tag in value:
                measured += 1
                value = value[tag]
            return {"depth": measured, "leaf": value}
        add("markup", f"depth-{depth}", {"format": kind, "operation": operation, "summary": "depth", "depth": depth}, depth_summary)

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
    for expression in ("none", "[True, None, 'a']", "{'x': False}", "'hello'", "\"'hello'\"", "'1 + 2'", "'0x10'", "'-2.5'", "missing is defined", "missing|default('False')", "missing == 1", "not missing", "'abc'|reg('a.*')", "'ABC'.lower() == 'abc'", "' x '.strip() == 'x'", "'abc'.startswith('a')", "'abc'.replace('b', 'B') == 'aBc'", "equals_without_host('http://a/x?q=1', 'https://b/x?q=1')", "calc_distance_km(35.664131,139.759302,35.694253,139.784099)"):
        add("template", "extra-" + expression, {"operation": "when", "expression": expression, "data": {}}, lambda: utils.when_filter(expression, {}))
    for fmt in ("{{ [True, None, 'a'] }}", "{{ {'x': False} }}", "{{ x }}\n", "{% if missing %}yes{% else %}no{% endif %}"):
        add("template", "extra-" + fmt, {"operation": "render", "template": fmt, "data": {"x": "日本語"}}, lambda: utils.jinja2_format(fmt, {"x": "日本語"}))
    for rendered in ("(1, 2)", "{1, 2}", "set()", "b'abc'", "1+2j", "...", "{1: 'one'}", "123456789012345678901234567890", "1e999"):
        expression = repr(rendered)
        def literal_type():
            value = utils.when_filter(expression, {})
            return {"type": type(value).__name__, "truthy": bool(value)}
        add("template", "literal-type-" + rendered, {"operation": "when_type", "expression": expression}, literal_type,
            rust_expected={"error": "ValueError"} if rendered == "1e999" else None,
            difference="compat-spec.md: 非有限数は入口でValueError" if rendered == "1e999" else None)
    for expression in ("response.byte", "response.url", "response.type", "response.elapsed_sec", "response.elapsed.total_seconds()", "response.elapsed.seconds", "response.headers['Content-Type']", "response.content_type.get()", "response.mime_type.get()", "response.charset.get()", "response.charset", "response.encoding.get()"):
        res = response("日本語".encode())
        # 文字列プロパティもliteral_evalを通せるようtojsonで引用する。
        expression += "|tojson"
        add("template", "property-" + expression, {"operation": "response_when", "expression": expression, "text": "日本語", "status_code": 200}, lambda: utils.when_filter(expression, {"response": res}))
    request_data = {"path": "/search", "qs": {"q": ["東京"]}, "name": "検索", "headers": {"X-Test": "yes"}}
    for expression in ("request.path == '/search'", "request.qs.q[0] == '東京'", "request.name.get() == '検索'", "request.raw.is_none()", "request.url_encoding == 'utf-8'"):
        add("template", "request-" + expression, {"operation": "request_when", "expression": expression, "request": request_data}, lambda: utils.when_filter(expression, {"request": Request.from_dict(request_data)}))
    fixed = datetime(2000, 1, 2, 3, 4, 5, tzinfo=timezone(timedelta(hours=9)))
    for expression in (
        "'bad'|int", "'bad'|float", "' 42 '|int", "' 42.5 '|float",
        "'1_000'|int", "'1_000.5'|float", "missing|int", "missing|float",
        "'bad'|int(7)", "'bad'|int(default=8)", "'bad'|float(default=2.5)",
        "'ff'|int(base=16)", "'11'|int(0, 2)", "'0x10'|int(base=0)",
        "'0x_FF'|int(base=16)", "'4.9'|int", "none|int", "none|float",
        "'1__0'|int", "'1__0'|float", "'x'|int(default=none)",
        "'bad'|float(none)", "'bad'|int(none)", "'1_e2'|float", "'z_z'|int(base=36)",
        "'10'|int(base=-1)", "'ff'|int(base=none)", "'inf'|int", "'nan'|int",
        "'inf'|int(base=36)",
        "'bad'|int(3, default=4)", "'bad'|float(3, default=4)",
    ):
        add("template", "review-" + expression, {"operation": "when", "expression": expression, "data": {}}, lambda: utils.when_filter(expression, {}))
    for fmt in (
        "{{ 'bad'|float }}", "{{ 1e20 }}", "{{ 1e-5 }}", "{{ 1e-4 }}", "{{ 1e16 }}", "{{ -0.0 }}",
        "{{ {'日本': '😀'}|tojson }}", "{{ 'a b/日'|urlencode }}",
        "{{ 'abcdef'|truncate(length=4, killwords=True, leeway=0) }}",
        "{{ 1024|filesizeformat }}", "{{ '<b>Hello</b> world'|striptags }}",
        "{{ 'one two'|wordcount }}", "{{ 'one two three'|wordwrap(width=7) }}",
        "{{ [42]|random }}", "{% set c=cycler('a', 'b') %}{{ c.next() }}{{ c.next() }}",
        "{% set j=joiner('/') %}{{ j() }}a{{ j() }}b", "{{ lipsum(n=0, html=False) }}",
        "{% if false %}{{ x|unknown_filter }}{% endif %}",
        "{% if false %}{{ x is unknown_test }}{% endif %}",
    ):
        load_error = "unknown_filter" in fmt or "unknown_test" in fmt
        add("template", "review-" + fmt, {"operation": "render", "template": fmt, "data": {}}, lambda: utils.jinja2_format(fmt, {}), rust_expected={"error": "TemplateAssertionError"} if load_error else None, difference="compat-spec.md: 未実行分岐でも未知filter/testをロード時に拒否" if load_error else None)
    for expression, rust in (
        ("missing > 1", {"value": False}), ("missing|length", {"error": "TypeError"}),
        ("s > 1", {"value": True}), ("nil < 1", {"value": True}),
        ("1 / 0", {"error": "TypeError"}),
    ):
        data = {"s": "hello", "nil": None}
        add("template", "difference-" + expression, {"operation": "when", "expression": expression, "data": data}, lambda: utils.when_filter(expression, data), rust_expected=rust, difference="compat-spec.md: 未定義値・異種比較・例外種別")
    for fmt, rust in (
        ("{{ (1, 2) }}", {"value": "[1, 2]"}),
        ("{{ range(3) }}", {"value": "[0, 1, 2]"}),
        ("x +", {"value": "x +"}),
        ("{{ x }}", {"value": "['\u200b']"}),
    ):
        data = {"x": ["\u200b"]}
        # x + はwhenの空白制御として下で採取する。
        if fmt == "x +":
            add("template", "difference-whitespace-plus", {"operation": "when", "expression": fmt, "data": {"x": True}}, lambda: utils.when_filter(fmt, {"x": True}), rust_expected={"value": True}, difference="compat-spec.md: x + の空白制御")
        else:
            add("template", "difference-repr-" + fmt, {"operation": "render", "template": fmt, "data": data}, lambda: utils.jinja2_format(fmt, data), rust_expected=rust, difference="compat-spec.md: tuple/range・非表示文字のrepr")
    for dsl in ("$DATETIME(%S)(1_000)", "$DATETIME(%S)(１２)", "$DATETIME(%Q)(0)", "$DATETIME(%3f)(0)"):
        with patch.object(utils, "now", return_value=fixed):
            add("template", "difference-" + dsl, {"operation": "datetime", "value": dsl, "now": fixed.isoformat()}, lambda: utils.parse_datetime_dsl(dsl), rust_expected={"value": "000"} if "%3f" in dsl else {"error": "ValueError"}, difference="compat-spec.md: DATETIMEのint/strftime差異")
    for dsl in ("$DATETIME(%Y-%m-%d)(-86400)", "$DATETIME(%Y-%m-%d %H:%M:%S)(86400)", "$DATETIME(%Y)(bad)", "literal", "$DATETIME(%Y)(0)", "$DATETIME(%Y)(+1)", "x$DATETIME(%Y)(0)", "$DATETIME(%Y)(0)\n", "$DATETIME()(0)", "$DATETIME(%Y-%m-%dT%H:%M:%S.%f%z %Z)(0)", "$DATETIME(%%f)(0)"):
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
        ("numeric-group", r"(a)(b)", r"\g<2>\g<1>", "ab"),
        ("lookbehind", r"(?<=a)b", "x", "ab cb"),
        ("negative-lookahead", r"a(?!b)", "x", "ab ac"),
        ("end-newline", r"x$", "y", "x\n"),
        ("multiline", r"(?m)^x$", "y", "x\nx\n"),
        ("escaped-dollar", r"\$", "$1", "$"),
        ("replacement-escapes", r"(a)", r"\1\n\\$1", "a"),
        ("unmatched-group", r"(a)?b", r"\1X", "b"),
        ("bad-group-no-match", r"(a)", r"\2", "z"),
        ("bad-escape", r"a", r"\q", "a"),
        ("octal-replacement", r"a", r"\101\0", "a"),
        ("zero-before-nonoctal", r"a", r"\08\09", "a"),
        ("empty-match", r"x*", "-", "abxd"),
        ("empty-then-nonempty", r".*?", "-", "ab"),
        ("unsupported-variable-lookbehind", r"(?<=a+)b", "x", "aab"),
    ]:
        add("regex", name, {"operation": "sub", "pattern": pattern, "replacement": replacement, "target": target}, lambda: re.sub(pattern, replacement, target))
    for name, pattern, target in [("search-middle", "b", "abc"), ("strict-end", r"x\Z", "x\n"), ("class-dollar", "[$]", "$"), ("unicode-digit", r"\d+", "１２３")]:
        add("regex", name, {"operation": "search", "pattern": pattern, "target": target}, lambda: bool(re.search(pattern, target)))

    unit_models = imported("tests/test_models.py")
    for name, pattern, target, rejected in [
        ("posix", r"[[:alpha:]]", "a", True),
        ("intersection", r"[a-z&&[^b]]", "a", True),
        ("unicode-property", r"\p{L}", "a", False),
        ("horizontal-space", r"\h", " ", False),
        ("escape-e", r"\e", "a", False),
        ("hex-braces", r"\x{41}", "A", False),
        ("rust-name", r"(?<n>a)", "a", False),
        ("rust-reference", r"(?P<n>a)\k<n>", "aa", False),
        ("numeric-name", r"(?P<1>a)", "a", False),
        ("duplicate-name", r"(?P<n>a)(?P<n>b)", "ab", False),
        ("conditional", r"(a)?(?(1)b|c)", "ab", True),
        ("named-character", r"\N{LATIN CAPITAL LETTER A}", "A", True),
        ("literal-bracket", r"[[]", "[", False),
        ("octal-zero", r"\0", "\0", False),
        ("octal-three", r"\101", "A", False),
        ("ascii-word", r"(?a)\w+", "日本語", False),
        ("ascii-digit", r"(?a)\d+", "123", False),
        ("ascii-scoped", r"(?a:\w+)\w", "a日", False),
    ]:
        add("regex", "review-" + name, {"operation": "search", "pattern": pattern, "target": target}, lambda: bool(re.search(pattern, target)), rust_expected={"error": "PatternError"} if rejected else None, difference="compat-spec.md: 読み込み時に拒否する正規表現構文" if rejected else None)
    for code in range(0x1c, 0x20):
        add("regex", f"difference-space-{code:02x}", {"operation": "search", "pattern": r"\s", "target": chr(code)}, lambda: bool(re.search(r"\s", chr(code))), rust_expected={"value": False}, difference="compat-spec.md: Pythonの追加空白文字")

    for encoding, texts in [
        ("cp932", ["〜‖¢£¬−", "¥", "‾", "ⅰ髙", "\uf8f0\uf8f1\uf8f2\uf8f3"]),
        ("euc_jp", ["〜‖¢£¬−", "～∥￠￡￢－", "①髙", "丂", "˘", "¥‾"]),
        ("iso8859_15", ["€ŠšŽžŒœŸ"]), ("mac-roman", ["Äé"]),
        ("cp437", ["Çα■"]), ("utf-32", ["日本😀"]), ("utf-32-be", ["日本😀"]),
        ("euc_kr", ["한국", "갂쀍힣", "ㅤ"]), ("cp949", ["한국", "갂쀍힣"]),
        ("windows-1252", ["\x81"]), ("windows-1254", ["\x8e"]),
        ("hz-gb-2312", ["x"]), ("iso-2022-kr", ["x"]),
    ]:
        for index, text in enumerate(texts):
            for errors in ("strict", "replace"):
                rejected = encoding in ("hz-gb-2312", "iso-2022-kr")
                add("encoding", f"review-encode-{encoding}-{index}-{errors}", {"operation": "encode", "text": text, "encoding": encoding, "errors": errors}, lambda: text.encode(encoding, errors=errors).hex(), rust_expected={"error": "LookupError"} if rejected else None, difference="compat-spec.md: WHATWG replacement codecを拒否" if rejected else None)
    for encoding, bodies in [
        ("cp932", ["a0fdfeff", "eeefeee0", "816081618191819281ca"]),
        ("euc_jp", ["a1c1a1c2a1dda1f1a1f2a2cc", "ada1", "f9a1", "fcfe", "8fb0a1"]),
        ("windows-1252", ["818d8f909d"]), ("windows-1254", ["818d8e8f909d9e"]),
        ("euc_kr", ["c7d1b1b9", "a4d4a4a1a4bfa4a2", "a4d4", "a4d4a4a1a4bf"]), ("cp949", ["c7d1b1b9", "814197cdc652"]),
        ("iso8859_15", ["a4a6a8b4b8bcbdbe"]),
        ("mac-roman", ["808e"]), ("cp437", ["80e0fe"]),
        ("utf-32", ["fffe000000f60100"]), ("utf-32-be", ["0001f600"]),
        ("hz-gb-2312", ["78"]), ("iso-2022-kr", ["78"]),
    ]:
        for body_hex in bodies:
            body = bytes.fromhex(body_hex)
            for errors in ("strict", "replace"):
                rejected = encoding in ("hz-gb-2312", "iso-2022-kr")
                add("encoding", f"review-decode-{encoding}-{body_hex}-{errors}", {"operation": "decode", "body_hex": body_hex, "encoding": encoding, "errors": errors}, lambda: body.decode(encoding, errors=errors), rust_expected={"error": "LookupError"} if rejected else None, difference="compat-spec.md: WHATWG replacement codecを拒否" if rejected else None)
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
    for name, body, encoding in [("utf8-keeps-bom", b"\xef\xbb\xbfabc", "utf-8"), ("latin1-not-windows1252", b"\x80\x91", "latin1"), ("ascii-replacement", b"\x80", "ascii"), ("sjis-wave-dash", b"\x81\x60", "sjis"), ("cp932-wave-dash", b"\x81\x60", "cp932"), ("utf16le-keeps-bom", b"\xff\xfea\x00", "utf-16-le"), ("utf16-no-bom", b"a\x00", "utf-16"), ("utf16be", b"\x00a", "utf-16-be"), ("utf16-invalid", b"\xff\xfe\x00", "utf-16"), ("binary-text", b"\xff", None)]:
        add("encoding", name, {"operation": "response_text", "body_hex": body.hex(), "encoding": encoding}, lambda: response(body, encoding).text)
    for name, body, encoding in [("sjis-extension", b"\x87\x40", "shift_jis"), ("cp932-extension", b"\x87\x40", "cp932"), ("sjis-single-invalid", b"\x80\xa0\xfd", "sjis"), ("utf16-surrogate-odd", b"\x00\xd8\x00", "utf-16-le")]:
        add("encoding", name, {"operation": "response_text", "body_hex": body.hex(), "encoding": encoding}, lambda: response(body, encoding).text)
    for name, text, encoding in [("sjis-encode-wave", "〜", "shift_jis"), ("sjis-encode-fullwidth", "～", "shift_jis"), ("sjis-encode-extension", "①", "shift_jis")]:
        add("encoding", name, {"operation": "encode", "text": text, "encoding": encoding, "errors": "replace"}, lambda: text.encode(encoding, errors="replace").hex())
    for name, text, encoding in [("save-question-mark", "a😀日", "ascii"), ("save-sjis-replacement", "a😀日", "cp932"), ("save-utf16", "日本😀", "utf-16"), ("save-utf16be", "日本😀", "utf-16-be"), ("save-utf8-sig", "日本", "utf-8-sig"), ("save-latin1", "\x80", "latin1"), ("save-iso2022", "日😀本", "iso2022_jp")]:
        add("encoding", name, {"operation": "encode", "text": text, "encoding": encoding, "errors": "replace"}, lambda: text.encode(encoding, errors="replace").hex())
    for name, body in [("detect-ascii", b"abc"), ("detect-utf8", "日本語の文字コードを調べます".encode()), ("detect-sjis", "日本語の文字コードを調べます".encode("cp932")), ("detect-euc", "日本語の文字コードを調べます".encode("euc-jp")), ("detect-iso2022", "日本語の文字コードを調べます".encode("iso2022_jp"))]:
        res = RequestsResponse()
        res._content = body
        add("encoding", name, {"operation": "detect", "body_hex": body.hex()}, lambda: res.apparent_encoding)
    for name, query, encoding in [("url-encode-sjis", "東京 a", "sjis"), ("url-encode-failure", "😀", "cp932")]:
        add("encoding", name, {"operation": "quote_plus", "text": query, "encoding": encoding}, lambda: urllib.parse.quote_plus(query, encoding=encoding))
    for name, body, source, dest in [
        ("dump-success", "日本語".encode(), "utf-8", "cp932"),
        ("dump-decode-fail", b"\xff", "utf-8", "cp932"),
        ("dump-encode-fail", "😀".encode(), "utf-8", "cp932"),
        ("dump-utf16", "日本語".encode(), "utf-8", "utf-16"),
    ]:
        payload = DumpAddOnPayload.from_dict({"response": response(body, source), "body": body, "encoding": source})
        add("encoding", name, {"operation": "dump_encoding", "body_hex": body.hex(), "source": source, "destination": dest}, lambda: DumpEncoding({"encoding": dest}).exec(payload).body.hex())
    for name, query, encodings in [("url-sjis", "q=%93%8c%8b%9e", ["utf-8", "sjis"]), ("url-fallback", "q=%ff", ["utf-8"]), ("url-order", "q=abc", ["euc-jp", "utf-8"]), ("url-blank-skipped", "%ff=", ["utf-8"]), ("url-unknown-codec", "q=%ff", ["no-such-codec"]), ("url-unescaped-unicode", "q=東京", ["ascii"]), ("url-mixed-sjis", "q=%83A", ["utf-8", "sjis"])]:
        add("encoding", name, {"operation": "guess_url_encoding", "query": query, "candidates": encodings}, lambda: guess_url_encoding(query, TList(encodings)).get())
    body = b"<p>\xff</p>"
    payload = DumpAddOnPayload.from_dict({"response": response(body, "ascii", "html"), "body": body, "encoding": "ascii"})
    add("encoding", "dump-replacement-question-mark", {"operation": "dump_html", "body_hex": body.hex(), "encoding": "ascii", "decoded_text": body.decode("ascii", errors="replace"), "formatted_text": pretty_html(body.decode("ascii", errors="replace"))}, lambda: DumpHTML({}).exec(payload).body.hex())
    with tempfile.TemporaryDirectory(prefix="jumeaux-fixtures-") as directory:
        file = Path(directory) / "requests"
        for name, body, encoding in [("input-decode-fail", b"\xff", "utf-8"), ("input-sjis", "/x?q=東京".encode("cp932"), "cp932"), ("input-utf8-bom", b"\xef\xbb\xbf/x", "utf-8"), ("input-utf8-sig", b"\xef\xbb\xbf/x", "utf-8-sig"), ("input-utf16", "/x".encode("utf-16"), "utf-16"), ("input-newlines", b"/a\r\n/b\r/c\n", "utf-8")]:
            file.write_bytes(body)
            add("encoding", name, {"operation": "plain_input", "body_hex": body.hex(), "encoding": encoding}, lambda: Plain({"encoding": encoding}).exec(Log2ReqsAddOnPayload.from_dict({"file": str(file)})).to_dicts())

    # 2周目レビュー。arbitrary_precision有効下の数値入力とJinja2の引数契約。
    for name, data, fmt in [
        ("numbers", {"i": -2**63, "u": 2**64 - 1, "big": 2**80, "f": 1.25}, "{{ i }} {{ u }} {{ big }} {{ f }}"),
        ("nested-numbers", {"obj": {"items": [42, 1.25]}}, "{{ obj['items'][0] + 1 }} {{ obj['items'][1] * 2 }}"),
        ("beyond-i128", {"big": 1e50}, "{{ big }}"),
    ]:
        add("template", "round2-" + name, {"operation": "render", "template": fmt, "data": data}, lambda: utils.jinja2_format(fmt, data))
    request_data = {"path": "/", "json": {"n": 2**70, "nested": [1.5]}, "form": {"n": 42}}
    for expression in ("request.json.get().n == 1180591620717411303424", "request.json.get().nested[0] == 1.5", "request.form.get().n == 42"):
        add("template", "round2-" + expression, {"operation": "request_when", "expression": expression, "request": request_data}, lambda: utils.when_filter(expression, {"request": Request.from_dict(request_data)}))
    for expression in (
        "'１２'|int", "'٣'|int", "'１.５'|float", "'0_7'|int(base=2)",
        "none|int(default=7)", "none|float(default=7.5)", "'１_０００'|float",
    ):
        add("template", "round2-" + expression, {"operation": "when", "expression": expression, "data": {}}, lambda: utils.when_filter(expression, {}))
    for fmt in (
        "{{ 1e20|string }} {{ 1e-5|string }} {{ 'NaN'|float|string }}",
        "{{ [1e20, 1e-5, 'NaN'|float]|join('/') }}",
        "{{ {'z': [1e20, {'z': 1e-5, 'a': '日本'}], 'a': 1}|tojson }}",
        "{{ {'z': 1, 'a': {'b': 2, 'a': 1}}|tojson(2) }}",
        "{{ {10: 'ten', 2: 'two', True: 'one', -1: 'negative'}|tojson }}",
        "{{ {None: 'none'}|tojson }}", "{{ {1: 'int', 'x': 'str'}|tojson }}",
        "{{ 'NaN'|float|tojson }}",
        "{{ 'abcdef'|truncate(4, True, '..', 0) }}",
        "{{ 'one two three'|wordwrap(7, True, '|', False) }}",
        "{{ 'aaaa'|replace('a', 'x', 2) }}",
        "{{ 'aaaa'|replace(old='a', new='x', count=2) }}",
        "{{ 'aaaa'|replace('a') }}", "{{ [1e20, 1e-5]|join(None) }}",
        "{{ 1024|filesizeformat(True) }}",
        "{{ ['b', 'A']|sort(True, True) }}",
        "{{ ['a', 'A', 'b']|unique(True)|list }}",
        "{{ {'z': 1, 'a': 2}|dictsort(False, 'value', True)|tojson }}",
        "{{ {'q': 'a b', 'x y': '/日本'}|urlencode }}",
        "{{ [('q', 'a b'), ('q', '/日本')]|urlencode }}",
    ):
        add("template", "round2-" + fmt, {"operation": "render", "template": fmt, "data": {}}, lambda: utils.jinja2_format(fmt, {}))
    for name in ("bool", "chain", "lines", "split", "zip"):
        fmt = "{{ x|" + name + " }}"
        add("template", "round2-filter-" + name, {"operation": "render", "template": fmt, "data": {}}, lambda: utils.jinja2_format(fmt, {}))
    for name in ("startingwith", "endingwith", "safe"):
        fmt = "{{ x is " + name + " }}"
        add("template", "round2-test-" + name, {"operation": "render", "template": fmt, "data": {}}, lambda: utils.jinja2_format(fmt, {}))
    for fmt, rust in (
        ("{{ 'n=' ~ 1e20 }}", {"value": "n=100000000000000000000.0"}),
        ("{{ 1|round(2, 'floor') }}", {"value": "1"}),
        ("{{ true|abs }}", {"error": "TypeError"}),
        ("{{ [1, 2] * 1.0 }}", {"value": "[1, 2]"}),
        ("{{ [{'key': 'b'}, {'key': 'A'}, {'key': 'a'}]|groupby('key') }}", {"value": "[['a', [{'key': 'A'}, {'key': 'a'}]], ['b', [{'key': 'b'}]]]"}),
    ):
        add("template", "round2-difference-" + fmt, {"operation": "render", "template": fmt, "data": {}}, lambda: utils.jinja2_format(fmt, {}), rust_expected=rust, difference="compat-spec.md: 連結・round・abs・リスト乗算・groupbyの差異")

    for pattern in (r"a{2,1}", r"{1}", r"a{1}{2}", r"\G", r"(?U)a", r"(?R)", r"(*FAIL)", r"(?'n'a)", r"\K", r"(?P<n>a)\g<n>", r"[a-z--b]", r"[a-z~~b]"):
        rejected = "--" in pattern or "~~" in pattern
        add("regex", "round2-" + pattern, {"operation": "search", "pattern": pattern, "target": "aa"}, lambda: bool(re.search(pattern, "aa")), rust_expected={"error": "PatternError"} if rejected else None, difference="compat-spec.md: 集合演算に見える文字クラスを拒否" if rejected else None)
    for pattern, target in [(r"(?i)a", "A"), (r"(a)\1", "aa"), (r"(a)\2", "aa"), (r"(?i:a)", "A")]:
        add("regex", "round2-exact-" + pattern, {"operation": "exact_match", "pattern": pattern, "target": target}, lambda: utils.exact_match(target, pattern))
    for pattern, target in [(r"(?i)(?m)^a$", "A"), (r"a(?i)b", "ab"), (r"(?a:\w+)\w", "a日"), (r"a{2,3}?", "aaa")]:
        add("regex", "round2-search-" + pattern, {"operation": "search", "pattern": pattern, "target": target}, lambda: bool(re.search(pattern, target)))

    for encoding, texts in [
        ("cp932", ["￢", "\ue000\ue03e\ue03f\ue0bb\ue0bc\ue757", "ⅰ髙"]),
        ("iso2022_jp", ["〜‖¢£¬−", "～∥￠￡￢－", "№", "℡", "Ⅰ", "ｱ"]),
        ("ansi_x3.4-1968", ["é"]),
        ("latin5", ["ĞİŞğış\x80"]), ("l5", ["ĞİŞğış\x80"]),
        ("iso-8859-9", ["ĞİŞğış\x80"]),
        ("cp819", ["\x80é"]), ("iso-ir-100", ["\x80é"]),
        ("csisolatin1", ["\x80é"]), ("ibm819", ["\x80é"]),
        ("koi8-r", ["abc"]), ("gbk", ["abc"]),
    ]:
        for index, text in enumerate(texts):
            for errors in ("strict", "replace"):
                unsupported = encoding in ("koi8-r", "gbk")
                add("encoding", f"round2-encode-{encoding}-{index}-{errors}", {"operation": "encode", "text": text, "encoding": encoding, "errors": errors}, lambda: text.encode(encoding, errors=errors).hex(), rust_expected={"error": "LookupError"} if unsupported else None, difference="compat-spec.md: 専用対応のないcodecを拒否" if unsupported else None)
    for encoding, bodies in [
        ("cp932", ["81caf040f07ef080f0fcf140f9fc"]),
        ("iso2022_jp", ["1b24422141214221712172224c1b2842", "1b24422d211b2842", "1b244279211b2842", "1b24427c7e1b2842", "1b2849311b2842"]),
        ("ansi_x3.4-1968", ["80"]),
        ("latin5", ["80d0dddef0fdfe"]), ("l5", ["80d0dddef0fdfe"]),
        ("cp819", ["8091e9"]), ("iso-ir-100", ["8091e9"]),
        ("csisolatin1", ["8091e9"]), ("ibm819", ["8091e9"]),
        ("euc_jp", ["8fa2b7", "8fa2", "8f"]), ("cp949", ["8140", "8180", "a180", "c680"]),
    ]:
        for body_hex in bodies:
            body = bytes.fromhex(body_hex)
            for errors in ("strict", "replace"):
                rust = None
                if encoding == "euc_jp" and body_hex == "8fa2b7":
                    rust = {"value": "〜"}
                elif errors == "replace":
                    if encoding == "euc_jp" and body_hex == "8fa2":
                        rust = {"value": "��"}
                    elif encoding == "iso2022_jp" and body_hex in ("1b24422d211b2842", "1b244279211b2842", "1b24427c7e1b2842"):
                        rust = {"value": "��"}
                    elif encoding == "iso2022_jp" and body_hex == "1b2849311b2842":
                        rust = {"value": "���1"}
                    elif encoding == "cp949" and body_hex in ("8180", "a180", "c680"):
                        rust = {"value": "�"}
                add("encoding", f"round2-decode-{encoding}-{body_hex}-{errors}", {"operation": "decode", "body_hex": body_hex, "encoding": encoding, "errors": errors}, lambda: body.decode(encoding, errors=errors), rust_expected=rust, difference="compat-spec.md: JIS X 0212の波線・不完全列の置換数・不正bytesの読み直し位置" if rust else None)
    # Pythonでもcp932へ解決される別名を、その名前のまま実測する。
    for alias in ("Windows-31J", "cswindows31j"):
        text = "￢ⅰ髙\ue000\ue757"
        body = bytes.fromhex("a0fd81caeeefeee0f040f9fc")
        add("encoding", "round2-alias-encode-" + alias, {"operation": "encode", "text": text, "encoding": alias, "errors": "strict", "python_codec": "cp932"}, lambda: text.encode(alias).hex())
        add("encoding", "round2-alias-decode-" + alias, {"operation": "decode", "body_hex": body.hex(), "encoding": alias, "errors": "strict", "python_codec": "cp932"}, lambda: body.decode(alias))

    for alias in ("ks_c_5601-1987", "ksx1001"):
        add("encoding", "round3-alias-decode-" + alias, {"operation": "decode", "body_hex": "82a0", "encoding": alias, "errors": "strict"}, lambda: bytes.fromhex("82a0").decode(alias))
        add("encoding", "round3-alias-encode-" + alias, {"operation": "encode", "text": "궇", "encoding": alias, "errors": "strict"}, lambda: "궇".encode(alias).hex())
    for code in (0x0e, 0x0f, 0x1b):
        text, body = chr(code), bytes([code])
        add("encoding", f"round3-iso2022-encode-control-{code:02x}", {"operation": "encode", "text": text, "encoding": "iso2022_jp", "errors": "strict"}, lambda: text.encode("iso2022_jp").hex(), rust_expected={"error": "UnicodeEncodeError"}, difference="compat-spec.md: ISO-2022-JPの生のSO・SI・ESC")
        add("encoding", f"round3-iso2022-decode-control-{code:02x}", {"operation": "decode", "body_hex": body.hex(), "encoding": "iso2022_jp", "errors": "strict"}, lambda: body.decode("iso2022_jp"), rust_expected={"error": "UnicodeDecodeError"} if code != 0x1b else None, difference="compat-spec.md: ISO-2022-JPの生のSO・SI・ESC" if code != 0x1b else None)
    for pattern, target, rust in [
        (r"^*", "a", {"value": True}),
        (r"(?P<n>a(?P=n))", "aa", {"value": False}),
        (r"(?au)a", "a", {"value": True}),
        (r"(?:)*", "a", {"error": "PatternError"}),
        (r"(?#c)(?i)a", "A", {"error": "PatternError"}),
    ]:
        add("regex", "round3-difference-" + pattern, {"operation": "search", "pattern": pattern, "target": target}, lambda: bool(re.search(pattern, target)), rust_expected=rust, difference="compat-spec.md: 繰り返し・未閉鎖group参照・フラグの構文差異")
    for number in (2**127 - 1, 2**127, 2**127 + 1):
        fmt, data = "{{ number }}", {"number": number}
        rust = {"value": repr(float(number))} if number >= 2**127 else None
        add("template", "round3-bigint-" + str(number), {"operation": "render", "template": fmt, "data": data}, lambda: utils.jinja2_format(fmt, data), rust_expected=rust, difference="compat-spec.md: i128を超える整数のf64変換" if rust else None)

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
