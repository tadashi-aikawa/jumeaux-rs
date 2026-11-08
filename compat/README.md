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
│   ├── config/          マージ・includeの入力YAML
│   ├── config-models.json 設定・モデル・出力ストリームのPython期待値
│   ├── review-fixes.json null・YAMLマージ・report読込・34アドオン設定の回帰fixture
│   ├── review-round2.json 重複キー・マージ優先順位・null logger・report走査の回帰fixture
│   ├── response-judgement.json 応答変換・判定の8アドオンのPython期待値
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

### challenge・保存の受入

```sh
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/challenge.py > /tmp/jumeaux-challenge.json
diff compat/fixtures/challenge.json /tmp/jumeaux-challenge.json
cargo test --test challenge -- --nocapture
cargo test --test challenge_e2e -- --ignored --nocapture
```

- 関数・コンポーネントfixtureは104件。Python完全一致102件、承認済み例外種別の差異2件。
- E2Eは記録用受信先を動的なローカルportで起動する。
  - 対象: 22ケース、Trial 43件、本文・prop 94ファイル。
  - 比較: 既存python-baselineのtrialと保存bytes。Report書き出し・final・CLIは後続タスク。
  - processes: Cookie分離案のPerTrialを明示して検証する。公開APIの既定契約は未決。
- Python基準の.venvが必要なE2Eだけ通常のcargo testではignoredとする。
  - 実行: 上記の`--ignored`付きコマンドで明示的に動かす。

### HTTP転送の受入

```sh
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_transport.py > /tmp/jumeaux-http-transport.json
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_transport.py --encoding > /tmp/jumeaux-http-encoding.json
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_transport.py --prepare > /tmp/jumeaux-http-request.json
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_transport.py --time > /tmp/jumeaux-http-time.json
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_review2.py > /tmp/jumeaux-http-review2.json
diff compat/fixtures/http-transport.json /tmp/jumeaux-http-transport.json
diff compat/fixtures/encoding/http.json /tmp/jumeaux-http-encoding.json
diff compat/fixtures/http-request.json /tmp/jumeaux-http-request.json
diff compat/fixtures/http-time.json /tmp/jumeaux-http-time.json
diff compat/fixtures/http-review2.json /tmp/jumeaux-http-review2.json
cargo test --test http --test http_encoding -- --nocapture
cargo build --example http_probe
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/http_review2.py --rust target/debug/examples/http_probe
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/harness/http_acceptance.py
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B -m unittest discover -s compat/harness -p test_compare.py
```

- 転送fixtureは62件のPython実測。動的ポートのループバック受信先だけを使う。
  - 対象: 本文bytes、自動・明示ヘッダ、redirect、Cookieの順序、gzip・zstd、retry、通信失敗とchallengeの捕捉。
  - 注入: DNS失敗・接続拒否・connect timeoutはソケット入口で注入し、外部へ通信しない。
  - 比較: Rustの送信・受信結果と保存済みPython観測値を照合する。timeoutの0.2秒は実測用であり、製品の設定ではない。
- リクエスト組み立ては18件すべてRustとPython期待値を比較する。
  - 対象: path・query・ヘッダのマージ、URL文字コード、本文の優先順位・JSON・formの表記。
- encodingは23件すべてRustでPython期待値と比較する。
  - 対象: 決定順、空本文、charsetの引用符・重複・空値、MIME typeとパラメータの区別。
- request_timeはPythonのisoformatから採取した4件でオフセットと小数部を検証する。
- `http-review2.json`はCAの5件と通知の14件を検証する。
  - 比較: 19件すべてをPython観測値と照合する。存在しないCAはOSError、読取権限のないCAはSSLError、通知は200だけを成功とする。
  - 通知: 本文bytesと自動ヘッダを比較する。User-Agentは比較から除外し、Rustが`jumeaux/<version>`を送ることを別途検証する。
- `http_acceptance.py`は既存31ケースのapi.jsonlの124リクエストも比較する。
  - 入力: 基準Pythonの入力アドオンで生成したRequest。転送はRustのone/other並行送信。
  - ヘッダ: 共通のcompare.pyでtrialごとの明示値を識別する。自動ヘッダも比較し、User-Agentはバージョン部分だけ正規化する。
  - proxy・TLS: 26ケース。OpenSSLで一時CAと証明書を生成する。EKUなしの10年・1年証明書による転送・通知も含む。外部のDNS失敗はresolver注入で検証し、外部へ通信しない。
  - 実行: `--phase transport|baseline|connection`で対象を限定できる。Python版の.venvとopensslコマンドが必要。
  - 制限: 製品CLIを通した保存成果物・アドオンのE2Eは全体統合の対象。

### 入力系アドオンと通知

入力系アドオンと通知の基準は次で追加採取する。

```sh
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/inputs.py
cargo test --test inputs -- --nocapture
```

- `compat/fixtures/inputs.json`にPython版の観測値を保存する。
  - 通知: 127.0.0.1の空きポートを使う。実際のSlackには接続しない。
  - 対象: 4形式の入力、8種のreqs2reqs、Slack v1・v2の本文bytesとContent-Type。
  - shuffle: countからpathが`/0`～`/(count-1)`のRequestを生成し、並び替えたpath配列を比較する。1000件のケースでMT19937の状態更新も検証する。
  - CLI: empty_guardで通知して終了コード1になる経路も別テストで検証する。

### 既存の関数fixture

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
- retryのヘッダ判定設定は `summary.comparison` に保存する。
  - 旧report: キーがなければ警告し、Pythonの既定値 `false`・`["Content-Length", "Date"]` を補う。
  - fixture: `original_config` は元設定を明示するAPIの入力契約を示す。保存済みcomparisonと旧reportの補完は `tests/review_fixes.rs` で別途検証する。
- 既存テストからテンプレート式、正規表現、encoding決定、HTML/XMLの入力を取り込む。
- Rustの互換モジュールは`cargo test --test compat_template --test compat_regex --test compat_encoding -- --nocapture`で検証する。
  - 件数: template 216、regex 82、encoding 267。既存ケースの期待値は保持し、不足した境界ケースを追加した。
  - 許容した差異: template 23件、regex 15件、encoding 28件を明示検証する。encodingの`detect-euc`もPython期待値とRust結果を両方assertする。
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
- 許容した差異の42件も実行し、`tests/markup_differences.json`のRust現出力との一致を検証する。
  - 規則: Python期待値は変更しない。差異の重複・未使用・理由欠落・解消済みの差異はテストで検出する。
  - 詳細: [互換仕様の許容した差異](../docs/compat-spec.md#許容した差異)。
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

### 応答変換・judgementの先行検証

```sh
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/response_judgement.py > compat/fixtures/response-judgement.json
cargo test --test response_judgement -- --nocapture
```

- 関数fixtureは279件。
  - Python一致: 229件。
  - 承認済み差異: 特殊パスの値取得修正7件、非有限数41件、lone surrogate 2件。Python観測値と`rust_expected`を両方保持する。
  - 未決: 0件。
  - 境界: Optionのvalue、整数の負のゼロ、深さ129・200の読み込みとソートもPythonと比較する。非有限数はjson_sort・res2dict/json・when・diffを同じ入力で比較する。
- `cargo test --test json_boundary`で深さ1,000と1,001を検証する。
  - 実行: 子プロセス内の2MiBスタックから、配列・辞書・混在JSONの読み込み・ソート・diff・破棄を呼び出す。
  - エラー: 非有限数のエラー種別とメッセージを全入口で検証する。json_sortからres2dict/jsonへの連結も検証する。
- `cargo test --release --test judgement_performance -- --nocapture`で10万キーのdictに対するsameの条件式1・10・40本を計測する。
  - 回帰検証: 構築済みcontextの走査回数が条件式の本数で増えないことも固定する。
- 同梱テンプレート8種とヘッダの追加3ケースは、既存reportの19 trialと比較する。
  - 対象: ignore、ignore_order、judge_response_headers、force_json、xml、html、all_same、root_array。
  - 比較: API応答ファイルに各テンプレートのres2res・res2dict・judgementを順に適用し、diffs_by_cognitionとstatusを検証する。
  - 差異: header_ignore_caseのstatusは承認済みの大小無視により対照ケースのsameを使う。
  - 範囲: HTTP通信・保存・CLI全体のE2Eは含まない。

### 設定・モデル・出力の先行検証

```sh
cargo test --test config_models -- --nocapture
# Python版から期待値を再採取する。既存fixtureと比較する場合は一時ファイルを使う。
CONFIG_MODEL_FIXTURE="$(mktemp)"
"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/config_models.py > "$CONFIG_MODEL_FIXTURE"
diff compat/fixtures/config-models.json "$CONFIG_MODEL_FIXTURE"
```

- `config-models.json` は32件。
  - Python一致: 設定6件、モデル12件、TextIOWrapperの出力12件。
  - 許容した差異: RequestとReportの数値表記2件。serde_jsonで正規化した表記を使い、Python期待値との差を明示検証する。
- 基準report31件はJSON値とbytesの往復を検証する。
  - 対象: テンプレート15種と追加ケース。可変値も除外しない。
  - 例外: encoding_hellは終了コード1とreport不存在を確認する。既存基準を作り直さない。
- 基準config31件の読み込みと登録を検証する。
  - 例外: encoding_hellは廃止済みの`addons.res2res[0].config.items[0].conditions`をエラーにする。
- 既存retry fixtureはrawの復元とヘッダ判定設定の明示的な受け渡しを検証する。
  - 入力: ヘッダ判定設定は`original_config`から供給する。
- `review-fixes.json` は186件。
  - Python一致: 180件。null既定値・YAMLマージキー・後続レイヤーの上書き・report未知キー・Request本文の型・34アドオンの型付き設定・プロセス出力を検証する。
  - 意図的な差異: 6件。設定の旧logger拒否、reportの旧logger破棄、Python transformer拒否2件、shuffle・viewerの未知設定拒否。
  - エラー: Python例外とRustのデシリアライズエラーを必須欠落・型違反・未知キーで対応付ける。
  - 実行: `cargo test --test review_fixes -- --nocapture`。
  - 再採取: `"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/review_fixes.py` を一時ファイルへ出力して比較する。
- `review-round2.json` は10件。
  - Python一致: 9件。重複キーとマージの優先順位、null logger、reportの既知キー保持・未知キー除去を検証する。
  - 意図的な差異: 1件。非nullのlogger設定を拒否する。
  - 性能: 8,000 trialへ展開するfixtureで、未知キー除去とモデル生成が1秒未満であることを確認する。JSON解析時間も別途表示する。
  - 並列性: 出力ハンドルの再取得・別encodingの併用・16スレッドの書き込みを検証する。ライブラリが警告を直接出力しないことも確認する。
  - 実行: `cargo test --test review_round2 -- --nocapture`。
  - 再採取: `"$JUMEAUX_PYTHON_ROOT/.venv/bin/python" -B compat/gen/review_round2.py` を一時ファイルへ出力して比較する。
- CLIのrun/retryは引き続きスケルトン。
  - 残件: HTTP・各アドオン本体・CLI接続後にE2Eハーネスを実行する。

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
