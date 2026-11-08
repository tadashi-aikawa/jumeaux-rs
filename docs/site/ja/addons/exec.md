# 外部コマンドアドオン

9レイヤーで `name: exec` を使えます。
`res2res/json` のtransformerも同じコマンド設定を使います。

詳細: [移行と雛形](custom.md)

## 設定

| キー | 既定値 | 内容 |
| --- | --- | --- |
| `command` | 必須 | 実行ファイルと引数の文字列配列 |
| `options` | `{}` | アドオン固有のJSONオブジェクト |
| `timeout_sec` | `60` | 起動から終了待ちまでの制限秒数 |
| `cwd` | 起動時のcwd | 子プロセスの作業ディレクトリ |
| `inherit_env` | `true` | 親の環境変数を継承するか |
| `env` | `{}` | 環境変数の上書き。値がnullなら削除 |
| `max_stdout_bytes` | `67108864` | stdoutの上限bytes |
| `max_stderr_bytes` | `65536` | エラー診断用に保持するstderr末尾bytes |

設定全体の検証後、各コマンドを `mode: "validate"` で1回呼びます。
コマンドは `{"valid":true}` を返してください。

## 入出力

stdinへJSON文書を1つ渡します。

```json
{
  "protocol_version": 1,
  "layer": "log2reqs",
  "mode": "payload",
  "options": {},
  "context": {
    "invocation_cwd": "/work/project",
    "verbosity": 0,
    "trial_seq": null,
    "side": null
  },
  "payload": {"file": "requests"},
  "reference": null
}
```

- `mode`: 通常は `payload`、JSON変換は `json_transformer`、起動前検証は `validate` です。
- `payload`: 対象レイヤーの入力です。bytesはBase64、Optionalはnullになります。
- `reference`: 参照用のスナップショットです。
- stdout: 戻り値そのものをJSON文書1つで返します。log2reqsはRequest配列、JSON変換はJSONテキストを表すJSON文字列を返します。
- stderr: 診断ログ用です。Jumeauxのstderrへ表示します。
- 成功条件: 終了コード0と、有効な戻り値の両方が必要です。

1回の呼び出しごとに別プロセスを起動します。
Pythonクラスのインスタンス状態は引き継ぎません。

macOS・Linuxを対象とします。
外部コマンドのWindows動作は対象外です。

詳細: [レイヤー別の入出力・異常系・シグナルの契約](https://github.com/tadashi-aikawa/jumeaux-rs/blob/main/docs/exec-addon-spec.md)
