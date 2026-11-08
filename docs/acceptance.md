# Jumeaux Rust版 v0.1.0 全体受入

検証日時: 2026-11-08T16:43 JST。
受入開始時のRust版は `2163275` 、Python版7.0.0は `dc276b8` 。

機械的な受入は成功した。Viewerの目視は未実施で、タダシの担当とする。
リリース・push・実AWSへの接続は実施していない。

## 環境

| 項目 | 実測 |
| --- | --- |
| ホスト | macOS、Darwin 25.6.0、arm64 |
| Rust | rustc 1.99.1 |
| Python基準 | 既存 `.venv` のPython 3.15.2、ソース `dc276b8` |
| uv | 0.13.6 |
| Docker | クライアント29.9.3、Engine 28.4.0、Colima linux/arm64 |
| Linux | AlmaLinux 8、glibc 2.28、linux/amd64とlinux/arm64 |
| クロスビルド | cargo-zigbuild 0.24.1、Zig 0.15.2、両ターゲットともglibc 2.28指定 |
| feature | 配布と同じ既定feature。miroirを含む |

LinuxコンテナはPythonを追加せずに実行した。
コンテナのネットワークは `--network none` とし、ループバックの同梱serverだけを使った。
x86_64の受入はarm64のColima VM上でのコンテナ実行である。

## 機械的な受入結果

| 検証 | 成功 | 失敗 | 補足 |
| --- | ---: | ---: | --- |
| `cargo build --release` | 1 | 0 | macOS arm64、配布feature |
| release CLIの比較ハーネス | 35/35ケース | 0 | reportの型、保存bytes、実送信、CLIログを比較 |
| 比較ハーネスの自己検証 | 43テスト | 0 | 比較規則のmutationテストを含む |
| `cargo test` | 173テスト | 0 | doctest 3件を含む。ignored 2件は別実行 |
| `cargo test -- --ignored` | 2テスト | 0 | challengeの24ケース・47 Trial・102ファイル、exec雛形3種・3 Trial・12ファイル・6送信 |
| `cargo test --no-default-features` | 170テスト | 0 | doctest 3件を含む。ignored 2件は別実行 |
| 旧Python reportのretry | 8/8シナリオ | 0 | 6種類のreport、警告、exec許可 |
| Linux x86_64の同梱テンプレート | 16/16 | 0 | 各テンプレートでinit → server → run → retry |
| Linux x86_64とmacOSの成果物比較 | 32/32レポート | 0 | run/retry各32 Trial、保存本文・prop・Viewerのbytes一致 |
| Linux arm64のsimple | 1/1 | 0 | init → server → run → retry |
| Linux arm64とmacOSの成果物比較 | 2/2レポート | 0 | run/retry各2 Trial、保存本文・prop・Viewerのbytes一致 |
| `cargo fmt --check` | 成功 | 0 | |
| `cargo clippy --all-targets -- -D warnings` | 成功 | 0 | |
| `uv run zensical build --clean --strict` | 成功 | 0 | 利用者ドキュメント |

`cargo test` の件数はRustのテスト関数単位である。
1関数内で複数fixtureを照合するテストを含む。

最終のOS間比較成果物は `target/acceptance-delivery/` に保存した。
実行ログは `/tmp/jumeaux-acceptance-*-final.log` に保存した。
ハーネスの比較ログには、保存済み成果物ディレクトリのパスも出力している。

### 検出した問題と対応

- `--no-default-features` の初回実行で、ViewerのE2Eが `Address already in use` になった。
  - 原因: テストで空きポートを確保・解放してから子サーバーを起動する間に、別のテストが同じポートを使用できた。
  - 対応: `tests/server_cli.rs` をポート0で起動し、起動ログから割り当て済みポートを取得するように変更した。
  - 結果: 単独再実行と全件再実行が成功した。
- 外部コマンドのエラーに表示する移行ガイドURLが、サイトの実際の出力パスと異なっていた。
  - 対応: `/addons/exec/` を `/ja/addons/exec/` へ修正し、案内先ページを追加した。
  - 検証: サイトのstrictビルドでページ生成を確認し、CLIテストのURL期待値を更新した。
- sandbox内の初回 `cargo test` はローカルソケットのbind権限で停止した。
  - 対応: ローカル受信先を利用できる実行環境で全件を再実行した。
- 途中の比較ハーネスで、同じポート8000を使うmacOS検証の送信が混入した。
  - 結果: 4ケースで送信件数が不一致になった。
  - 対応: 2つの検証を直列で再実行した。最終結果は35/35ケース一致。
- feature違いのcargo testを並行させた実行で、CLIバイナリが入れ替わった。
  - 結果: featureなしのMiroir拒否テストが失敗した。
  - 対応: 同じtargetを使うcargo testを直列化して再実行した。
- Linux比較では、実行時刻に依存するHTTP Dateを検出した。
  - 比較規則: `trials[*].one/other.response_header.date` だけを対象に、HTTP日時としての妥当性と実行時間内であることを検証して正規化した。
  - 保存本文: 同名のJSONキーや保存bytesは正規化しない。

### バイナリのSHA-256

```text
macOS arm64     a0408fb609c0002c5bd8b4c00460516e4e57acb636a66f5746269b349fa09625
Linux x86_64    d41f446d1b5f68ed3d65ea9499eb039bd2a9855ac6ab77925f16a33dc88d8b14
Linux arm64     8decee8be76b82bfaaf085bd6f92f0ae0ad81dbaa7e9bafa81b1121ebc6437e2
```

## 旧Python reportからのretry

入力は `compat/python-baseline/<case>/responses/<key>/report.json` 。
`summary.comparison` がないことを検査してから、受信先のポートだけをローカルの動的ポートへ変更した。

| ケース | 確認した結果 |
| --- | --- |
| `simple` | 送信、新report、保存bytesがPython retryと一致 |
| `path_empty` | 空pathを維持し、送信と成果物が一致 |
| `notifier` | notifiersを維持し、送信と成果物が一致 |
| `request_headers` | 明示ヘッダと上書きを維持し、送信と成果物が一致 |
| `judge_response_headers` | 比較設定をPython既定値で補完し、Python retryと一致 |
| `post` | formとJSONは一致。rawはRustだけが保持し、one/otherへ `a=100&b=200` を2回送信 |
| 未知フィールド・旧logger | comparison補完とフィールド破棄の両系統の警告を表示。新reportから未知キーとloggerを除去 |
| execを含む旧report | 許可なしはvalidate前に拒否。許可付きは成功。新reportへ許可を保存せず、次のretryでは再度拒否 |

全6種類でcomparison欠落の警告と、次の保存値を確認した。

```json
{
  "judge_response_header": false,
  "ignore_response_header_keys": ["Content-Length", "Date"]
}
```

`post` のPython版はrawを喪失する。
この差異は互換仕様の修正対象どおりである。
rawを除いたRustの対照実行では、Python版と送信・新report・保存bytesが完全一致した。

report比較ではversion、key、日時、経過時間だけを既存規則で正規化する。
`retry_hash` と元reportのkey、latestのリンク、ファイル参照も検証する。
Rustが追加する `summary.comparison` は期待値を明示して比較する。

## Python版E2E 23件との対応

比較対象はPython版 `e2e/main.py` 。発注文にあった `tests/e2e/` は存在しない。
テストメソッドは23件で、未対応は0件だった。
元の23件を埋めるためのE2E追加は不要だった。
旧reportのretry受入8シナリオと、OS間比較のハーネスを追加した。

下表のケース名は `compat/harness/run.py` のケースを指す。
同ハーネスはCLIの終了コードに加え、全report、保存bytes、実送信、生成物の有無を比較する。

| Python版のテスト | 確認すること | Rust版の対応 |
| --- | --- | --- |
| `TestHelp.test_usage` | helpが終了コード0 | `help`、`tests/cli.rs` |
| `TestInit.test_with_no_args` | 引数なしinitは1、生成物なし | `init_no_args` |
| `TestInit.test_with_invalid_args` | 未知のテンプレートは1、生成物なし | `init_invalid` |
| `TestInit.test_simple` | simpleのapi・requests・configを生成 | `simple` のinit段階 |
| `TestRun.test_simple` | GET、same 1・different 1、notifiersなし、成果物 | `simple` |
| `TestRun.test_path_custom` | 両側のpath変換とdifferent 2 | `path_custom` |
| `TestRun.test_query_custom` | queryの追加・削除・日時と実送信URL | `query_custom`、日時は入力固定 |
| `TestRun.test_all_same` | same 1、本文・propを保存しない | `all_same` |
| `TestRun.test_xml` | XML差分、different 1、価格タグ | `xml` |
| `TestRun.test_html` | HTML差分、different 1、成果物 | `html` |
| `TestRun.test_ignore_order` | 順序無視、same 1・different 2 | `ignore_order` |
| `TestRun.test_ignore` | same 2・different 2、分類済み差分パス | `ignore` |
| `TestRun.test_force_json` | plainとJSONのtype判定 | `force_json` |
| `TestRun.test_request_headers` | 設定とtrialごとのヘッダ上書き | `request_headers` |
| `TestRun.test_judge_response_headers` | ヘッダ比較、different 1、ヘッダ保存 | `judge_response_headers` |
| `TestRun.test_with_log_level_options` | -v・-vv・-vvvの実行と保存 | `verbose_1`・`verbose_2`・`verbose_3` |
| `TestRun.test_with_threads` | threads 2・processes 1 | `threads_2` |
| `TestRun.test_with_processes` | threads 1・processes 2 | `processes_2`、RustはmacOSでも実行 |
| `TestRun.test_root_array` | 配列root、same 1・different 1 | `root_array` |
| `TestRun.test_notifier` | Slack種別・channel・iconの保存 | `notifier` |
| `TestRun.test_post` | POSTのform・JSON・raw、same 1・different 2 | `post` |
| `TestRetry.test_with_empty_path` | 空pathのreportからretry、different 1 | `retry_path_empty`、追加の旧report受入 |
| `TestRetry.test_with_notifiers` | retryでnotifiersの種別・channel・iconを保持 | `retry_notifier`、追加の旧report受入 |

## v0.1.0でPython版から変わること

差異の単一情報源は [互換仕様](compat-spec.md#意図的に変える差異) と [許容した差異](compat-spec.md#許容した差異) 。
外部コマンドの契約は [exec仕様](exec-addon-spec.md) に従う。

### 設定を直す必要があるもの

| 対象 | 移行内容 |
| --- | --- |
| Pythonカスタムアドオン | `name: exec`、`config.command`、`config.options` へ移行 |
| Python transformer | module・functionをtransformer.command・optionsへ移行 |
| `cls_name` | Executor以外は使用不可。処理の選択はcommandで指定 |
| 旧loggerと未知キー | 非nullのoutput.loggerと未知キーを削除。旧reportは警告して破棄 |
| 空log2reqs | `log2reqs: {}` を削除するか必須nameを指定 |
| 入れ子include | 宣言元から1段で解決できる形へ整理 |
| 型の不一致 | 整数欄へのboolを修正。YAML 1.1固有の010・1_000に依存しない |
| 認証 | .netrcの自動付与に依存せずAuthorizationを明示 |
| proxy | macOSのシステムproxyに依存せず設定か環境変数で指定 |
| テンプレート・正規表現・codec | サポート外のfilter・test・Pythonメソッド・構文・文字コードを置き換える |

### 挙動が変わるもの

| 対象 | 利用者への影響 |
| --- | --- |
| インストール | macOS・Linuxのarm64・x86_64用バイナリ。組み込み機能はPython不要 |
| 外部コマンド | JSONとBase64で通信し、呼び出しごとにプロセス起動。validate、上限、タイムアウトを適用 |
| `init addon` | 外部コマンド3雛形を生成。雛形の実行にはPython 3が必要 |
| execを含むretry | 毎回--allow-execを指定。許可前はvalidateも実行しない |
| retryの本文と比較 | rawとヘッダ比較設定を維持。旧reportは警告してPython既定値を補完 |
| ヘッダ名 | 除外設定と条件式で大小を無視 |
| JSON差分パス | キーのroot・ブラケット・引用符などを正しく扱う |
| `encoding_hell` | 廃止済み条件と不足GET列を修正し、同梱例が成功する |
| trialの例外 | 新規着手を止め、実行中trialの完了を待つ。エラー時の送信・保存件数に差が出る |
| JSON境界 | 非有限数・lone surrogateを拒否。配列・辞書は1,000段まで |
| YAML入力 | yes・数値風文字列・日付・タグ・キーの解釈が異なる。範囲外整数はf64へ丸める |
| テンプレート | Optionのis_someを追加。未定義値・異種比較・日時・repr・一部filter・メソッドに差異 |
| 大整数 | JSON整数の保存精度は維持。テンプレート入力のi128超、YAMLの範囲外整数は丸め得る |
| 正規表現 | 一部構文の受理・拒否、空白クラスに差異 |
| 文字コード | 推定名と対応codec、不正bytes、EUC-JPなどの一部写像に差異 |
| HTML・XML | HTML5の木構造、DTD・エンティティ・dump・深さ制限に差異 |
| HTTP | gzipとidentity併記、不正charsetの一部を受理。ループバックHTTPへSecure Cookieを送る場合がある |
| ignore・CSV・保存 | 後続caseの評価、複数エラーの検出順、tupleヘッダの失敗箇所に差異 |
| CLI | versionはjumeaux 0.1.0。help・-Vを追加。init一覧はソート順 |
| 通知 | User-Agentはjumeaux/<version>。Slack v1もWebhook未設定時は警告してスキップ |
| エラー | 保存I/Oの汎用名はIOError。Python tracebackは表示しない |
| Miroir | 配布featureでは有効。--no-default-featuresでは設定エラー |

### 内部の差

- `--processes` は並列数として扱い、threadsより優先する。
  - 出力: concurrencyの互換形式を維持する。
- モデル内部のJSON number表記、時間の指数表記はPythonと異なる場合がある。
  - 保存: reportの数値表現とJSON整数精度は互換アダプターで維持する。
- `did_challenge/sleep` のmin・maxは整数も受理する。
- 外部コマンドの動作保証はmacOS・Linuxとする。

### 案内のずれの修正

| 修正先 | 修正内容 |
| --- | --- |
| 移行ページ | 外部コマンド・init addonの準備中を実装済みへ更新。許可とPython製雛形の条件、許容差異を追記 |
| v0.1.0リリースノート | exec・transformer・雛形、retry修正、設定検証と移行時の注意を反映 |
| custom・res2res・Addonsトップ | 同じ準備中を除去。設定例と移行手順を反映 |
| execページとナビゲーション | `/ja/addons/exec/` を追加し、CLIの案内URLも修正 |
| ドキュメント開発手順 | 外部コマンドを準備中とする記述を更新 |
| README | 外部コマンドの利用可否、雛形、retryの明示許可を更新 |
| exec仕様と雛形README | 移行ガイドURLを実際のサイト出力パスへ統一 |

互換仕様の「意図的に変える差異」「許容した差異」は現実装と整合していた。
差異の承認内容は変更していない。

## 再実行

リポジトリルートで実行する。

```sh
cargo build --release
JUMEAUX_BIN="$PWD/target/release/jumeaux" uv run --project compat/harness python compat/harness/run.py
cargo test
cargo test -- --ignored --nocapture
cargo test --no-default-features
JUMEAUX_BIN="$PWD/target/release/jumeaux" uv run --project compat/harness python compat/harness/retry_acceptance.py
```

Linuxビルドは [リリース手順](releasing.md#linuxのクロスビルド) と同じバージョンとglibc指定を使う。

```sh
uv tool install cargo-zigbuild==0.24.1 --with ziglang==0.15.2
export PATH="$(uv tool dir --bin):$PATH"
export CARGO_ZIGBUILD_PYTHON_PATH="$(uv tool dir)/cargo-zigbuild/bin/python"
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
cargo zigbuild --locked --release --target x86_64-unknown-linux-gnu.2.28 --target aarch64-unknown-linux-gnu.2.28 --bin jumeaux
mkdir -p target/acceptance/linux-amd64 target/acceptance/linux-arm64
bash compat/harness/container_acceptance.sh "$PWD/target/release/jumeaux" "$PWD/target/acceptance/macos"
docker pull --platform linux/amd64 almalinux:8
docker run --rm --network none --platform linux/amd64 \
  -e JUMEAUX_REQUIRE_NO_PYTHON=1 -v "$PWD:/source:ro" \
  -v "$PWD/target/acceptance/linux-amd64:/results" almalinux:8 \
  bash /source/compat/harness/container_acceptance.sh /source/target/x86_64-unknown-linux-gnu/release/jumeaux /results
docker pull --platform linux/arm64 almalinux:8
docker run --rm --network none --platform linux/arm64 \
  -e JUMEAUX_REQUIRE_NO_PYTHON=1 -v "$PWD:/source:ro" \
  -v "$PWD/target/acceptance/linux-arm64:/results" almalinux:8 \
  bash /source/compat/harness/container_acceptance.sh /source/target/aarch64-unknown-linux-gnu/release/jumeaux /results simple
uv run --project compat/harness python compat/harness/compare_platforms.py target/acceptance/macos target/acceptance/linux-amd64
uv run --project compat/harness python compat/harness/compare_platforms.py target/acceptance/macos target/acceptance/linux-arm64
```

既存の結果ディレクトリを再利用しないこと。
受信先のポート8000が空いている状態で実行する。
ファイルmtimeは2000-01-01 UTCへ固定する。
比較は既存のreport正規化と、前述のHTTP Date検証だけを使用する。

## Viewerの目視手順

担当はタダシ。機械試験では静的配信、report変更検出、latest差し替えを確認済み。
ブラウザでの表示確認は未実施。

リポジトリルートからターミナル1で実行する。

```sh
export JUMEAUX_BIN="$PWD/target/release/jumeaux"
demo_dir=$(mktemp -d /tmp/jumeaux-viewer-acceptance.XXXXXX)
cd "$demo_dir"
"$JUMEAUX_BIN" init simple
"$JUMEAUX_BIN" server > server.log 2>&1 &
api_pid=$!
until curl --silent --fail http://127.0.0.1:8000/api/one/same-1.json > /dev/null; do
  kill -0 "$api_pid" || exit 1
  sleep 0.1
done
"$JUMEAUX_BIN" run requests
printf '目視用ディレクトリ: %s\n' "$PWD"
"$JUMEAUX_BIN" viewer
```

ブラウザで `http://127.0.0.1:5500/` が開く。
ViewerはVue・Monacoなどを外部CDNから読み込む。
目視時はCDNへ接続できるブラウザが必要。

| 観点 | 確認内容 |
| --- | --- |
| 一覧 | Simpleのタイトル、Same 1・Different 1・Failure 0、フィルタ切り替え |
| 差分表示 | Differentの行を選ぶと、oneとotherの保存JSONが左右に表示される |
| 同一レスポンス | Sameは保存条件により本文がない。保存ファイルの存在と区別できる |
| 自動リロード | reportのタイトル変更が手動更新なしで反映される |
| latest差し替え | 同じディレクトリで再runすると、新しいreportを表示する |

ターミナル2で、表示された目視用ディレクトリへ移動する。

```sh
cd <目視用ディレクトリ>
jq '.title = "自動リロード確認"' responses/latest/report.json > updated-report.json
mv updated-report.json responses/latest/report.json
```

タイトルが自動で変わることを確認する。
続けてターミナル2でreleaseバイナリの絶対パスを使って `run requests` を実行し、latest差し替えも確認する。

終了時はターミナル1のviewerをCtrl-Cで止め、同じターミナルでAPIサーバーを停止する。

```sh
kill "$api_pid"
wait "$api_pid"
```

## 残件

- タダシによるViewerの目視受入。
- リリースするかの判断と、push・タグ作成・公開作業。
