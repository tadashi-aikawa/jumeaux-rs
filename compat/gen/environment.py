"""基準環境の版・lock・既存ユニットテストの結果を保存する。"""

import hashlib
from importlib.metadata import distributions
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get("JUMEAUX_PYTHON_ROOT", Path.home() / "git/github.com/tadashi-aikawa/jumeaux"))
DEST = ROOT / "compat/python-baseline"
sys.path.insert(0, str(ROOT / "compat/harness"))
from run import preserve_empty_directories  # noqa: E402

preserve_empty_directories(DEST)
commit = subprocess.check_output(["git", "-C", str(SOURCE), "rev-parse", "HEAD"], text=True).strip()
assert commit == "dc276b8e3a0486bd8d4de2c8e3900aa36cbb7a66"
lock = (SOURCE / "uv.lock").read_bytes()
(DEST / "uv.lock").write_bytes(lock)
packages = sorted({f"{d.metadata['Name']}=={d.version}" for d in distributions()})
locked = {(p["name"].lower().replace("_", "-"), p["version"]) for p in tomllib.loads(lock.decode())["package"]}
for package in packages:
    name, version = package.split("==")
    if name.lower() == "jumeaux":
        assert version == "7.0.0"
        continue
    assert (name.lower().replace("_", "-"), version) in locked, f"lockと環境の版が不一致: {package}"
(DEST / "packages.txt").write_text("\n".join(packages) + "\n")
env = os.environ.copy()
env.update(PYTHONPATH=str(SOURCE), PYTHONDONTWRITEBYTECODE="1", PYTHONHASHSEED="0", TZ="Asia/Tokyo")
with tempfile.TemporaryDirectory(prefix="jumeaux-unit-") as directory:
    # 旧テストはcwdにtmp/responsesを書く。テストとデータのコピーだけを実行する。
    shutil.copytree(SOURCE / "tests", Path(directory) / "tests", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    result = subprocess.run([sys.executable, "-B", "-m", "pytest", "tests", "-q", "-p", "no:cacheprovider"], cwd=directory, env=env, capture_output=True)
for stream in ("stdout", "stderr"):
    (DEST / f"unit-tests.{stream}").write_bytes(getattr(result, stream))
(DEST / "unit-tests.exit-code").write_text(str(result.returncode) + "\n")
(DEST / "README.md").write_text(f"""# Python版の固定基準

- コミット: `{commit}`。タグ: `v7.0.0`。
- Python: `{platform.python_version()}`。
- OS: `{platform.platform()}`。
- lock SHA-256: `{hashlib.sha256(lock).hexdigest()}`。
- 依存版の実測: `packages.txt`。依存解決の入力: `uv.lock`。インストール済み依存パッケージの版がlockにあることを検証済み。
  - 差異: lock内のルートパッケージjumeauxの版は6.1.9のまま。ソースとインストール済みメタデータは7.0.0。元ファイルは変更しない。
- 実行環境: 移行元 `.venv`。`TZ=Asia/Tokyo`、`PYTHONHASHSEED=0`。
- コマンド: `jumeaux init <template>`、`jumeaux run requests`。
  - 入口: `compat/harness/python-jumeaux` がPython版main.pyを直接起動する。
- 既存ユニットテスト: testsを一時ディレクトリへコピーし、`python -B -m pytest tests -q -p no:cacheprovider`。
  - 結果: 終了コード `{result.returncode}`。全文は `unit-tests.stdout` と `unit-tests.stderr`。

## 保存内容

各ケースにはinit/runのstdout・stderr・終了コード、初期化入力、API・通知のJSONLを保存する。
成功ケースにはreport.jsonとresponsesの保存ファイル一式、latestシンボリックリンクも保存する。

空ディレクトリの `.gitkeep` はGit保存用のマーカー。実行出力の一部には数えない。

- `encoding_hell` はPython版で `Sorter.conditions` がUnknownPropertiesErrorになり終了コード1。
  - 結果: reportもresponsesも存在しない。`artifacts.json` に不在を明示する。架空のreportは作らない。
- `query_custom` の2つの日時queryは `comparison.json` の固定値へ置換する。
  - 理由: 日付が変わっても送信URLを完全比較するため。元configは `init-config.yml` に保存する。
  - 検証: `$DATETIME` 自体は固定時刻の関数fixtureで別途検証する。
- モックはPython版 `MyServerHandler` を継承する。
  - 差異: Date・Last-Modified・Serverのみ固定応答にする。GET/POSTの処理は元実装を使う。
- `notifier` は同梱設定のままでは通知しない。
  - 追加ケース: `notify_v1`、`notify_v2`、`notify_v2_blocks` はnotifyを有効にし、localhost:8001へ送る。
- `retry_path_empty` はrun後のreportを入力にretryし、元reportと両出力ディレクトリを保存する。
- Python版の既存不具合は `compat/fixtures/retry.json` と `diff/cases.json` の修正後期待値で区別する。

## 再生成

リポジトリルートで実行する。既存の基準への上書きはハーネスが拒否する。

```sh
JUMEAUX_BIN="$PWD/compat/harness/python-jumeaux" uv run --project compat/harness python compat/harness/run.py --record --baseline /tmp/jumeaux-new-baseline
PYTHONDONTWRITEBYTECODE=1 "$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/environment.py
```

`JUMEAUX_PYTHON_ROOT` の既定は `~/git/github.com/tadashi-aikawa/jumeaux`。
移行元の作業ツリーは変更しない。実際のSlack・外部サービスへの送信は行わない。
""", encoding="utf-8")
print(f"Python {platform.python_version()}, {len(packages)} packages, pytest exit {result.returncode}")
raise SystemExit(result.returncode)
