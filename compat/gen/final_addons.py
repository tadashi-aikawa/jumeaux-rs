"""finalのPython観測値。通信せず、保存先は一時ディレクトリだけを使う。"""
import contextlib
import importlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import warnings

ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", ROOT.parent / "jumeaux"))
sys.path.insert(0, str(PYTHON_ROOT))
import jumeaux.addons
from jumeaux.models import Report, FinalAddOnPayload, FinalAddOnReference
warnings.filterwarnings("ignore")


def main():
    report = Report.from_jsonf(str(ROOT / "compat/python-baseline/simple/report.json"))
    report.key = "fixture-key"
    report.title = "日本語,タイトル"
    report.description.value = "説明\n2行目"
    report.trials[0].name = '日本語,"引用"\n改行\rCR'
    report.trials[0].json.value = {"null": None, "nested": [1, None, {"x": None}]}
    cases = []
    with tempfile.TemporaryDirectory() as tmp:
        previous = Path.cwd()
        os.chdir(tmp)
        report.summary.output.response_dir = "responses"
        destination = Path("responses") / report.key
        destination.mkdir(parents=True)
        for encoding in ["utf8", "utf-16", "utf-8-sig", "iso2022_jp"]:
            report.summary.output.encoding = encoding
            payload = FinalAddOnPayload.from_dict({"report": report, "output_summary": report.summary.output})
            reference = FinalAddOnReference.from_dict({"notifiers": None})
            for addon, configs in [
                ("summary", [{}, {"sysout": True}]),
                ("json", [{}, {"indent": 0}, {"indent": -1}, {"indent": 2}, {"sysout": True, "indent": 4}]),
                ("csv", [{"column_names": ["seq", "name", "method", "status", "headers", "queries", "one.response_sec", "missing"], "with_header": True},
                         {"column_names": ["missing"]}, {"column_names": []}]),
            ]:
                for config in configs:
                    config = dict(config)
                    if addon == "csv": config["output_path"] = str(destination / "out.csv")
                    cls = importlib.import_module("jumeaux.addons.final." + addon).Executor
                    output = io.StringIO()
                    with contextlib.redirect_stdout(output):
                        cls(config).exec(payload, reference)
                    path = destination / {"summary": "summary.txt", "json": "report.json", "csv": "out.csv"}[addon]
                    expected = {"stdout": output.getvalue()}
                    if not config.get("sysout"): expected["file_hex"] = path.read_bytes().hex()
                    # 一時ディレクトリ名を含めず、再採取できる固定パスを使う。
                    cases.append({"name": f"{addon}/{encoding}/{len(cases)}", "addon": addon, "config": config,
                                  "report": report.to_dict(ignore_none=False), "expected": expected})
        cases.append({"name": "notify/context", "report": report.to_dict(ignore_none=False),
                      "expected": report.to_dict(ignore_none=False)})
        os.chdir(previous)
    print(json.dumps(cases, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
