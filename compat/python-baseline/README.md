# Python版の固定基準

- コミット: `dc276b8e3a0486bd8d4de2c8e3900aa36cbb7a66`。タグ: `v7.0.0`。
- Python: `3.15.2`。
- OS: `macOS-26.7.2-arm64-arm-64bit-Mach-O`。
- lock SHA-256: `a0c2d1d2bf6dda8ba34de2e595aa02abd5aade7e6debe38faf7572fb219c1128`。
- 依存版の実測: `packages.txt`。依存解決の入力: `uv.lock`。インストール済み依存パッケージの版がlockにあることを検証済み。
  - 差異: lock内のルートパッケージjumeauxの版は6.1.9のまま。ソースとインストール済みメタデータは7.0.0。元ファイルは変更しない。
- 実行環境: 移行元 `.venv`。`TZ=Asia/Tokyo`、`PYTHONHASHSEED=0`、`PYTHONWARNINGS=ignore`。
- コマンド: `jumeaux init <template>`、`jumeaux run requests`。
  - 入口: `compat/harness/python-jumeaux` がPython版main.pyを直接起動する。
- 既存ユニットテスト: testsを一時ディレクトリへコピーし、`python -B -m pytest tests -q -p no:cacheprovider`。
  - 結果: 終了コード `0`。全文は `unit-tests.stdout` と `unit-tests.stderr`。

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
- 追加19ケースを含む35ケースを保存する。ケース一覧は `compat/harness/run.py` のEXTRASとCLI_CASES。
  - 内容: 通知、retry、threads/processes、help、initエラー、verbose、encoding、ヘッダの大小。
- `encoding_fixed` はjson_sort設定とCSVのGET欠落を補った変種。元テンプレートの基準は変更しない。
- `header_ignore_case` のPython観測値はdifferent。Rust版の修正後期待値は `header_ignore_control` のsameを使う。
- Last-Modifiedはone/otherで異なる固定値を使う。
- Pythonラッパーはowlmixinのimport後に警告抑制を再設定する。
  - 理由: owlmixinのsimplefilterがPYTHONWARNINGS=ignoreを上書きするため。
- Python版の既存不具合は `compat/fixtures/retry.json` と `diff/cases.json` の修正後期待値で区別する。

## 再生成

リポジトリルートで実行する。既存の基準への上書きはハーネスが拒否する。

```sh
JUMEAUX_BIN="$PWD/compat/harness/python-jumeaux" uv run --project compat/harness python compat/harness/run.py --record --baseline /tmp/jumeaux-new-baseline
PYTHONDONTWRITEBYTECODE=1 "$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/environment.py --output /tmp/jumeaux-new-baseline
```

`JUMEAUX_PYTHON_ROOT` の既定は `~/git/github.com/tadashi-aikawa/jumeaux`。
移行元の作業ツリーは変更しない。実際のSlack・外部サービスへの送信は行わない。
