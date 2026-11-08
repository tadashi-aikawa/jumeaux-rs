# 外部コマンドアドオン

Python 3の標準ライブラリだけで動きます。

`config.example.yml` の各要素を、既存config.ymlの同じレイヤーの配列へ追記してください。

- 既存のjudgement・finalは残し、必要な位置へ追加します。
- 複数configのマージではレイヤー配列は連結されません。
  - 注意: この断片を別configとして指定するだけでは追記になりません。
- `command` は引数の配列です。シェル展開は行いません。
- `options` はアドオン固有の設定です。
- stdinからJSON文書を1つ読み、stdoutへ戻り値のJSON文書を1つ返します。
- stdinは必ずEOFまで読み切ってから応答します。
  - 理由: ホストは入力全体の送信を確認します。送信完了前に終了するとエラーになります。
- 診断と表はstderrへ出します。
- `validate` では設定を検証し、`{"valid":true}` を返します。
- retryには毎回 `--allow-exec` が必要です。

3つのスクリプトは、それぞれリクエスト追加、値による差分の無視、表表示の例です。

- ignore_valuesはunknownの差分だけを分類します。ヘッダの判定は変更しません。
- tableはタブ区切りで全行をstderrへ出します。
- 旧Pythonモジュールをcommandへ指定するだけでは動きません。stdin/stdoutの契約への書き換えが必要です。

詳細: https://tadashi-aikawa.github.io/jumeaux-rs/addons/exec/

## JSON transformer

以下を `transforms/wrap.py` として保存します。

```python
import base64
import json
import sys

try:
    message = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    if type(message.get("protocol_version")) is not int or message["protocol_version"] != 1 or message.get("layer") != "res2res":
        raise ValueError("unsupported protocol or layer")
    options = message["options"]
    if not isinstance(options, dict) or set(options) - {"key", "input_encoding"}:
        raise ValueError("options accepts only key and input_encoding")
    key = options.get("key", "wrap")
    fallback = options.get("input_encoding", "utf8")
    if not isinstance(key, str) or not isinstance(fallback, str):
        raise ValueError("key and input_encoding must be strings")
    "".encode(fallback)
    if message["mode"] == "validate":
        result = {"valid": True}
    elif message["mode"] == "json_transformer":
        payload = message["payload"]
        encoding = payload["encoding"] if payload["encoding"] is not None else fallback
        body = base64.b64decode(payload["body"], validate=True)
        value = json.loads(body.decode(encoding))
        result = json.dumps({key: value}, ensure_ascii=False, allow_nan=False)
    else:
        raise ValueError("unsupported mode")
    sys.stdout.buffer.write((json.dumps(result, allow_nan=False) + "\n").encode("utf-8"))
except Exception as error:
    print(str(error), file=sys.stderr)
    sys.exit(1)
```

設定例:

```yaml
addons:
  res2res:
    - name: json
      config:
        when: '"sample" in req.path'
        default_encoding: utf8
        transformer:
          command: [python3, transforms/wrap.py]
          options: {key: wrap}
```

入力のencodingがnullなら、この雛形はdecodeに `options.input_encoding` を使います。既定はutf8です。

ホストのdefault_encodingは出力のencode用です。コマンドへ渡すencodingのnullは保持されます。
