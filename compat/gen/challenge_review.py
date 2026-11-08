"""Pythonの進捗ログと保存先再使用を観測する。JSONファイル・外部HTTPは読まない。"""
from datetime import timedelta
import json
import os
from pathlib import Path
import sys
import tempfile
from unittest.mock import patch
import warnings

SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
sys.path.insert(0, str(SOURCE))
sys.dont_write_bytecode = True
import requests
from jumeaux import executor
from jumeaux.addons import AddOnExecutor
from jumeaux.models import Addons
warnings.filterwarnings("ignore")
cases = []


def directories(root):
    return sorted(str(p.relative_to(root)) for p in root.rglob("*") if p.is_dir())


for setup in ["missing", "empty", "unrelated", "one", "other", "file"]:
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        key = root / "responses" / "key"
        if setup == "file":
            key.parent.mkdir()
            key.write_bytes(b"existing")
        elif setup != "missing":
            key.mkdir(parents=True)
            if setup == "unrelated":
                (key / "keep").write_bytes(b"existing")
            elif setup in ["one", "other"]:
                (key / setup).mkdir()
        error_kind, error_path = None, None
        try:
            for side in ["one", "other", "one-props", "other-props"]:
                executor.make_dir(str(key / side))
        except OSError as error:
            error_kind = type(error).__name__
            error_path = str(Path(error.filename).relative_to(root))
        cases.append({"name": "storage-" + setup, "input": {"layer": "storage", "setup": setup},
                      "expected": {"error": error_kind, "error_path": error_path,
                                   "directories": directories(root)}})


def response(body):
    raw = requests.Response()
    raw.status_code = 200
    raw._content = body
    raw.encoding = "utf-8"
    raw.headers["Content-Type"] = "application/json"
    raw.url = "http://localhost/a"
    raw.elapsed = timedelta(0)
    return raw


for label, name, status, method in [
    ("same-unnamed", None, "same", "GET"),
    ("same-empty", "", "same", "GET"),
    ("different-named", "日本 (a)", "different", "GET"),
    ("different-post", None, "different", "POST"),
    ("failure-unnamed", None, "failure", "GET"),
    ("failure-empty", "", "failure", "GET"),
    ("failure-named", "日本", "failure", "GET"),
    ("storage-error", "a/b", "same", "GET"),
    ("parse-error", None, "invalid", "GET"),
]:
    req = {"name": name, "path": "/a", "method": method}
    logs = []
    addons = Addons.from_dict({"log2reqs": {"name": "plain"}, "res2dict": [{"name": "json"}],
                              "store_criterion": [{"name": "free"}]})
    with tempfile.TemporaryDirectory() as temp:
        for side in ["one", "other", "one-props", "other-props"]:
            executor.make_dir(f"{temp}/key/{side}")
        pair = (response(b'{' if status == "invalid" else b'{"a":1}'),
                response(b'{"a":2}' if status == "different" else b'{"a":1}'))
        with patch.object(executor, "global_addon_executor", AddOnExecutor(addons), create=True), \
             patch.object(executor, "concurrent_request", return_value=pair,
                          side_effect=requests.exceptions.ConnectionError() if status == "failure" else None), \
             patch.object(executor.logger, "info_lv1", side_effect=lambda message: logs.append({"verbosity": 1, "message": message})), \
             patch.object(executor.logger, "info_lv2", side_effect=lambda message: logs.append({"verbosity": 2, "message": message})):
            error_kind = None
            try:
                executor.challenge({"seq": 1, "number_of_request": 8, "key": "key", "session": object(),
                                    "req": req, "host_one": "http://localhost", "host_other": "http://localhost",
                                    "headers_one": {}, "headers_other": {}, "res_dir": temp,
                                    "judge_response_header": False, "ignore_response_header_keys": []})
            except Exception as error:
                error_kind = type(error).__name__
        cases.append({"name": label, "input": {"layer": "progress", "request": req, "status": status},
                      "expected": {"error": error_kind, "logs": logs}})

print(json.dumps(cases, ensure_ascii=False, indent=2))
