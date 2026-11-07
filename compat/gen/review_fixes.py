"""レビュー修正の回帰fixture。Pythonのモデルと設定関数を実行して採取する。"""
import copy
import importlib
import json
import os
from pathlib import Path
import sys
import subprocess
import warnings

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import jumeaux.addons  # noqa: E402,F401
from owlmixin import TList, TOption  # noqa: E402
from jumeaux.domain.config.service import create_config  # noqa: E402
from jumeaux.domain.config.vo import Notifier  # noqa: E402
from jumeaux.models import Request, Report, StatusCounts  # noqa: E402
warnings.filterwarnings("ignore")

cases = []


def capture(name, operation, source, run, **extra):
    try:
        expected = {"value": run()}
    except Exception as error:
        expected = {"error": type(error).__name__}
    case = {"name": name, "operation": operation, "input": source, "expected": expected, **extra}
    cases.append(case)
    return case


for name, paths in [
    ("explicit-null-defaults", ["null-defaults.yml"]),
    ("yaml-merge-key", ["merge-keys.yml"]),
    ("replaced-invalid-layer", ["base.yml", "replaced-invalid.yml", "replacement.yml"]),
    ("missing-addon-name", ["base.yml", "missing-addon-name.yml"]),
    ("obsolete-logger", ["base.yml", "obsolete-logger.yml"]),
]:
    case = capture(name, "config", {"paths": paths},
        lambda: create_config(TList([str(ROOT / "compat/fixtures/config" / p) for p in paths]), TOption(None)).to_dict(force_value=True))
    if name == "obsolete-logger":
        case.update(rust_expected={"error": "ConfigError"},
            difference="docs/compat-spec.md: 設定ファイルのoutput.loggerは空辞書でも拒否する")

for name, cls, source in [
    ("request-null-defaults", Request, {"path": "/", "qs": None, "headers": None, "method": None, "url_encoding": None}),
    ("status-counts-null", StatusCounts, {"same": None, "different": None, "failure": None}),
    ("notifier-null", Notifier, {"type": "slack", "username": None, "version": None, "use_blocks": None}),
    ("request-json-list", Request, {"path": "/", "json": [1]}),
    ("request-json-scalar", Request, {"path": "/", "json": 1}),
    ("request-form-list", Request, {"path": "/", "form": ["x"]}),
    ("request-form-scalar", Request, {"path": "/", "form": "x"}),
]:
    capture(name, cls.__name__, source, lambda: cls.from_dict(source).to_dict(force_value=True))

trial = {"seq": 1, "name": "", "tags": [], "headers": {}, "queries": {}, "one": {"url": "one", "type": "unknown"},
         "other": {"url": "other", "type": "unknown"}, "method": "GET", "path": "", "request_time": "time", "status": "failure"}
report = {"version": "7.0.0", "key": "key", "title": "", "summary": {
    "one": {"name": "one", "host": "one"}, "other": {"name": "other", "host": "other"},
    "status": {}, "tags": [], "time": {"start": "start", "end": "end", "elapsed_sec": 0},
    "concurrency": {"threads": 1, "processes": 1}, "output": {"response_dir": "out"}},
    "trials": [trial], "addons": {"log2reqs": {"name": "plain"}}}
for name in ["report-unknown", "report-logger"]:
    source = copy.deepcopy(report)
    if name == "report-unknown":
        for obj in [source, source["summary"], source["summary"]["one"], source["summary"]["output"],
                    source["trials"][0], source["trials"][0]["one"], source["addons"]["log2reqs"]]:
            obj["future"] = 1
        # 本文の同名キーはモデルの未知キーではない。
        source["trials"][0]["json"] = {"future": 1}
    else:
        source["summary"]["output"]["logger"] = {}
    # from_jsonf(force_cast=True)と同じrestrict=Falseを通す。
    case = capture(name, "Report", source,
        lambda: Report.from_json(json.dumps(source), force_cast=True).to_dict(force_value=True))
    if name == "report-logger":
        clean = copy.deepcopy(case["expected"])
        del clean["value"]["summary"]["output"]["logger"]
        case.update(rust_expected=clean,
            difference="docs/compat-spec.md: report側のoutput.loggerは警告して捨てる")

# PythonのConfigクラスを直接実行する。本体や通知、外部サービスには接続しない。
valid = {
    "log2reqs/plain": {}, "log2reqs/csv": {}, "log2reqs/json": {}, "log2reqs/yaml": {},
    "reqs2reqs/add": {"reqs": [{"path": "/"}]}, "reqs2reqs/empty_guard": {},
    "reqs2reqs/filter": {"when": "true"}, "reqs2reqs/head": {"size": 1},
    "reqs2reqs/rename": {"conditions": [{"name": "new"}]}, "reqs2reqs/repeat": {"times": 2},
    "reqs2reqs/replace": {"items": [{"queries": {"q": ["x"]}, "headers": {"X": "y"}}]}, "reqs2reqs/shuffle": {},
    "res2res/json": {"transformer": {"module": "unused"}},
    "res2res/json_sort": {"items": [{"when": "true", "targets": [{"path": "/"}]}]},
    "res2res/type": {"conditions": [{"type": "json"}]},
    "res2dict/block": {"header_regexp": "(.*)", "record_regexp": "(.*)"},
    "res2dict/html": {}, "res2dict/json": {}, "res2dict/xml": {},
    "judgement/ignore": {"ignores": [{"title": "test", "conditions": [{"added": [{"path": "/"}]}]}]},
    "judgement/same": {"when_any": ["true"]}, "store_criterion/free": {},
    "dump/encoding": {"encoding": "utf8"}, "dump/html": {}, "dump/json": {}, "dump/xml": {},
    "did_challenge/sleep": {"min": 1.0, "max": 2.0}, "did_challenge/tag": {"conditions": [{"tag": "tag"}]},
    "final/csv": {"column_names": ["name"], "output_path": "out.csv"}, "final/json": {},
    "final/miroir": {"table": "table", "bucket": "bucket"}, "final/notify": {}, "final/summary": {}, "final/viewer": {},
}
for addon, config in valid.items():
    module = importlib.import_module("jumeaux.addons." + addon.replace("/", "."))
    cls = getattr(module, "Config", None)
    null_defaults = {key: None for key in getattr(cls, "__annotations__", {}) if hasattr(cls, key)}
    for variant, value in [("valid", config), ("empty", {}), ("null-fields", {k: None for k in config}),
                           ("null-defaults", {**config, **null_defaults}), ("unknown", {**config, "typo": 1})]:
        # shuffle/viewerはPythonにConfigがない。空config以外はRustの明示拒否として記録する。
        def check():
            if cls is not None:
                return cls.from_dict(value).to_dict(force_value=True)
            return {}
        case = capture(f"addon-{variant}-{addon}", "addon", {"layer": addon.split("/")[0], "name": addon.split("/")[1], "config": value}, check)
        if (addon == "res2res/json" and variant in ["valid", "null-defaults"]) or (cls is None and variant == "unknown"):
            case.update(rust_expected={"error": "ConfigError" if cls is not None else "UnknownPropertiesError"},
                difference="docs/compat-spec.md: Python transformerと解釈しない設定は拒否する")

for codec in ["utf-16", "utf-8-sig"]:
    program = """
import io, logging, sys
logger = logging.getLogger("probe")
logger.addHandler(logging.StreamHandler(sys.stderr))
logger.setLevel(logging.INFO)
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding=sys.argv[1])
sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding=sys.argv[1])
for chunk in ["日本", "語\\n"]:
    sys.stdout.write(chunk)
    sys.stdout.flush()
    sys.stderr.write(chunk)
    sys.stderr.flush()
logger.info("ログ")
"""
    result = subprocess.run([sys.executable, "-B", "-c", program, codec], capture_output=True, check=True,
                            env={**os.environ, "PYTHONIOENCODING": "utf-8"})
    cases.append({"name": f"process-output-{codec}", "operation": "process-output", "input": {"encoding": codec},
                  "expected": {"value": {"stdout_hex": result.stdout.hex(), "stderr_hex": result.stderr.hex()}}})

assert len(valid) == 34
for case in cases:
    if "rust_expected" in case:
        assert case["expected"] != case["rust_expected"], case["name"]
print(json.dumps(cases, ensure_ascii=False, indent=2))
