"""基準採取と、任意のJUMEAUX_BINに対する同一入力の比較。"""

import argparse
import json
import os
from pathlib import Path
import shutil
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


def environment():
    env = os.environ.copy()
    for key in list(env):
        if key.lower().endswith("_proxy") or key.startswith("SLACK_"):
            del env[key]
    env.update({
        "PYTHONPATH": str(PYTHON_ROOT), "PYTHONDONTWRITEBYTECODE": "1",
        "PYTHONHASHSEED": "0", "TZ": "Asia/Tokyo", "NO_PROXY": "localhost,127.0.0.1",
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


def run_case(binary, case, workspace):
    template = EXTRAS.get(case, case)
    workspace.mkdir()
    execute(binary, ["init", template], workspace, "init")
    prepare(workspace, case)
    log = (workspace / "receiver.log").open("wb")
    server = subprocess.Popen(
        [str(PYTHON_ROOT / ".venv/bin/python"), "-B", str(HERE / "receivers.py"), str(workspace)],
        cwd=workspace, env=environment(), stdout=log, stderr=log,
    )
    try:
        deadline = time.monotonic() + 15
        while not (workspace / "receiver.ready").exists():
            if server.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("receiver failed: " + (workspace / "receiver.log").read_text())
            time.sleep(0.05)
        execute(binary, ["run", "requests"], workspace, "run")
        if case.startswith("retry_"):
            shutil.copyfile(workspace / "responses/latest/report.json", workspace / "source-report.json")
            execute(binary, ["retry", "source-report.json"], workspace, "retry")
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        log.close()
    (workspace / "receiver.ready").unlink(missing_ok=True)
    (workspace / "receiver.log").unlink()
    # latestでなく、参照元reportも保存し、retryの関係を検証する。
    report = workspace / "responses/latest/report.json"
    if report.is_file():
        shutil.copyfile(report, workspace / "report.json")
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
    parser.add_argument("--case", action="append", choices=TEMPLATES + list(EXTRAS))
    args = parser.parse_args()
    binary = Path(os.environ.get("JUMEAUX_BIN", ROOT / "target/debug/jumeaux")).resolve()
    cases = args.case or TEMPLATES + list(EXTRAS)
    if args.record and binary != HERE / "python-jumeaux":
        raise RuntimeError("基準の採取にはJUMEAUX_BIN=compat/harness/python-jumeauxを指定してください")
    output = args.baseline.resolve() if args.record else (args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="jumeaux-compat-")))
    output.mkdir(parents=True, exist_ok=True)
    if any((output / case).exists() for case in cases):
        raise RuntimeError(f"既存結果を上書きしません: {output}")
    failures = []
    for case in cases:
        try:
            run_case(binary, case, output / case)
            if args.record:
                preserve_empty_directories(output / case)
            if not args.record:
                from compare import compare_case
                compare_case(args.baseline / case, output / case, RULES)
            print(f"PASS {case}", flush=True)
        except Exception as error:
            failures.append(case)
            print(f"FAIL {case}: {error}", flush=True)
    print(f"{len(cases)-len(failures)}/{len(cases)} {'recorded' if args.record else 'matched'}; output={output}")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
