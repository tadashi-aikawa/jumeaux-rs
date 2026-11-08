"""生成した3雛形とPython版addon_sampleを、記録用ローカル受信先で比較する。"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PYTHON_ROOT = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
PYTHON = PYTHON_ROOT / ".venv/bin/python"
RUST = Path(os.environ.get("JUMEAUX_BIN", ROOT / "target/debug/jumeaux")).resolve()


def environment():
    env = {k: v for k, v in os.environ.items() if not k.lower().endswith("_proxy") and not k.startswith("SLACK_")}
    env.update(PYTHONPATH=os.pathsep.join([str(PYTHON_ROOT), str(PYTHON_ROOT / "jumeaux/sample/addon")]),
               PYTHONDONTWRITEBYTECODE="1", PYTHONWARNINGS="ignore", NO_PROXY="127.0.0.1,localhost")
    return env


def run(argv, work):
    result = subprocess.run([str(x) for x in argv], cwd=work, env=environment(), capture_output=True, timeout=40)
    assert result.returncode == 0, result.stderr.decode(errors="replace")
    return result


def wait_ready(server, work):
    deadline = time.monotonic() + 15
    while not (work / "receiver.ready").exists():
        assert server.poll() is None, "receiver exited"
        assert time.monotonic() < deadline, "receiver timeout"
        time.sleep(0.01)


def config(port, rust):
    def addon(layer, name, options):
        if rust:
            return {"name": "exec", "config": {"command": ["python3", f"addon_sample/{layer}/{name}.py"], "options": options}}
        return {"name": f"addon_sample.{layer}.{name}", "config": options}
    return {
        "one": {"name": "one", "host": f"http://127.0.0.1:{port}/api/one"},
        "other": {"name": "other", "host": f"http://127.0.0.1:{port}/api/other"},
        "output": {"response_dir": "responses"}, "max_retries": 0,
        "addons": {
            "log2reqs": {"name": "plain"},
            "reqs2reqs": [addon("reqs2reqs", "add_path", {"path": "/added.json"})],
            "res2dict": [{"name": "json"}],
            "judgement": [addon("judgement", "ignore_values", {"values": ["apple", "orange"]})],
            "store_criterion": [{"name": "free"}],
            "final": [addon("final", "table", {"columns": ["seq", "name", "path", "status"]}), {"name": "json"}],
        },
    }


def observe(work):
    root = (work / "responses/latest").resolve()
    report = json.loads((root / "report.json").read_text())
    trials = report["trials"]
    for trial in trials:
        assert trial.pop("request_time")
        for side in ("one", "other"):
            assert trial[side].pop("response_sec") >= 0
    files = {str(path.relative_to(root)): path.read_bytes() for directory in ("one", "other", "one-props", "other-props") for path in (root / directory).rglob("*") if path.is_file()}
    requests = sorted((r["method"], r["url"], r["body_base64"]) for r in (json.loads(line) for line in (work / "api.jsonl").read_text().splitlines()))
    return trials, files, requests


def main():
    with tempfile.TemporaryDirectory(prefix="jumeaux-exec-samples-") as temp:
        temp = Path(temp)
        server = subprocess.Popen([str(PYTHON), "-B", str(HERE / "receivers.py"), str(temp), "--api-port", "0", "--notification-port", "0"], env=environment(), stdin=subprocess.PIPE, text=True, stdout=subprocess.DEVNULL)
        try:
            wait_ready(server, temp)
            port = int((temp / "receiver.ports").read_text().splitlines()[0])
            observations = []
            for rust in (False, True):
                work = temp / ("rust" if rust else "python")
                work.mkdir()
                for side in ("one", "other"):
                    api = work / "api" / side
                    api.mkdir(parents=True)
                    (api / "fruit.json").write_text(json.dumps({"fruit": "apple" if side == "one" else "orange"}))
                    (api / "different.json").write_text(json.dumps({"id": 1 if side == "one" else 2}))
                    (api / "added.json").write_text('{"added": true}')
                (work / "requests").write_text("/fruit.json\n/different.json\n")
                (work / "config.yml").write_text(json.dumps(config(port, rust)))
                server.stdin.write(str(work) + "\n")
                server.stdin.flush()
                wait_ready(server, work)
                if rust:
                    run([RUST, "init", "addon"], work)
                    output = run([RUST, "run", "requests"], work)
                    assert b"seq\tname\tpath\tstatus" in output.stderr
                else:
                    run([PYTHON, "-B", HERE / "exec_sample_python.py", "run", "requests"], work)
                observations.append(observe(work))
            expected, actual = observations
            assert actual[0] == expected[0], (actual[0], expected[0])
            assert actual[1] == expected[1], "saved response/prop bytes differ"
            assert actual[2] == expected[2], "recorded HTTP requests differ"
            assert [t["status"] for t in actual[0]] == ["same", "different", "same"]
            assert actual[0][0]["diffs_by_cognition"]["ignore_values"]["changed"] == ["root<'fruit'>"]
            assert len(actual[0]) == 3 and len(actual[1]) == 12 and len(actual[2]) == 6
            print("3/3 templates; 3/3 trials; 12/12 files; 6/6 HTTP requests matched")
        finally:
            server.stdin.close()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


if __name__ == "__main__":
    main()
