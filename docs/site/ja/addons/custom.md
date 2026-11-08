# カスタムアドオン

カスタムアドオンは9レイヤー共通の `exec` を使います。

Rust版ではPythonモジュールを読み込む方式を使えません。
アドオン名へPythonモジュールのパスを指定すると、設定エラーになります。
`cls_name` は互換用の `Executor` だけを受理します。

```yaml
addons:
  reqs2reqs:
    - name: exec
      config:
        command: [python3, addon_sample/reqs2reqs/add_path.py]
        options:
          path: /added.json
```

旧アドオンのconfigは `config.options` へ移します。
コマンドはシェルを介さず、配列の先頭を実行ファイル、残りを引数として起動します。

詳細: [外部コマンドの設定とプロトコル](exec.md)

## jumeaux init addon

```sh
jumeaux init addon
```

`addon_sample/` に次のPython 3製の雛形を生成します。
既存の `addon_sample/` は上書きしません。

- `reqs2reqs/add_path.py`: パスを追加します。
- `judgement/ignore_values.py`: 指定値の差分を分類します。
- `final/table.py`: 表をstderrへ出力します。

Jumeaux本体はPython不要です。
生成した雛形をそのまま実行する場合はPython 3が必要です。

## res2res/jsonのtransformer {#transformer}

Pythonの `module` と `function` を指定するtransformerは使用できません。
`transformer.command` と `transformer.options` へ移行してください。

```yaml
addons:
  res2res:
    - name: json
      config:
        default_encoding: utf8
        transformer:
          command: [./response-to-json]
          options: {}
```

transformerはstdinのJSONに含まれる `payload.body` をBase64から復元して変換します。
stdoutへJSONテキストを表すJSON文字列を返します。

## retry

外部コマンドを含むレポートには明示許可が必要です。

```sh
jumeaux retry report.json --allow-exec
```

許可はレポートへ保存しません。再実行のたびに指定してください。
許可がなければ、起動前検証も含めて外部コマンドを起動しません。

JSONレスポンスをソートする場合は [json_sort](res2res.md#json_sort) を使えます。
レスポンスの形式だけを指定する場合は [type](res2res.md#type) を使えます。
