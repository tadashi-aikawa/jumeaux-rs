"""除外は設定の明示パスにのみ適用する。本文や任意のJSONキーは除外しない。"""

from collections import Counter
from datetime import datetime
import json
import math
from pathlib import Path
import re


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def match(pattern, path):
    parts = pattern.split(".")
    return len(parts) == len(path) and all(a == "*" or a == b for a, b in zip(parts, path))


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def normalize_report(value, rules, path=()):
    field = next((kind for pattern, kind in rules["report_fields"].items() if match(pattern, path)), None)
    if field:
        if field in ("string", "key_reference", "retry_reference"):
            require(isinstance(value, str) and bool(value) or (field == "retry_reference" and value is None), f"{path}: invalid string/reference")
        elif field == "datetime":
            require(isinstance(value, str), f"{path}: not a timestamp")
            datetime.fromisoformat(value)
        elif "nonnegative" in field:
            if value is None and field.startswith("nullable_"):
                return None
            require(type(value) in (int, float) and math.isfinite(value) and value >= 0, f"{path}: invalid duration")
        return f"<{field}>" if value is not None else None
    if isinstance(value, dict):
        if any(match(pattern, path) for pattern in rules["lowercase_header_paths"]):
            result = {key.lower(): val for key, val in value.items()}
            require(len(result) == len(value), f"{path}: duplicate header names")
            return result
        return {key: normalize_report(val, rules, path + (key,)) for key, val in value.items()}
    if isinstance(value, list):
        return [normalize_report(val, rules, path + (str(i),)) for i, val in enumerate(value)]
    return value


def equal(expected, actual, label):
    # JSONの1と1.0、trueと1の差も失わない。
    if type(expected) is not type(actual):
        raise AssertionError(f"{label}: type {type(expected).__name__} != {type(actual).__name__}")
    if isinstance(expected, dict):
        require(expected.keys() == actual.keys(), f"{label}: keys {set(expected) ^ set(actual)}")
        for key in expected:
            equal(expected[key], actual[key], f"{label}.{key}")
    elif isinstance(expected, list):
        require(len(expected) == len(actual), f"{label}: length {len(expected)} != {len(actual)}")
        for i, (one, other) in enumerate(zip(expected, actual)):
            equal(one, other, f"{label}[{i}]")
    else:
        require(expected == actual, f"{label}: {str(expected)[:180]!r} != {str(actual)[:180]!r}")


def validate_report(report, folder, source_key=None):
    require(report["key"] == folder.name, f"{folder}: key/directory mismatch")
    require(report.get("retry_hash") == source_key, f"{folder}: retry_hash/source mismatch")
    clock = report["summary"]["time"]
    require(datetime.fromisoformat(clock["end"]) >= datetime.fromisoformat(clock["start"]), "negative time span")
    for trial in report["trials"]:
        for side in ("one", "other"):
            for field in ("file", "prop_file"):
                if trial[side].get(field) is not None:
                    file = (folder / trial[side][field]).resolve()
                    require(file.is_relative_to(folder.resolve()) and file.is_file(), f"{folder}: broken {side}.{field}")


def reports(workspace):
    top = workspace / "report.json"
    if not top.exists():
        require(not (workspace / "responses").exists(), f"{workspace}: unexpected responses without report")
        return []
    report = load(top)
    source = load(workspace / "source-report.json") if (workspace / "source-report.json").exists() else None
    result = ([source] if source else []) + [report]
    response_dir = workspace / "responses"
    equal(sorted(x.name for x in response_dir.iterdir()), sorted([r["key"] for r in result] + ["latest"]), "response directories")
    require((response_dir / "latest").is_symlink(), "latest must be a symlink")
    require((response_dir / "latest").resolve() == (response_dir / report["key"]).resolve(), "latest/key mismatch")
    for i, item in enumerate(result):
        folder = response_dir / item["key"]
        equal(item, load(folder / "report.json"), "report copy")
        validate_report(item, folder, result[i-1]["key"] if i else None)
    return result


def text(data, report_list, rules):
    value = data.decode("utf-8")
    for i, report in enumerate(report_list):
        for field in rules["text_report_references"]:
            value = value.replace(report[field], f"<{field}:{i}>")
    for rule in rules["text_rules"]:
        value = re.sub(rule["pattern"], rule["replacement"], value)
    return value


def tree(root, snapshot=False):
    return sorted((p.relative_to(root).as_posix(), "dir" if p.is_dir() else "file") for p in root.rglob("*") if not (snapshot and p.name == ".gitkeep"))


def records(path):
    result = Counter()
    occurrences = Counter()
    for line in path.read_text().splitlines():
        row = json.loads(line)
        occurrence = row.pop("occurrence")
        identity = json.dumps(row, sort_keys=True)
        occurrences[identity] += 1
        require(occurrence == occurrences[identity], f"{path}: occurrence count mismatch")
        # ヘッダの順序を保った原記録を比較。本文はbase64のままbytes完全一致。
        result[identity] += 1
    return dict(result)


def compare_case(baseline, actual, rules):
    baseline, actual = Path(baseline), Path(actual)
    expect_reports, actual_reports = reports(baseline), reports(actual)
    equal(len(expect_reports), len(actual_reports), "report count")
    for one, other in zip(expect_reports, actual_reports):
        equal(normalize_report(one, rules), normalize_report(other, rules), "report")
        left, right = baseline / "responses" / one["key"], actual / "responses" / other["key"]
        equal(tree(left, snapshot=True), tree(right, snapshot=baseline == actual), "saved file tree")
        for rel, kind in tree(left, snapshot=True):
            if kind == "file" and rel != "report.json":
                equal((left / rel).read_bytes(), (right / rel).read_bytes(), rel)
    for path in ("api.jsonl", "notifications.jsonl"):
        equal(records(baseline / path), records(actual / path), path)
    if actual.name.startswith("notify_"):
        require(sum(records(actual / "notifications.jsonl").values()) == 1, "notification missing/duplicated")
    for path in ("artifacts.json", "init-config.yml", "config.yml", "requests"):
        equal((baseline / path).read_bytes(), (actual / path).read_bytes(), path)
    equal(tree(baseline / "api"), tree(actual / "api"), "init api tree")
    for rel, kind in tree(baseline / "api"):
        if kind == "file":
            equal((baseline / "api" / rel).read_bytes(), (actual / "api" / rel).read_bytes(), "init api " + rel)
    for label in ("init", "run", "retry"):
        for stream in ("stdout", "stderr", "exit-code"):
            path = f"{label}.{stream}"
            equal((baseline / path).exists(), (actual / path).exists(), path + " existence")
            if (baseline / path).exists():
                equal(text((baseline / path).read_bytes(), expect_reports, rules), text((actual / path).read_bytes(), actual_reports, rules), path)
