# Python版からの移行

Rust版はPython版 `7.0.0` を基準にしています。

CLI、設定ファイル、リクエスト形式、レポート、Viewerの互換性を検証しています。
既存設定では、以下の差異を確認してください。

## インストールと拡張

- GitHub Releasesのバイナリを使います。Pythonは不要です。
    - 詳細: [インストール](quickstart.md#installation)
- Pythonモジュール方式のカスタムアドオンと `res2res/json` のtransformerは使用できません。
    - 移行先: 外部コマンド方式を準備中です。
    - 詳細: [カスタムアドオン](../addons/custom.md)
- `jumeaux init addon` は準備中の案内を表示し、終了コード1で停止します。
- `final/miroir` はRust版でも利用できます。
    - ビルド: Cargo feature `miroir` は既定で有効です。配布バイナリにも含まれます。
    - 無効時: `--no-default-features` のビルドでは設定がエラーになります。
    - 再有効化: `--features miroir` を付けてビルドしてください。
    - 詳細: [Miroirの認証と接続](../addons/final.md#miroir)

## 設定と認証

- 未知の設定キー、未知のアドオン名、`Executor` 以外の `cls_name` はエラーになります。
- `output.logger` の非null値はエラーになります。設定から削除してください。
    - 旧レポート: 非nullのloggerは警告して破棄します。nullと値のない `logger:` は受理します。
- `log2reqs: {}` は必須の `name` がないためエラーになります。
- `include` は宣言した設定ファイルからの相対パスで、1段だけ解決します。入れ子のincludeはエラーになります。
- `head.size: true` のように整数へboolを指定する設定は型エラーになります。
- `.netrc` の認証情報は自動で使いません。必要な `Authorization` を設定またはリクエストへ明示してください。

## 比較と再実行

- レスポンスヘッダ名の大小を無視して、除外設定と条件式を評価します。
    - 対象外: ヘッダ値と通常のJSONキーは従来どおりです。
- `retry` で `raw` 本文とレスポンスヘッダの比較設定を引き継ぎます。
    - 保存先: `report.json` の `summary.comparison` に比較設定を保存します。
    - 旧レポート: 比較設定がなければ警告し、ヘッダ比較なし、除外キー `Content-Length` と `Date` を補います。
- JSONキーに `root` やブラケットを含む場合の差分パスと値取得を修正しています。
- 同梱の `encoding_hell` は廃止済みの設定と不足していたGET列を修正しています。
- エラーを受け取ると新しいtrialへの着手を止め、実行中のtrialが終わるまで待ちます。
    - 影響: エラー時の送信件数と保存件数はPython版と異なる場合があります。
- `--processes` は並列数として扱います。両方指定すると `--threads` より優先します。
    - 出力: concurrencyの形式は維持します。

## JSONとテンプレート

- `NaN`、`Infinity`、`-Infinity` とf64の範囲を超える浮動小数点数を拒否します。
    - 条件式: 入力や評価結果の非有限数もエラーになります。
- lone surrogateを含むJSONを拒否します。
- JSONの配列と辞書は1,000段まで扱います。1,001段でエラーになります。
- Optionに `is_some()` が加わります。互換の値参照には `value`、`get()`、`get_or()` を使えます。

### サポート外のテンプレート表現

以下は設定エラーで停止します。

| 分類 | サポート外 |
| --- | --- |
| filter | `center`、`forceescape`、`urlize`、`xmlattr` |
| test | `callable` |
| Python文字列メソッド | `rsplit`、`index`、`partition`、`zfill`、`encode` |
| 演算 | `"%d" % n` |

未実行の分岐やmacro内でも、未対応のfilterとtestは検査します。
MiniJinja固有の以下の拡張も使えません。

- filter: `bool`、`chain`、`lines`、`split`、`zip`
- test: `startingwith`、`endingwith`、`safe`

詳細: [テンプレートのサポート外](https://github.com/tadashi-aikawa/jumeaux-rs/blob/main/docs/compat-spec.md#テンプレートのサポート外)

## CLIと通知

- `--version` は `jumeaux 0.1.0` の形式です。`-V` と `help` サブコマンドも使えます。
- `init` のテンプレート名一覧はソート順です。
- 通知のUser-Agentは `jumeaux/<version>` です。
- Slack v1もWebhook未設定時は警告してスキップします。
- 保存I/Oの汎用エラー名は `IOError` です。Python固有のtracebackは表示しません。

細かなテンプレート、正規表現、文字コード、HTMLとXMLの許容差異は互換仕様にまとめています。

詳細: [互換仕様](https://github.com/tadashi-aikawa/jumeaux-rs/blob/main/docs/compat-spec.md#意図的に変える差異)
