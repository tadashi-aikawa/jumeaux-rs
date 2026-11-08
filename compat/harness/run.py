"""基準採取と、任意のJUMEAUX_BINに対する比較。RustではViewerの削除をcompare.pyで反映する。"""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PYTHON_ROOT = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux")).resolve()
RULES = json.loads((HERE / "comparison.json").read_text())
TEMPLATES = [
    "all_same", "encoding_hell", "force_json", "html", "ignore", "ignore_order",
    "judge_response_headers", "notifier", "path_custom", "path_empty", "post",
    "query_custom", "request_headers", "root_array", "simple", "xml",
]
EXTRAS = {"notify_v1": "notifier", "notify_v2": "notifier", "notify_v2_blocks": "notifier", "retry_path_empty": "path_empty"}
EXTRAS.update({
    "threads_2": "simple", "processes_2": "simple", "threads_and_processes": "simple",
    "verbose_1": "simple", "verbose_2": "simple", "verbose_3": "simple",
    "retry_notifier": "notifier", "encoding_fixed": "encoding_hell", "output_encoding": "simple",
    "header_ignore_case": "judge_response_headers", "header_ignore_control": "judge_response_headers", "header_condition_case": "simple",
})
RUN_ARGS = {
    "threads_2": ["--threads", "2"], "processes_2": ["--processes", "2"],
    "threads_and_processes": ["--threads", "4", "--processes", "2"],
    "verbose_1": ["-v"], "verbose_2": ["-vv"], "verbose_3": ["-vvv"],
}
CLI_CASES = {"help": (["-h"], 0), "init_no_args": (["init"], 1), "init_invalid": (["init", "hogehoge"], 1)}


def environment():
    env = os.environ.copy()
    for key in list(env):
        if key.lower().endswith("_proxy") or key.startswith("SLACK_"):
            del env[key]
    env.update({
        "PYTHONPATH": str(PYTHON_ROOT), "PYTHONDONTWRITEBYTECODE": "1",
        "PYTHONHASHSEED": "0", "TZ": "Asia/Tokyo", "NO_PROXY": "localhost,127.0.0.1",
        "PYTHONWARNINGS": "ignore",
        "SLACK_INCOMING_WEBHOOKS_URL": f"http://localhost:{RULES['server']['notification_port']}/webhook",
    })
    return env


def execute(binary, argv, workspace, label):
    result = subprocess.run([str(binary), *argv], cwd=workspace, env=environment(), capture_output=True, timeout=120)
    for stream in ("stdout", "stderr"):
        (workspace / f"{label}.{stream}").write_bytes(getattr(result, stream))
    (workspace / f"{label}.exit-code").write_text(str(result.returncode) + "\n")
    if result.returncode and label == "init":
        raise RuntimeError(f"{workspace.name}: {label} exited {result.returncode}\n{result.stderr.decode(errors='replace')}")
    return result.returncode


def prepare(workspace, case):
    config = workspace / "config.yml"
    original = config.read_text()
    (workspace / "init-config.yml").write_text(original)
    text = original
    for before, after in RULES["fixed_inputs"].items():
        text = text.replace(before, after)
    if case == "encoding_fixed":
        text = text.replace("conditions:\n              - path:\n                  items:\n                    - regexp: .+", "when: 'True'")
        (workspace / "requests").write_text("Invalid encoding,GET,/diff-1.json\n")
    if case == "output_encoding":
        text = text.replace("response_dir: responses", "response_dir: responses\n  encoding: utf-16")
    if case in ("header_ignore_case", "header_ignore_control"):
        if case == "header_ignore_case":
            text = text.replace('"Last-Modified"', '"last-modified"')
        # 同じ本文でヘッダだけが違う状況を作る。
        (workspace / "requests").write_text("/same-1.json\n")
        (workspace / "api/one/same-1.json").write_text('{"id": 1}\n')
        (workspace / "api/other/same-1.json").write_text('{"id": 1}\n')
    if case == "header_condition_case":
        text = text.replace("  store_criterion:", "  judgement:\n    - name: same\n      config:\n        when_any:\n          - \"res_one.headers['lAsT-mOdIfIeD'] == 'Sat, 01 Jan 2000 00:00:00 GMT' and res_other.headers['LAST-MODIFIED'] == 'Sun, 02 Jan 2000 00:00:00 GMT'\"\n\n  store_criterion:")
    if case.startswith("notify_"):
        if case != "notify_v1":
            text = text.replace("type: slack", "type: slack\n    version: 2")
        if case == "notify_v2_blocks":
            text = text.replace("version: 2", "version: 2\n    use_blocks: true")
            message = '[{"type":"section","text":{"type":"plain_text","text":"{{ title }} 完了"}}]'
        else:
            message = "{{ title }} is Finish!! There are {{ summary.status.different }} diffs.."
        # finalレイヤー内の最後へ追加。元テンプレートのコメントも維持する。
        text += "\n    - name: notify\n      config:\n        notifies:\n          - notifier: jumeaux\n            message: '" + message + "'\n"
    config.write_text(text)


def ports_available():
    for port in (RULES["server"]["api_port"], RULES["server"]["notification_port"]):
        with socket.socket() as client:
            client.settimeout(0.3)
            if client.connect_ex(("127.0.0.1", port)) == 0:
                raise RuntimeError(f"receiver port already in use: {port}")


@contextmanager
def receiver(output):
    ports_available()
    log_path = output / "receiver.log"
    with log_path.open("wb") as log:
        server = subprocess.Popen(
            [str(PYTHON_ROOT / ".venv/bin/python"), "-B", str(HERE / "receivers.py"), str(output)],
            cwd=output, env=environment(), stdin=subprocess.PIPE, text=True, stdout=log, stderr=log,
        )
        try:
            wait_ready(server, output, log_path)
            yield server
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
            server.stdin.close()
    log_path.unlink()


def wait_ready(server, workspace, log_path):
    deadline = time.monotonic() + 140
    while not (workspace / "receiver.ready").exists():
        if server.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError("receiver failed: " + log_path.read_text())
        time.sleep(0.05)
    (workspace / "receiver.ready").unlink()


def run_case(binary, case, workspace, server, extra_args=()):
    template = EXTRAS.get(case, case)
    workspace.mkdir()
    if case in CLI_CASES:
        argv, expected = CLI_CASES[case]
        code = execute(binary, argv, workspace, "cli")
        if code != expected or set(p.name for p in workspace.iterdir()) != {"cli.stdout", "cli.stderr", "cli.exit-code"}:
            raise AssertionError(f"{case}: unexpected exit or files")
        return
    execute(binary, ["init", template], workspace, "init")
    prepare(workspace, case)
    server.stdin.write(str(workspace) + "\n")
    server.stdin.flush()
    wait_ready(server, workspace, workspace.parent / "receiver.log")
    code = execute(binary, ["run", "requests", *RUN_ARGS.get(case, []), *extra_args], workspace, "run")
    if code != (1 if case == "encoding_hell" and binary == HERE / "python-jumeaux" else 0):
        raise AssertionError(f"{case}: unexpected run exit {code}")
    if case.startswith("retry_"):
        shutil.copyfile(workspace / "responses/latest/report.json", workspace / "source-report.json")
        if execute(binary, ["retry", "source-report.json"], workspace, "retry") != 0:
            raise AssertionError(f"{case}: retry failed")
    # latestでなく、参照元reportも保存し、retryの関係を検証する。
    report = workspace / "responses/latest/report.json"
    if report.is_file():
        shutil.copyfile(report, workspace / "report.json")
        from compare import reports, validate_case_contract
        contract = RULES["python_mismatches"].get(case, case) if binary != HERE / "python-jumeaux" else case
        if not extra_args:
            validate_case_contract(contract, reports(workspace), RULES)
    (workspace / "artifacts.json").write_text(json.dumps({
        "report_exists": report.is_file(),
        "responses_exists": (workspace / "responses").exists(),
    }, indent=2) + "\n")


def preserve_empty_directories(workspace):
    # Gitで空ディレクトリを保持するための基準専用マーカー。
    for folder in workspace.rglob("*"):
        if folder.is_dir() and not folder.is_symlink() and not any(folder.iterdir()):
            (folder / ".gitkeep").touch()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--record", action="store_true")
    parser.add_argument("--baseline", type=Path, default=ROOT / "compat/python-baseline")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--case", action="append", choices=TEMPLATES + list(EXTRAS) + list(CLI_CASES))
    parser.add_argument("--run-arg", action="append", default=[], help="追加run引数。例: --run-arg=--threads --run-arg=2")
    args = parser.parse_args()
    binary = Path(os.environ.get("JUMEAUX_BIN", ROOT / "target/debug/jumeaux")).resolve()
    cases = args.case or TEMPLATES + list(EXTRAS) + list(CLI_CASES)
    if args.record and binary != HERE / "python-jumeaux":
        raise RuntimeError("基準の採取にはJUMEAUX_BIN=compat/harness/python-jumeauxを指定してください")
    output = args.baseline.resolve() if args.record else (args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="jumeaux-compat-")))
    output.mkdir(parents=True, exist_ok=True)
    if any((output / case).exists() for case in cases):
        raise RuntimeError(f"既存結果を上書きしません: {output}")
    failures = []
    with receiver(output) as server:
        for case in cases:
            try:
                run_case(binary, case, output / case, server, args.run_arg)
                if args.record:
                    preserve_empty_directories(output / case)
                if not args.record:
                    from compare import compare_case
                    corrected = RULES["python_mismatches"].get(case) if binary != HERE / "python-jumeaux" else None
                    if case == "encoding_hell" and binary != HERE / "python-jumeaux":
                        corrected = "encoding_fixed"
                    compare_case(args.baseline / (corrected or case), output / case, RULES, input_baseline=args.baseline / case, rust=binary != HERE / "python-jumeaux")
                print(f"PASS {case}", flush=True)
            except Exception as error:
                failures.append(case)
                print(f"FAIL {case}: {error}", flush=True)
    print(f"{len(cases)-len(failures)}/{len(cases)} {'recorded' if args.record else 'matched'}; output={output}")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
