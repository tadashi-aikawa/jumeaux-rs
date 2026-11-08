"""式メソッド・未定義フィルタのレビュー回帰をPython版から採取する。"""
import json
import os
from pathlib import Path
import sys
import warnings

SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from jumeaux import utils  # noqa: E402
from owlmixin import TOption  # noqa: E402
warnings.filterwarnings("ignore")

cases = []


def add(expression, *, render=False, option=False, one="key", rust=None, summary=None):
    data = {"one": one} if option else {"qs": {}}
    context = {"one": TOption(one)} if option else data
    operation = ("option_" if option else "") + ("render" if render else "when")
    template = "{{" + expression + "}}"
    field = "template" if render else "expression"
    source = template if render else expression
    try:
        value = utils.jinja2_format(template, context) if render else utils.when_filter(expression, context)
        expected = {"value": value}
    except Exception as error:
        expected = {"error": type(error).__name__}
    case = {"name": "option-review-" + expression + "-" + operation + "-" + repr(one),
            "input": {"operation": operation, field: source, "data": data}, "expected": expected}
    if rust is not None:
        assert rust != expected, expression
        case.update(rust_expected=rust, difference="compat-spec.md: " + summary)
    cases.append(case)


for key in ["[1]", "[]", "{'x': 1}", "{}", "([1],)"]:
    add("{'a': 1}.get(" + key + ")")
    add("{}.get(" + key + ", 99)")
for value in [[1], [], {"a": 1}, {}]:
    add("one.map({'a': 1}.get).get()", option=True, one=value)
    add("one.map({'a': 1}|attr('get')).get()", option=True, one=value)
    add("one.flat_map({'a': 1}.get).get()", option=True, one=value)
for key in ["'a'", "'missing'", "1", "true", "none", "(1, 2)"]:
    add("{'a': 1}.get(" + key + ", 99)")
add("one.map({'a': 1}.get).is_none()", option=True, one=None)
add("one.map(debug).get()", option=True, one=1)
add("debug is defined")
add("debug()")

for name in ["first", "last"]:
    add("missing|" + name, render=True)
    add("missing|" + name)
    add("missing|" + name + " is defined")
    add("missing|" + name + " == missing|" + name)
    add("missing|" + name + "|default('fallback')", render=True)
    add("missing.child|" + name, render=True)
    add("qs.missing|" + name, render=True)
    for value in ["[]", "[1, 2]", "''", "'日本語'", "none", "7"]:
        add(value + "|" + name, render=True)

for expression, rust in [
    ("missing|items|list", {"error": "TypeError"}),
    ("missing|random", {"error": "TypeError"}),
    ("missing|format", {"error": "TypeError"}),
    ("missing|indent", {"value": ""}),
    ("missing|wordwrap", {"value": ""}),
    ("missing|dictsort", {"error": "TypeError"}),
    ("missing|filesizeformat", {"error": "TypeError"}),
]:
    add(expression, render=True, rust=rust, summary="未定義値を受け取るフィルタの差異")

for expression, value in [
    ("one.map('abc'.rsplit).get()", "b"),
    ("one.map('abc'.index).get()", "b"),
    ("one.map('abc'.partition).get()", "b"),
    ("one.map('abc'.zfill).get()", 5),
    ("one.map('abc'.center).get()", 7),
    ("one.map('a'.__eq__).get()", "a"),
    ("one.map({'a': 1}.pop).get()", "a"),
    ("one.map([1, 2].index).get()", 2),
]:
    add(expression, render=True, option=True, one=value, rust={"error": "UndefinedError"},
        summary="メソッド参照の許可リスト外は未対応")
add("one.get()[-1:].count('a')", render=True, option=True, one=["a"],
    rust={"error": "TypeError"}, summary="リストのスライスはiteratorになる")
add("one.map(one.get()[-1:].count).get()", render=True, option=True, one=["a"],
    rust={"error": "UndefinedError"}, summary="リストのスライスはiteratorになる")
add("(one.get()[-1:]|list).count('a')", option=True, one=["a"])

print(json.dumps(cases, ensure_ascii=False, indent=2))
