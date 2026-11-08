"""比較ハーネスが不正な出力を見逃さないためのmutationテスト。"""

import copy
import json
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from compare import compare_case, equal, load, normalize_report, records, text, validate_report

HERE = Path(__file__).resolve().parent
BASELINE = HERE.parent / "python-baseline"
RULES = load(HERE / "comparison.json")


class ComparisonTests(unittest.TestCase):
    def test_stage_elapsed_normalization_requires_microsecond_precision(self):
        for duration in ("0.0", "0.057", "1.23"):
            line = f"[1 / 2] ⏰ Judgement:   {duration}ms\n".encode()
            self.assertEqual(text(line, [], RULES), "[1 / 2] ⏰ Judgement:   <elapsed_sec>\n")
        for duration in ("0", "0.057123", "1.0000", "-0.1", "1e-5"):
            line = f"[1 / 2] ⏰ Judgement:   {duration}ms\n".encode()
            self.assertEqual(text(line, [], RULES), line.decode())

    def test_rust_comparison_settings_are_required_and_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "simple"
            shutil.copytree(BASELINE / "simple", target, symlinks=True)
            with self.assertRaises(AssertionError):
                compare_case(BASELINE / "simple", target, RULES, rust=True)
            report = load(target / "report.json")
            report["summary"]["comparison"] = copy.deepcopy(RULES["rust_comparison_default"])
            for path in [target / "report.json", target / "responses" / report["key"] / "report.json"]:
                path.write_text(json.dumps(report))
            compare_case(BASELINE / "simple", target, RULES, rust=True)
            report["summary"]["comparison"]["judge_response_header"] = True
            for path in [target / "report.json", target / "responses" / report["key"] / "report.json"]:
                path.write_text(json.dumps(report))
            with self.assertRaises(AssertionError):
                compare_case(BASELINE / "simple", target, RULES, rust=True)

    def test_all_baselines_are_self_consistent(self):
        from run import TEMPLATES, EXTRAS, CLI_CASES
        for case in TEMPLATES + list(EXTRAS) + list(CLI_CASES):
            with self.subTest(case=case):
                compare_case(BASELINE / case, BASELINE / case, RULES)

    def test_missing_field_is_not_ignored(self):
        report = load(BASELINE / "simple/report.json")
        changed = copy.deepcopy(report)
        del changed["trials"][0]["request_time"]
        with self.assertRaises(AssertionError):
            equal(normalize_report(report, RULES), normalize_report(changed, RULES), "report")

    def test_same_name_in_payload_is_not_ignored(self):
        report = load(BASELINE / "post/report.json")
        report["trials"][1]["json"]["key"] = "before"
        changed = copy.deepcopy(report)
        changed["trials"][1]["json"]["key"] = "after"
        with self.assertRaises(AssertionError):
            equal(normalize_report(report, RULES), normalize_report(changed, RULES), "report")

    def test_invalid_times_and_duration_are_rejected(self):
        report = load(BASELINE / "simple/report.json")
        for value in (-1, float("inf"), True, "0"):
            with self.subTest(value=value), self.assertRaises(AssertionError):
                changed = copy.deepcopy(report)
                changed["trials"][0]["one"]["response_sec"] = value
                normalize_report(changed, RULES)
        with self.assertRaises(ValueError):
            changed = copy.deepcopy(report)
            changed["trials"][0]["request_time"] = "broken"
            normalize_report(changed, RULES)

    def test_response_header_case_only_is_allowed(self):
        report = load(BASELINE / "judge_response_headers/report.json")
        changed = copy.deepcopy(report)
        headers = changed["trials"][0]["one"]["response_header"]
        changed["trials"][0]["one"]["response_header"] = {key.lower(): value for key, value in headers.items()}
        equal(normalize_report(report, RULES), normalize_report(changed, RULES), "report")
        changed["trials"][0]["one"]["response_header"]["content-type"] = "wrong/type"
        with self.assertRaises(AssertionError):
            equal(normalize_report(report, RULES), normalize_report(changed, RULES), "report")

    def test_retry_reference_is_checked(self):
        report = load(BASELINE / "retry_path_empty/report.json")
        folder = BASELINE / "retry_path_empty/responses" / report["key"]
        with self.assertRaises(AssertionError):
            validate_report(report, folder, "wrong-source")

    def test_numeric_types_are_distinct(self):
        for value in (True, 1.0):
            with self.assertRaises(AssertionError):
                equal(1, value, "payload")

    def test_normal_run_rejects_null_retry_hash(self):
        report = load(BASELINE / "simple/report.json")
        report["retry_hash"] = None
        with self.assertRaises(AssertionError):
            validate_report(report, BASELINE / "simple/responses" / report["key"])

    def change_headers(self, target, transform, filename="api.jsonl"):
        path = target / filename
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        for row in rows:
            row["headers"] = transform(row["headers"])
        path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")

    def test_host_and_header_order_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "simple"
            shutil.copytree(BASELINE / "simple", target, symlinks=True)
            self.change_headers(target, lambda headers: [[name.swapcase(), "different-client" if name.lower() in RULES["request_header_rules"]["automatic_examples"] else value] for name, value in reversed(headers)])
            compare_case(BASELINE / "simple", target, RULES)

    def test_automatic_header_changes_are_detected(self):
        for name in ("accept", "accept-encoding", "connection", "content-length", "cookie", "user-agent"):
            with self.subTest(name=name):
                self.mutated_case("simple", lambda target: self.change_headers(target,
                    lambda headers: [[k, v] for k, v in headers if k.lower() != name] + [[name, "wrong"]]))

    def test_default_user_agent_version_is_normalized_per_trial(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "request_headers"
            shutil.copytree(BASELINE / "request_headers", target, symlinks=True)
            self.change_headers(target, lambda headers: [[k, "jumeaux/0.1.0" if k.lower() == "user-agent" and v == "jumeaux/7.0.0" else v] for k, v in headers])
            compare_case(BASELINE / "request_headers", target, RULES)

    def test_configured_user_agent_is_compared(self):
        self.mutated_case("request_headers", lambda target: self.change_headers(target, lambda headers: [[name, "wrong" if name.lower() == "user-agent" else value] for name, value in headers]))

    def test_notification_user_agent_is_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "notify_v1"
            shutil.copytree(BASELINE / "notify_v1", target, symlinks=True)
            self.change_headers(target, lambda headers: [[k, "jumeaux/0.1.0" if k.lower() == "user-agent" else v] for k, v in headers], "notifications.jsonl")
            compare_case(BASELINE / "notify_v1", target, RULES)

    def test_notification_other_automatic_headers_are_compared(self):
        for name in ("accept", "accept-encoding", "connection", "content-length", "cookie"):
            with self.subTest(name=name):
                self.mutated_case("notify_v1", lambda target: self.change_headers(target,
                    lambda headers: [[k, v] for k, v in headers if k.lower() != name] + [[name, "wrong"]], "notifications.jsonl"))

    def test_missing_configured_header_is_detected(self):
        self.mutated_case("request_headers", lambda target: self.change_headers(target, lambda headers: [[name, value] for name, value in headers if name.lower() != "xxx-header-key"]))

    def test_configured_header_case_and_order_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "request_headers"
            shutil.copytree(BASELINE / "request_headers", target, symlinks=True)
            self.change_headers(target, lambda headers: [[name.swapcase(), value] for name, value in reversed(headers)])
            compare_case(BASELINE / "request_headers", target, RULES)

    def test_content_type_is_compared_for_api_and_notifications(self):
        for case, filename in (("post", "api.jsonl"), ("notify_v1", "notifications.jsonl")):
            with self.subTest(case=case):
                self.mutated_case(case, lambda target: self.change_headers(target, lambda headers: [[name, value] for name, value in headers if name.lower() != "content-type"] + [["Content-Type", "wrong/type"]], filename))

    def test_unexpected_file_outside_responses_is_detected(self):
        self.mutated_case("simple", lambda target: (target / "unexpected.csv").write_text("extra"))

    def test_unexpected_file_for_failed_init_is_detected(self):
        self.mutated_case("init_invalid", lambda target: (target / "config.yml").touch())

    def test_wrong_report_encoding_is_detected(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "output_encoding"
            shutil.copytree(BASELINE / "output_encoding", target, symlinks=True)
            report = load(target / "report.json")
            path = target / "responses" / report["key"] / "report.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaises((UnicodeError, ValueError, AssertionError)):
                compare_case(BASELINE / "output_encoding", target, RULES)

    def test_header_ignore_corrected_baseline_rejects_python_bug(self):
        with self.assertRaises(AssertionError):
            compare_case(BASELINE / "header_ignore_control", BASELINE / "header_ignore_case", RULES, input_baseline=BASELINE / "header_ignore_case")

    def test_header_ignore_corrected_baseline_accepts_fixed_result(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "header_ignore_case"
            shutil.copytree(BASELINE / "header_ignore_control", target, symlinks=True)
            for marker in target.rglob(".gitkeep"):
                marker.unlink()
            shutil.copyfile(BASELINE / "header_ignore_case/config.yml", target / "config.yml")
            compare_case(BASELINE / "header_ignore_control", target, RULES, input_baseline=BASELINE / "header_ignore_case")

    def test_environment_generator_refuses_existing_baseline(self):
        result = subprocess.run([sys.executable, "-B", str(HERE.parent / "gen/environment.py")], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("既存の環境情報を上書きしません", result.stderr)

    def test_busy_port_is_rejected_before_process_start(self):
        import run
        with socket.socket() as occupied, tempfile.TemporaryDirectory() as directory:
            occupied.bind(("127.0.0.1", 0))
            occupied.listen()
            port = occupied.getsockname()[1]
            with patch.dict(run.RULES["server"], api_port=port), patch("run.subprocess.Popen") as spawn:
                with self.assertRaisesRegex(RuntimeError, "already in use"):
                    with run.receiver(Path(directory)):
                        self.fail("occupied port accepted")
                spawn.assert_not_called()
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_verbose_without_urllib3_logs_and_with_rust_version_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "verbose_3"
            shutil.copytree(BASELINE / "verbose_3", target, symlinks=True)
            path = target / "run.stderr"
            original = path.read_bytes()
            changed = b"".join(line for line in original.splitlines(keepends=True) if not line.startswith((b"Starting new HTTP connection", b"http://localhost:8000 ")))
            self.assertNotEqual(original, changed)
            self.assertIn(b"jumeaux/7.0.0", changed)
            path.write_bytes(changed.replace(b"jumeaux/7.0.0", b"jumeaux/0.1.0"))
            compare_case(BASELINE / "verbose_3", target, RULES)

    def test_urllib3_exclusion_is_limited_to_run_stderr(self):
        value = b'Starting new HTTP connection (1): localhost:8000\nhttp://localhost:8000 "GET /api/one/x HTTP/1.1" 200 10\n'
        self.assertEqual(text(value, [], RULES, "run.stderr"), "")
        for stream in ("run.stdout", "cli.stderr", "retry.stderr"):
            with self.subTest(stream=stream):
                self.assertEqual(text(value, [], RULES, stream), value.decode())

    def test_application_log_changes_are_detected(self):
        def mutate(target):
            path = target / "run.stderr"
            path.write_bytes(path.read_bytes().replace(b"[final/viewer] Create", b"[final/viewer] Wrong"))
        self.mutated_case("verbose_3", mutate)

    def test_template_list_order_is_ignored(self):
        for case in ("init_no_args", "init_invalid"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                target = Path(directory) / case
                shutil.copytree(BASELINE / case, target)
                path = target / "cli.stderr"
                prefix, names = path.read_text().split("✨ [Valid names] ✨\n", 1)
                for reverse in (False, True):
                    path.write_text(prefix + "✨ [Valid names] ✨\n" + "\n".join(sorted(names.splitlines(), reverse=reverse)) + "\n")
                    compare_case(BASELINE / case, target, RULES)

    def test_template_list_missing_extra_duplicate_are_detected(self):
        for replacement in ("", "unknown_template\n", "simple\nsimple\n"):
            with self.subTest(replacement=replacement):
                def mutate(target):
                    path = target / "cli.stderr"
                    path.write_text(path.read_text().replace("\nsimple\n", "\n" + replacement))
                self.mutated_case("init_invalid", mutate)

    def test_template_order_normalization_stays_inside_block(self):
        value = "zebra\napple\n✨ [Valid names] ✨\nzebra\napple\n\nzebra\napple\n"
        expected = "zebra\napple\n✨ [Valid names] ✨\napple\nzebra\n\nzebra\napple\n"
        self.assertEqual(text(value.encode(), [], RULES, "cli.stderr"), expected)
        self.assertEqual(text(value.encode(), [], RULES, "run.stderr"), value)

    def mutated_case(self, case, mutate):
        with tempfile.TemporaryDirectory(prefix="jumeaux-mutation-") as directory:
            target = Path(directory) / case
            shutil.copytree(BASELINE / case, target, symlinks=True)
            for marker in target.rglob(".gitkeep"):
                marker.unlink()
            mutate(target)
            with self.assertRaises(AssertionError):
                compare_case(BASELINE / case, target, RULES)

    def test_post_body_mutation_is_detected(self):
        def mutate(target):
            path = target / "api.jsonl"
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            rows[0]["body_base64"] = "d3Jvbmc="
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
        self.mutated_case("post", mutate)

    def test_missing_request_is_detected(self):
        def mutate(target):
            path = target / "api.jsonl"
            path.write_text("\n".join(path.read_text().splitlines()[1:]) + "\n")
        self.mutated_case("simple", mutate)

    def test_extra_request_is_detected(self):
        def mutate(target):
            path = target / "api.jsonl"
            row = json.loads(path.read_text().splitlines()[0])
            row["occurrence"] = 2
            with path.open("a") as stream:
                stream.write(json.dumps(row) + "\n")
        self.mutated_case("simple", mutate)

    def test_notification_body_mutation_is_detected(self):
        def mutate(target):
            path = target / "notifications.jsonl"
            row = json.loads(path.read_text())
            row["body_base64"] = "e30="
            path.write_text(json.dumps(row) + "\n")
        self.mutated_case("notify_v1", mutate)

    def test_response_byte_mutation_is_detected(self):
        def mutate(target):
            report = load(target / "report.json")
            path = target / "responses" / report["key"] / report["trials"][1]["one"]["file"]
            path.write_bytes(path.read_bytes() + b"\n")
        self.mutated_case("simple", mutate)

    def test_missing_artifact_is_detected(self):
        def mutate(target):
            (target / "responses/latest/index.html").unlink()
        self.mutated_case("simple", mutate)

    def test_wrong_latest_reference_is_detected(self):
        def mutate(target):
            latest = target / "responses/latest"
            latest.unlink()
            latest.symlink_to("wrong-key")
        self.mutated_case("simple", mutate)

    def test_occurrence_sequence_is_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "api.jsonl"
            row = json.loads((BASELINE / "simple/api.jsonl").read_text().splitlines()[0])
            row["occurrence"] = 9
            path.write_text(json.dumps(row) + "\n")
            with self.assertRaises(AssertionError):
                records(path)


if __name__ == "__main__":
    unittest.main()
