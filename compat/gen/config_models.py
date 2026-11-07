"""Python実関数から設定マージ・モデルのJSON入出力fixtureを標準出力へ採取する。"""
import json
import io
import os
from pathlib import Path
import sys
import warnings

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from owlmixin import TList, TOption  # noqa: E402
from jumeaux.domain.config.service import create_config, merge_args2config  # noqa: E402
from jumeaux.domain.config.vo import MergedArgs  # noqa: E402
from jumeaux.models import Request, Trial, Report  # noqa: E402
warnings.filterwarnings("ignore")

cases = []
for name, paths, args in [
    ("defaults", ["base.yml"], {}),
    ("replace-and-include", ["base.yml", "override.yml"], {}),
    ("empty-and-skipped-layers", ["base.yml", "empty.yml"], {}),
    ("empty-cli", ["base.yml"], {"title": "", "description": "", "tag": [], "files": [], "threads": 0, "processes": 0, "max_retries": 0}),
    ("override-cli", ["base.yml"], {"title": "CLI", "description": "CLI description", "tag": ["cli"], "files": ["new.txt"], "threads": 8, "processes": 3, "max_retries": 1}),
    ("negative-cli", ["base.yml"], {"threads": -1, "processes": -2, "max_retries": -3}),
]:
    config = create_config(TList([str(ROOT / "compat/fixtures/config" / p) for p in paths]), TOption(TList(["skip"])))
    result = merge_args2config(MergedArgs.from_dict(args), config)
    cases.append({"name": name, "operation": "config", "input": {"paths": paths, "skip_tags": ["skip"], "args": args}, "expected": {"value": result.to_dict(force_value=True)}})

trial = {"seq": 1, "name": "", "tags": [], "headers": {}, "queries": {}, "one": {"url": "one", "type": "unknown"}, "other": {"url": "other", "type": "unknown"}, "method": "GET", "path": "", "request_time": "time", "status": "failure"}
for name, cls, source in [
    ("request-defaults", Request, {"path": ""}),
    ("request-null", Request, {"path": "/", "name": None, "raw": None, "form": None, "json": None}),
    ("request-empty", Request, {"path": "/", "name": "", "raw": "", "form": {}, "json": {}}),
    ("request-nested-null", Request, {"path": "/", "json": {"none": None, "list": [None, {}, False, 0, "", {"gone": None}]}}),
    ("trial-missing", Trial, trial),
    ("trial-null", Trial, {**trial, "diffs_by_cognition": None, "raw": None}),
    ("trial-empty", Trial, {**trial, "diffs_by_cognition": {}, "raw": "", "form": {}, "json": {}}),
    ("trial-diffs", Trial, {**trial, "diffs_by_cognition": {"unknown": {"added": [], "changed": ["root<'a'>"], "removed": []}}}),
]:
    cases.append({"name": name, "operation": cls.__name__, "input": source, "expected": {"value": cls.from_dict(source).to_dict(force_value=True)}})

number_input = '{"path":"/","json":{"decimal":1.2300,"exponent":1e2,"big":18446744073709551617}}'
cases.append({"name": "number-spelling", "operation": "number-spelling", "input": number_input, "expected": {"value": Request.from_json(number_input).to_json()}})
report = {"version": "7.0.0", "key": "key", "title": "", "summary": {
    "one": {"name": "one", "host": "one"}, "other": {"name": "other", "host": "other"},
    "status": {}, "tags": [], "time": {"start": "start", "end": "end", "elapsed_sec": 0},
    "concurrency": {"threads": 1, "processes": 1}, "output": {"response_dir": "out"}}, "trials": [trial]}
for name, source in [
    ("report-missing", report),
    ("report-null", {**report, "description": None, "notifiers": None, "addons": None, "retry_hash": None}),
    ("report-empty", {**report, "description": "", "notifiers": {}, "retry_hash": ""}),
    ("report-notifier", {**report, "notifiers": {"test": {"type": "slack", "channel": ""}}}),
]:
    cases.append({"name": name, "operation": "Report", "input": source, "expected": {"value": Report.from_dict(source).to_dict(force_value=True)}})
number_report = json.dumps({**report, "trials": [{**trial, "json": "NUMBER"}]}).replace('"NUMBER"', '{"decimal":1.2300,"exponent":1e2,"big":18446744073709551617}')
cases.append({"name": "report-number-spelling", "operation": "report-number-spelling", "input": number_report, "expected": {"value": Report.from_json(number_report).to_json()}})

for codec in ["utf8", "utf-8-sig", "utf-16", "utf-32", "cp932", "iso2022_jp"]:
    for chunks in [["日本", "語\n"], ["", "日", "", "本", "ASCII", "語", "\n", "¥", "‾"]]:
        raw = io.BytesIO()
        stream = io.TextIOWrapper(raw, encoding=codec)
        result = []
        try:
            for chunk in chunks:
                stream.write(chunk)
                stream.flush()
                result.append(raw.getvalue().hex())
            expected = {"value": result}
        except UnicodeError as error:
            expected = {"error": type(error).__name__}
        cases.append({"name": f"output-{codec}-{len(chunks)}", "operation": "output", "input": {"encoding": codec, "chunks": chunks}, "expected": expected})
print(json.dumps(cases, ensure_ascii=False, indent=2))
