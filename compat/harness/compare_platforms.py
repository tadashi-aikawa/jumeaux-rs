"""同じ同梱serverで生成したmacOS/Linuxのreportと保存bytesを比較する。"""
import argparse
import copy
from datetime import datetime
from email.utils import parsedate_to_datetime
from pathlib import Path

from compare import equal, load, normalize_report, tree, validate_report
from run import RULES


def normalize(report):
    report = copy.deepcopy(report)
    start = datetime.fromisoformat(report["summary"]["time"]["start"]).timestamp()
    end = datetime.fromisoformat(report["summary"]["time"]["end"]).timestamp()
    for trial in report["trials"]:
        for side in ("one", "other"):
            headers = trial[side].get("response_header", {})
            if "date" in headers:
                stamp = parsedate_to_datetime(headers["date"]).timestamp()
                assert start - 1 <= stamp <= end, "HTTP Date outside execution interval"
                headers["date"] = "<server-date>"
    return normalize_report(report, RULES)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("expected", type=Path)
    parser.add_argument("actual", type=Path)
    args = parser.parse_args()
    cases = sorted(p.name for p in args.actual.iterdir() if p.is_dir())
    assert cases, "no cases"
    for case in cases:
        left, right = args.expected / case, args.actual / case
        for label in ("run", "retry"):
            expected, actual = (load(p / f"{label}-report.json") for p in (left, right))
            for report, work in ((expected, left), (actual, right)):
                source_key = load(work / "run-report.json")["key"] if label == "retry" else None
                validate_report(report, work / "responses" / report["key"], source_key)
                assert report["summary"]["status"]["failure"] == 0
                assert (work / "responses/latest").resolve().name == load(work / "retry-report.json")["key"]
            equal(normalize(expected), normalize(actual), f"{case}.{label}")
            one, other = (work / "responses" / report["key"] for work, report in ((left, expected), (right, actual)))
            equal(tree(one), tree(other), f"{case}.{label}.tree")
            for rel, kind in tree(one):
                if kind == "file" and rel != "report.json":
                    equal((one / rel).read_bytes(), (other / rel).read_bytes(), f"{case}.{label}.{rel}")
        print(f"PASS {case}: 2 reports and all response/prop bytes matched")
    print(f"{len(cases)}/{len(cases)} templates matched; {2 * len(cases)} reports")


if __name__ == "__main__":
    main()
