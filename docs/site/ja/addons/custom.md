# カスタムアドオン

外部コマンド方式を準備中です。

Rust版ではPythonモジュールを読み込む方式を使えません。
アドオン名へPythonモジュールのパスを指定すると、設定エラーになります。
`cls_name` は互換用の `Executor` だけを受理します。

外部コマンドの設定、入出力、エラー処理、タイムアウトの仕様は策定中です。
確定後、このページに設定と実装例を掲載します。

## jumeaux init addon

現在はひな形を生成しません。準備中の案内を表示し、終了コード1で停止します。

## res2res/jsonのtransformer {#transformer}

Pythonの `module` と `function` を指定するtransformerは使用できません。
外部コマンド方式への移行を準備中です。

JSONレスポンスをソートする場合は [json_sort](res2res.md#json_sort) を使えます。
レスポンスの形式だけを指定する場合は [type](res2res.md#type) を使えます。
