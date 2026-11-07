# 互換テスト

契約は [互換仕様](../docs/compat-spec.md) に記す。
以下のコマンドはリポジトリルートで実行する。

```text
compat/
├── python-baseline/     16テンプレート＋追加19ケースの実測
├── fixtures/
│   ├── diff/            DeepDiff、差分パスの値取得
│   ├── markup/          HTML・XMLのdict化とdump整形
│   ├── template/        条件式、文字列描画、日時、Responseプロパティ
│   ├── regex/           完全一致、置換、後方参照、lookahead
│   ├── encoding/        決定順、別名、BOM、変換失敗、入力、URL
│   └── retry.json       再構築の既存不具合と修正後の意味上の契約
├── gen/                 Python版からのfixture・環境情報生成
└── harness/             バイナリ差し替え、記録用受信先、比較規則
```

## E2E比較

Python版のソースと既存 `.venv` が必要。
APIモックは移行元の `jumeaux server` と同じハンドラを継承して使う。

```sh
export JUMEAUX_PYTHON_ROOT="$HOME/git/github.com/tadashi-aikawa/jumeaux"
JUMEAUX_BIN="$PWD/compat/harness/python-jumeaux" uv run --project compat/harness python compat/harness/run.py
```

Rust版の実装後は次で同じ比較を行う。

```sh
cargo build
uv run --project compat/harness python compat/harness/run.py
```

- `JUMEAUX_BIN` は単一の実行ファイルパス。引数を含むコマンド文字列は受け付けない。
- `--case simple` のように対象を限定できる。
- `--run-arg=--threads --run-arg=2` のようにrunの引数を追加できる。
  - 条件: 比較時は同じ引数で採取した別の基準を `--baseline` で指定する。
- `--output /tmp/jumeaux-actual` で結果保存先を指定できる。
  - 既定: 一意の一時ディレクトリ。失敗時も削除しない。
- 既存出力は上書きしない。
- APIは127.0.0.1:8000、Webhookは127.0.0.1:8001で待ち受ける。
  - 条件: ポートが使用中なら失敗する。他のプロセスは停止しない。
  - 実装: 起動前に接続確認し、`allow_reuse_address=False` で排他的にbindする。受信先は全ケースを通じて1回だけ起動する。
  - 待機: LISTENしていない終了直後のTIME_WAITは、各ポート最大65秒だけbindを再試行する。
- proxyとSlackの環境変数を子プロセス用環境から除去し、Webhookをlocalhostへ固定する。
- API記録はリクエストの多重集合を比較する。
  - 内容: メソッド、URL、本文bytes、同一リクエストの回数。
  - ヘッダ: 設定・requests由来とContent-Typeだけを比較する。名前の大小と順序を無視し、値を比較する。
  - 原記録: 自動ヘッダも含め、ヘッダの原表記と順序をJSONLへ保存する。
  - 条件: one/other間の到着順だけには依存しない。
- 作業ディレクトリ全体の保存ファイルはbyte単位、reportはJSONの型を含めて比較する。
- `comparison.json` の明示パスだけ可変値を扱う。
  - 検証: keyとlatest、retry_hashと元report、ファイル参照、非負時間、日時形式もチェックする。
- `PYTHONWARNINGS=ignore` を設定し、Pythonラッパーはowlmixinのimport後にも警告抑制を設定する。
  - 理由: owlmixinが環境変数によるignoreを上書きするため。
- 失敗時のtracebackは基準に残る。
  - 制限: encoding_hellのstderrはPython版の絶対パスとランタイム版に依存する。別環境やRust版のエラー表現の許容範囲は未決であり、暗黙に除外しない。

追加ケースはthreads/processesの優先順位、help、initエラー、3段階のverbose、notifierのretry、文字コード、ヘッダ名の大小を検証する。

- `encoding_fixed` はjson_sortとCSVを修正した変種。`output_encoding` はUTF-16で保存する。
- Rust版の同梱テンプレート `encoding_hell` は、json_sortの `conditions` の `path` を `when` へ直し、requestsにメソッド列 `GET` を補う。
- `header_ignore_case` は小文字の除外設定、`header_ignore_control` は対照用の正規表記。
  - 差異: Python版の小文字設定ではdifferentになる。Rust版の期待出力はsameとなる対照ケースを使う。入力configは小文字のまま比較する。
- `header_condition_case` は大小を混ぜた条件式でone/otherの異なるLast-Modifiedを検証する。

## 関数fixture

```sh
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/fixtures.py
# 既存fixtureを変更せず再現性を確かめる
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/fixtures.py --output /tmp/jumeaux-fixtures
diff -r compat/fixtures /tmp/jumeaux-fixtures
```

- 各ファイルは `{ "name": ..., "input": ..., "expected": ... }` の配列。
- expectedは成功時 `{"value": ...}`、失敗時 `{"error": "Python例外クラス名"}`。
  - 理由: 他言語の実装からは例外の種類を対応付ける。スタックやメモリアドレスを契約にしない。
- バイナリ入力は `body_hex`。変換後のバイナリ出力もhex文字列。
- `python_mismatch: true` のケースはPython観測値と修正後のexpectedを分ける。
- `rust_expected`があるケースはPythonの`expected`を保持し、Rust側の既知差異を別に固定する。
  - 規則: `difference`に互換仕様への参照と差異の説明を書く。生成時に両者の不一致を確認し、Rustテストは`rust_expected`との一致を検証する。
- retryのヘッダ判定設定は保存形式が未決。
  - 条件: `original_config` は意味上の入力契約を示す。旧reportだけから復元できることは主張しない。
- 既存テストからテンプレート式、正規表現、encoding決定、HTML/XMLの入力を取り込む。
- Rustの互換モジュールは`cargo test --test compat_template --test compat_regex --test compat_encoding -- --nocapture`で検証する。
  - 件数: template 216、regex 82、encoding 267。既存ケースの期待値は保持し、不足した境界ケースを追加した。
  - 差異: template 22件、regex 15件、encoding 28件を明示検証する。encodingの`detect-euc`は決定案Aの例としてPython期待値とRust結果を両方assertする。
  - 別名: Windows-31J・cswindows31jはPythonでもcp932へ解決される。別名をそのまま指定して採取し、`input.python_codec`に解決先を記載する。
  - HTML: `dump_html`の`decoded_text`・`formatted_text`はPythonで採取した中間値。ここでは文字コード境界を検証し、Rustの整形結果との接続はmarkup統合で検証する。
- 既存ユニットテスト一式は `gen/environment.py` で一時コピーを実行する。
  - 出力: `python-baseline/unit-tests.*`、版一覧、lock、README。
  - 再生成: `--output /tmp/jumeaux-new-baseline` を指定する。既存の環境情報があれば、既定先でも明示先でも上書きを拒否する。

### markupの先行検証

```sh
cargo test --test markup -- --nocapture
```

- `input.operation`が`pretty`のケースはdumpの整形文字列を比較する。省略時はdict化。
- 221件中179件はPython期待値と完全一致する。
- 未決の42件も実行し、`tests/markup_differences.json`のRust現出力との一致を検証する。
  - 規則: Python期待値は変更しない。差異の重複・未使用・理由欠落・解消済みの差異はテストで検出する。
  - 詳細: [互換仕様の未決](../docs/compat-spec.md#未決)。
- 条件式付き2件はdictの参照と比較を検証する。
  - 制限: 条件式エンジンの統合検証は後続タスクで行う。
- `input.summary: "expansion"`は属性と本文のエンティティ展開量を検証する。
  - 入力: 1万文字の内部エンティティを使い、`attributes`個の属性、`text`個の本文参照、`padding`文字のコメントを生成する。
  - 出力: dictは属性と本文の文字数、prettyはinvalid token判定と文字数。生成式は`gen/fixtures.py`と`tests/markup.rs`に置く。
- `input.summary: "depth"`は`depth`個のrまたはdivを入れ子にし、末端をxにする。
  - 出力: XML dictは深さと末端値、prettyはinvalid token判定と文字数。
- `input.serialize: true`はdictをJSON保存するところまでPythonで実行する。
  - 対象: plaintextの循環オブジェクトによるValueErrorを記録する。

## ハーネスの自己検証

### JSON差分エンジンの検証

`cargo test --test diff -- --nocapture` で差分fixture全件と特殊キーのパス往復を検証する。

配列の組み合わせを追加してPython版と比較する場合:

```sh
DIFF_FIXTURE_DIR="$(mktemp -d)"
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/fixtures.py --diff-matrix --output "$DIFF_FIXTURE_DIR"
JUMEAUX_DIFF_FIXTURES="$DIFF_FIXTURE_DIR/diff/cases.json" cargo test --test diff all_diff_fixtures -- --nocapture
```

- `--diff-matrix` はscalar配列1,600組と型の混在する配列5,329組を追加する。
- `JUMEAUX_DIFF_FIXTURES` は検証用のfixture差し替え。通常のテストはリポジトリのfixtureを使う。
- `JUMEAUX_DIFF_TIMINGS=1` と `--nocapture` を指定すると、各diffの処理時間を秒で表示する。
  - 計測範囲: JSONの読み込みと期待値との照合を除いた差分検出。性能比較は `--release` で実行する。

### E2E比較ハーネスの検証

```sh
uv run --project compat/harness python -m unittest discover -s compat/harness -p 'test_*.py'
```

本文の改変、通知の欠落・改変、リクエスト回数、保存ファイルの欠落、参照切れ、型の変更、同名キーの誤除外を検出する。
