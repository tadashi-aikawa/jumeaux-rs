"""保存済みPython旧reportを両CLIでretryし、送信と新reportを比較する。"""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from compare import configured_headers, equal, load, normalize_report, records, tree, validate_report
from exec_samples import wait_ready
from run import HERE, PYTHON_ROOT, ROOT, RULES, environment

RUST = Path(os.environ.get("JUMEAUX_BIN", ROOT / "target/debug/jumeaux")).resolve()
PYTHON = PYTHON_ROOT / ".venv/bin/python"
BASELINE = ROOT / "compat/python-baseline"


def execute(binary, work, env, *args, code=0):
    result = subprocess.run([str(binary), *args], cwd=work, env=env, capture_output=True, timeout=120)
    (work / "retry.stdout").write_bytes(result.stdout)
    (work / "retry.stderr").write_bytes(result.stderr)
    assert result.returncode == code, result.stderr.decode(errors="replace")
    return result


def source_report(case, port):
    key = load(BASELINE / case / "report.json")["key"]
    source = load(BASELINE / case / "responses" / key / "report.json")
    assert "comparison" not in source["summary"]
    for side in ("one", "other"):
        source["summary"][side]["host"] = source["summary"][side]["host"].replace(":8000", f":{port}")
    return source


def select(server, work):
    server.stdin.write(str(work) + "\n")
    server.stdin.flush()
    wait_ready(server, work)


def observe(work, source):
    report = load(work / "responses/latest/report.json")
    folder = work / "responses" / report["key"]
    validate_report(report, folder, source["key"])
    assert report["key"] != source["key"]
    assert (work / "responses/latest").resolve() == folder.resolve()
    saved = {rel: (folder / rel).read_bytes() for rel, kind in tree(folder) if kind == "file" and rel != "report.json"}
    headers = RULES["request_header_rules"]
    requests = records(work / "api.jsonl", headers, configured_headers([report], headers))
    return report, saved, requests


def compare_retry(expected, actual):
    report = copy.deepcopy(expected[0])
    report["summary"]["comparison"] = RULES["rust_comparison_default"]
    equal(normalize_report(report, RULES), normalize_report(actual[0], RULES), "retry report")
    equal(expected[1], actual[1], "response/prop/Viewer bytes")
    equal(expected[2], actual[2], "recorded requests")


def main():
    output = Path(tempfile.mkdtemp(prefix="jumeaux-old-retry-"))
    env = environment()
    server = subprocess.Popen([str(PYTHON), "-B", str(HERE / "receivers.py"), str(output), "--api-port", "0", "--notification-port", "0"],
                              env=env, stdin=subprocess.PIPE, text=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_ready(server, output)
        port, webhook = (int(p) for p in (output / "receiver.ports").read_text().splitlines())
        env["SLACK_INCOMING_WEBHOOKS_URL"] = f"http://localhost:{webhook}/webhook"
        for case in ("simple", "path_empty", "notifier", "request_headers", "judge_response_headers", "post"):
            source = source_report(case, port)
            observations = []
            for label, binary in (("python", HERE / "python-jumeaux"), ("rust", RUST)):
                work = output / case / label
                work.mkdir(parents=True)
                shutil.copytree(BASELINE / case / "api", work / "api", ignore=shutil.ignore_patterns(".gitkeep"))
                write_json(work / "source.json", source)
                select(server, work)
                result = execute(binary, work, env, "retry", "source.json")
                if label == "rust":
                    assert b"report.summary.comparison: missing" in result.stderr
                    assert b"using Python defaults (false, Content-Length/Date)" in result.stderr
                observations.append(observe(work, source))
            if case != "post":
                compare_retry(*observations)
                print(f"PASS old {case}: requests, report, saved bytes matched; defaults warned")
                continue
            # Pythonのraw喪失は既知の修正対象。rawなしのRust対照実行と完全一致を確認する。
            control = output / case / "rust-without-raw"
            control.mkdir()
            shutil.copytree(BASELINE / case / "api", control / "api")
            legacy = copy.deepcopy(source)
            legacy["trials"][2]["raw"] = None
            write_json(control / "source.json", legacy)
            select(server, control)
            execute(RUST, control, env, "retry", "source.json")
            compare_retry(observations[0], observe(control, legacy))
            expected, actual = observations
            assert expected[0]["trials"][2].get("raw") is None
            assert actual[0]["trials"][2]["raw"] == "a=100&b=200"
            for index in (0, 1):
                equal(normalize_report(expected[0]["trials"][index], RULES, ("trials", str(index))),
                      normalize_report(actual[0]["trials"][index], RULES, ("trials", str(index))), "non-raw trial")
            # 同じURLへJSONとrawを送る。JSONの2件は共通、残り2件だけ本文が変わる。
            from collections import Counter
            python_rows, rust_rows = (Counter(obs[2]) for obs in observations)
            assert sum((python_rows - rust_rows).values()) == 2
            assert sum((rust_rows - python_rows).values()) == 2
            rows = [json.loads(line) for line in (output / case / "rust/api.jsonl").read_text().splitlines()]
            assert sum(row["method"] == "POST" and row["body_base64"] == "YT0xMDAmYj0yMDA=" for row in rows) == 2
            print("PASS old post: form/JSON matched; raw restored on Rust only (2 requests); Python matches no-raw control")

        work = output / "unknown-logger"
        work.mkdir()
        shutil.copytree(BASELINE / "simple/api", work / "api")
        source = source_report("simple", port)
        source["unknown"] = True
        source["summary"]["output"]["logger"] = {"old": True}
        write_json(work / "source.json", source)
        select(server, work)
        result = execute(RUST, work, env, "retry", "source.json")
        for warning in (b"report.unknown", b"summary.output.logger", b"comparison: missing"):
            assert warning in result.stderr
        saved = observe(work, source)[0]
        assert "unknown" not in saved and "logger" not in saved["summary"]["output"]
        print("PASS old unknown/logger: field-discard and comparison-default warnings; clean new report")

        work = output / "exec-permission"
        work.mkdir()
        source = source_report("simple", port)
        source["trials"] = []
        addon = {"name": "exec", "config": {"command": [str(PYTHON), str(ROOT / "tests/fixtures/exec.py")], "options": {"marker": "marker"}}}
        source["addons"] = {"log2reqs": addon, "final": [addon, {"name": "json"}]}
        write_json(work / "source.json", source)
        refused = execute(RUST, work, env, "retry", "source.json", code=1)
        assert b"--allow-exec" in refused.stderr and not (work / "marker").exists()
        assert not (work / "responses").exists()
        execute(RUST, work, env, "retry", "source.json", "--allow-exec")
        assert (work / "marker").read_text() == "log2reqs validate\nfinal validate\nfinal payload\n"
        assert "allow_exec" not in load(work / "responses/latest/report.json")
        execute(RUST, work, env, "retry", "responses/latest/report.json", code=1)
        assert len((work / "marker").read_text().splitlines()) == 3
        print("PASS exec retry: denied before validate, explicit allow succeeds, permission is not persisted")
        print(f"8/8 scenarios passed; output={output}")
    finally:
        server.stdin.close()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False))


if __name__ == "__main__":
    main()
