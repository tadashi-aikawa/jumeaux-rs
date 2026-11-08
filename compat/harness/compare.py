"""除外は設定の明示パスにのみ適用する。本文や任意のJSONキーは除外しない。"""

from collections import Counter
import copy
from datetime import datetime
import json
import math
from pathlib import Path
import re


def load(path):
    return json.loads(path.read_bytes())


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
    require(("retry_hash" not in report) if source_key is None else report.get("retry_hash") == source_key, f"{folder}: retry_hash/source mismatch")
    clock = report["summary"]["time"]
    require(datetime.fromisoformat(clock["end"]) >= datetime.fromisoformat(clock["start"]), "negative time span")
    equal(report, json.loads((folder / "report.json").read_bytes().decode(report["summary"]["output"]["encoding"])), "report output.encoding")
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


def text(data, report_list, rules, stream=None):
    value = data.decode("utf-8")
    for i, report in enumerate(report_list):
        for field in rules["text_report_references"]:
            value = value.replace(report[field], f"<{field}:{i}>")
    for rule in rules["text_rules"]:
        value = re.sub(rule["pattern"], rule["replacement"], value)
    lines = value.splitlines(keepends=True)
    for excluded in (rules["excluded_transport_lines"], rules["excluded_notification_transport_lines"]):
        if stream in excluded["streams"]:
            lines = [line for line in lines if not any(re.fullmatch(pattern, line.rstrip("\n")) for pattern in excluded["patterns"])]
    for block in rules["unordered_text_blocks"]:
        if stream not in block["streams"]:
            continue
        result = []
        i = 0
        while i < len(lines):
            line = lines[i]
            result.append(line)
            i += 1
            if line.rstrip("\n") == block["start"]:
                start = i
                while i < len(lines) and re.fullmatch(block["line_pattern"], lines[i].rstrip("\n")):
                    i += 1
                # ソートだけを行い、名前の欠落・追加・重複は残して検出する。
                result.extend(sorted(lines[start:i]))
        lines = result
    return "".join(lines)


def tree(root, snapshot=False):
    return sorted((p.relative_to(root).as_posix(), "dir" if p.is_dir() else "file") for p in root.rglob("*") if not (snapshot and p.name == ".gitkeep"))


def configured_headers(report_list, rules):
    result = {}
    for report in report_list:
        for trial in report["trials"]:
            for side in ("one", "other"):
                access, request = {}, {}
                if f"summary.{side}.headers" in rules["configured_sources"]:
                    access = report["summary"][side].get("headers", {})
                if "trials.*.headers" in rules["configured_sources"]:
                    request = trial.get("headers", {})
                names = {k.lower() for k in access.keys() | request.keys()}
                merged = {"User-Agent": "jumeaux/<version>", **access, **request}
                merged = {k.lower(): v for k, v in merged.items()}
                variant = {k: v for k, v in merged.items() if k in names}
                result.setdefault((trial["method"], trial[side]["url"]), []).append(variant)
    return result


def request_headers(row, rules, configured):
    require(rules["name_case"] == "insensitive" and rules["order"] == "ignore" and rules["unconfigured"] == "ignore", "unsupported request header rule")
    received = {name.lower(): value for name, value in row["headers"]}
    variants = (configured or {}).get((row["method"], row["url"]), [{}])
    matches = [v for v in variants if all(received.get(k) == value for k, value in v.items())]
    require(bool(matches), f"configured headers missing/changed: {row}")
    explicit = max(matches, key=len)
    names = (set(rules["always_compare"]) | explicit.keys()) - set(rules.get("exclude", []))
    selected = {k: v for k, v in received.items() if k in names}
    if "user-agent" in selected and "user-agent" not in explicit:
        selected["user-agent"] = re.sub(r"^(jumeaux|python-requests)/[^ ]+$", r"\1/<version>", selected["user-agent"])
    return sorted(selected.items())


def records(path, header_rules=None, configured=None):
    result = Counter()
    occurrences = Counter()
    for line in path.read_text().splitlines():
        row = json.loads(line)
        occurrence = row.pop("occurrence")
        identity = json.dumps(row, sort_keys=True)
        occurrences[identity] += 1
        require(occurrence == occurrences[identity], f"{path}: occurrence count mismatch")
        if header_rules:
            row["headers"] = request_headers(row, header_rules, configured)
        result[json.dumps(row, sort_keys=True)] += 1
    return dict(result)


def workspace_tree(workspace, report_list, snapshot=False):
    keys = {report["key"]: f"<key:{i}>" for i, report in enumerate(report_list)}
    result = {}
    for file in workspace.rglob("*"):
        if snapshot and file.name == ".gitkeep":
            continue
        name = "/".join(keys.get(part, part) for part in file.relative_to(workspace).parts)
        kind = "symlink" if file.is_symlink() else "dir" if file.is_dir() else "file"
        target = keys.get(str(file.readlink()), str(file.readlink())) if file.is_symlink() else None
        result[name] = (kind, target, file)
    return result


def compare_case(baseline, actual, rules, input_baseline=None, rust=False):
    baseline, actual = Path(baseline), Path(actual)
    expect_reports, actual_reports = reports(baseline), reports(actual)
    validate_case_contract(baseline.name, expect_reports, rules)
    validate_case_contract(baseline.name, actual_reports, rules)
    equal(len(expect_reports), len(actual_reports), "report count")
    for one, other in zip(expect_reports, actual_reports):
        if rust:
            one = copy.deepcopy(one)
            settings = rules["rust_comparison_by_case"].get(actual.name, rules["rust_comparison_default"])
            one["summary"]["comparison"] = settings
        equal(normalize_report(one, rules), normalize_report(other, rules), "report")
        left, right = baseline / "responses" / one["key"], actual / "responses" / other["key"]
        equal(tree(left, snapshot=True), tree(right, snapshot=baseline == actual), "saved file tree")
        for rel, kind in tree(left, snapshot=True):
            if kind == "file" and rel != "report.json":
                equal((left / rel).read_bytes(), (right / rel).read_bytes(), rel)
    for path in ("api.jsonl", "notifications.jsonl"):
        if not (baseline / path).exists():
            continue
        header_rules = rules["request_header_rules"]
        if path == "notifications.jsonl":
            header_rules = {**header_rules, "exclude": rules["notification_header_exclusions"]}
        configured = configured_headers(expect_reports, header_rules) if path == "api.jsonl" else {}
        equal(records(baseline / path, header_rules, configured), records(actual / path, header_rules, configured), path)
    if actual.name.startswith("notify_"):
        require(sum(records(actual / "notifications.jsonl").values()) == 1, "notification missing/duplicated")
    for label in ("cli", "init", "run", "retry"):
        for stream in ("stdout", "stderr", "exit-code"):
            path = f"{label}.{stream}"
            equal((baseline / path).exists(), (actual / path).exists(), path + " existence")
            if (baseline / path).exists():
                equal(text((baseline / path).read_bytes(), expect_reports, rules, path), text((actual / path).read_bytes(), actual_reports, rules, path), path)
    left = workspace_tree(baseline, expect_reports, snapshot=True)
    right = workspace_tree(actual, actual_reports, snapshot=baseline == actual)
    equal(set(left), set(right), "workspace tree")
    for name, (kind, target, file) in left.items():
        other_kind, other_target, other_file = right[name]
        equal((kind, target), (other_kind, other_target), name)
        report_paths = {"report.json", "source-report.json"} | {f"responses/<key:{i}>/report.json" for i in range(len(expect_reports))}
        if kind != "file" or name in {"api.jsonl", "notifications.jsonl"} | report_paths:
            continue
        if file.name in {f"{label}.{stream}" for label in ("cli", "init", "run", "retry") for stream in ("stdout", "stderr", "exit-code")}:
            continue
        if input_baseline and name == "config.yml":
            file = Path(input_baseline) / name
        expected_bytes = file.read_bytes()
        if rust and actual.name in ("encoding_hell", "encoding_fixed") and name in ("config.yml", "init-config.yml"):
            expected_bytes = expected_bytes.replace(b"conditions:\n              - path:\n                  items:\n                    - regexp: .+", b"when: 'True'")
        if rust and actual.name == "encoding_hell" and name == "requests":
            expected_bytes = b"Invalid encoding,GET,/diff-1.json\n\n"
        equal(expected_bytes, other_file.read_bytes(), name)


def validate_case_contract(case, report_list, rules):
    for path, expected in rules["case_contracts"].get(case, {}).items():
        value = report_list[-1]
        for key in path.split("."):
            value = value[key]
        equal(expected, value, case + "." + path)
    if case == "retry_notifier":
        require(len(report_list) == 2, "retry_notifier: source missing")
        require(bool(report_list[0].get("notifiers")), "retry_notifier: notifiers missing")
        equal(report_list[0]["notifiers"], report_list[1].get("notifiers"), "retry notifiers")
