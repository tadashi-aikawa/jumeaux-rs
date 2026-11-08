"""owlmixin 7.0.1のOptionメソッドをJumeauxの実際の式環境で採取する。"""
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


def add(expression, one="key", other="other", render=False):
    data = {"one": one, "other": other}
    context = {name: TOption(value) for name, value in data.items()}
    context["empty"] = TOption(None)
    try:
        template = expression if expression.startswith("{%") else "{{" + expression + "}}"
        value = (utils.jinja2_format(template, context) if render
                 else utils.when_filter(expression, context))
        expected = {"value": value}
    except Exception as error:
        expected = {"error": type(error).__name__}
    cases.append({"name": expression + "-" + repr(data),
                  "input": {"expression": expression, "data": data, "render": render},
                  "expected": expected})


for value in [None, False, 0, "", [], {}, "key", [1], {"a": 1}]:
    for expression in ["one.any()", "one.any(123)", "one.any(func=123)",
                       "one.map(123).get()", "one.flat_map(123).get()",
                       "one.map(empty.get_or).get()|tojson",
                       "one.map(other.get_or).get()|tojson",
                       "one.flat_map(empty.map).is_none()"]:
        add(expression, one=value)

for expression in [
    "one.map(other.get_or).get()|tojson",
    "one.map(func=other.get_or).get()|tojson",
    "one.map(other.get_or, func=empty.get_or).get()",
    "one.map()", "one.flat_map()", "one.map(1, 2)", "one.flat_map(1, 2)",
    "one.map(unknown=1)", "one.flat_map(unknown=1)",
    "one.map(other.get).get()", "one.map(other.is_none).get()",
    "one.map(other.any).get()", "one.map(other.map).get()",
    "one.map(empty.map).get().is_none()",
    "one.map(empty.flat_map).get().is_none()",
    "one.flat_map(func=empty.map).is_none()",
    "one.flat_map(other.get_or).get()",
    "one.flat_map(other.map(empty.map).get_or).is_none()",
    "one.map(other|attr('get_or')).get()|tojson",
    "one.map({'key': 42}.get).get()",
    "one.map(({'key': 42}).get).get()",
    "one.map({'key': 42}|attr('get')).get()",
    "one.map({'get': 1, 'key': 42}.get).get()",
    "one.map('keychain'.startswith).get()",
    "one.map('keychain'|attr('startswith')).get()",
    "one.map(('keychain').startswith).get()",
    "one.map('keychain'.find).get()",
    "one.map([1, 'key', 'key'].count).get()",
    "one.map('key'.upper).get()",
    "one.map({'key': 1}.keys).get()",
    "one.map(len).get()", "one.map(str).get()", "one.map(bool).get()",
    "one.map(missing).get()", "one.flat_map(missing).get()",
    "empty.map(missing).is_none()", "empty.flat_map(missing).is_none()",
    "one.map(lambda x: x).get()",
    "one.map(other|attr('absent')).get()",
    "one.flat_map({'key': 42}.get).get()",
    "one.flat_map({'key': {'x': 1}}.get).get()",
    "one.flat_map({'key': none}.get).get()",
    "one.map(empty.get_or).map('key'.startswith).get()",
    "one.map({'key': none}.get).map(123).is_none()",
    "one.map(empty.get_or).flat_map(empty.map).is_none()",
    "one.map((other.get_or)).get()|tojson",
    "one.map(({'key': 42}.get if true else empty.get_or)).get()",
    "one.map([{'key': 42}.get][0]).get()",
    "one.map({'fn': {'key': 42}.get}.fn).get()",
    "one.map((none or {'key': 42}.get)).get()",
    "one.map(('key' ~ 'chain').startswith).get()",
    "one.map(('keychain'|replace('chain', '')).startswith).get()",
    "one.map((other.get_or)(none).startswith).get()",
    "one.map(other.map(empty.map).get().get_or).get()|tojson",
    "one.map('日本.語'.startswith).get()",
    "one.flat_map({'key': 42}|attr('get')).get()",
    "one.map(dict).get()",
]:
    add(expression)

for expression in ["one.map(range).get()|list", "one.map(dict).get()",
                   "one.flat_map(dict).get()"]:
    add(expression, one=3 if "range" in expression else [])

# 描画のあとliteral_evalする二段階と、設定のrenameでも使うlengthを固定する。
for expression in ["one.map(other.get_or).get()", "one.flat_map(empty.map).get()"]:
    add(expression, render=True)
for template in [
    "{% set f = {'key': 42}.get %}{{ one.map(f).get() }}",
    "{% macro f(x) %}{{ x }}{% endmacro %}{{ one.map(f).get() }}",
    "{% for f in ['keychain'.startswith, {'key': 42}.get] %}{{ one.map(f).get() }} {% endfor %}",
]:
    add(template, render=True)

print(json.dumps(cases, ensure_ascii=False, indent=2))
