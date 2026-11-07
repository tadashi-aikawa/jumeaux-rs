"""2周目の回帰fixtureをPython実関数から採取する。"""
import json
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
from jumeaux.domain.config.service import create_config  # noqa: E402
from jumeaux.models import Report  # noqa: E402
warnings.filterwarnings("ignore")

cases = []
for name, paths in [
    ("duplicate-keys", ["base.yml", "duplicate-keys.yml"]),
    ("duplicate-with-merge", ["duplicate-merge.yml"]),
    ("nested-merge-explicit-first", ["nested-merge.yml"]),
    ("logger-null", ["base.yml", "logger-null.yml"]),
    ("logger-blank", ["base.yml", "logger-blank.yml"]),
    ("logger-value", ["base.yml", "logger-value.yml"]),
    ("invalid-merge", ["base.yml", "invalid-merge.yml"]),
    ("invalid-merge-list", ["base.yml", "invalid-merge-list.yml"]),
]:
    try:
        result = create_config(TList([str(ROOT / "compat/fixtures/config" / p) for p in paths]), TOption(None))
        expected = {"value": result.to_dict(force_value=True)}
    except Exception as error:
        expected = {"error": type(error).__name__}
    case = {"name": name, "operation": "config", "input": {"paths": paths}, "expected": expected}
    if name == "logger-value":
        case.update(rust_expected={"error": "ConfigError"},
                    difference="docs/compat-spec.md: 非nullのoutput.loggerを設定ファイルで拒否する")
    cases.append(case)

response = {"url": "one", "type": "json", "status_code": 200, "byte": 3, "response_sec": 0.5,
            "content_type": "application/json", "mime_type": "application/json", "encoding": "utf8",
            "file": "body", "prop_file": "props", "response_header": {"future": "keep"}, "future": "drop"}
trial = {"seq": 1, "name": "trial", "tags": [], "headers": {"future": "keep"}, "queries": {"future": ["keep"]},
         "raw": "raw", "form": {"future": 1}, "json": {"future": 1}, "one": response, "other": response,
         "method": "GET", "path": "/", "request_time": "time", "status": "same",
         "diffs_by_cognition": {"json": {"added": [], "changed": [], "removed": [], "future": "drop"}}, "future": "drop"}
endpoint = {"name": "one", "host": "one", "path": {"before": "x", "after": "y", "future": "drop"},
            "query": {"overwrite": {"future": ["keep"]}, "remove": ["x"], "future": "drop"},
            "proxy": "proxy", "default_response_encoding": "utf8", "headers": {"future": "keep"}, "future": "drop"}
report = {"version": "7.0.0", "key": "key", "title": "title", "description": "description", "retry_hash": "retry",
          "notifiers": {"test": {"type": "slack", "version": 2, "channel": "channel", "username": "user",
                                 "icon_emoji": "emoji", "icon_url": "url", "use_blocks": True, "future": "drop"}},
          "summary": {"one": endpoint, "other": endpoint,
                      "status": {"same": 1, "different": 0, "failure": 0, "future": "drop"},
                      "tags": [], "time": {"start": "start", "end": "end", "elapsed_sec": 1, "future": "drop"},
                      "concurrency": {"threads": 1, "processes": 1, "future": "drop"},
                      "output": {"response_dir": "out", "encoding": "utf8", "logger": None, "future": "drop"},
                      "default_encoding": "utf8", "future": "drop"},
          "trials": [trial],
          "addons": {"log2reqs": {"name": "plain", "cls_name": "Executor", "tags": ["tag"],
                                  "config": {"future": "keep"}, "include": "include", "future": "drop"},
                     "final": [{"name": "json", "future": "drop"}], "future": "drop"}, "future": "drop"}
expected = Report.from_json(json.dumps(report), force_cast=True).to_dict(force_value=True)
cases.append({"name": "all-report-fields", "operation": "Report", "input": report, "expected": {"value": expected}})
# 大量のtrial本文をfixtureへ重複保存せず、同じ入力から両実装で展開する。
large = {**report, "trials": [trial] * 8000}
parsed = Report.from_json(json.dumps(large), force_cast=True)
cases.append({"name": "report-8000-trials", "operation": "report-repeat",
              "input": {"report": report, "count": 8000},
              "expected": {"value": {"count": len(parsed.trials),
                                     "first": parsed.trials[0].to_dict(force_value=True),
                                     "last": parsed.trials[-1].to_dict(force_value=True)}}})
print(json.dumps(cases, ensure_ascii=False, indent=2))
