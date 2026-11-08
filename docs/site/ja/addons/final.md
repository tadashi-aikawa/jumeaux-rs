final
=======================

[s1]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final

[ソースコード][s1]

Jumeauxの処理が完了する直前処理を行う事ができます。


summary
------------------------------

[summary]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/summary.rs

[ソースコード][summary]

結果の概要をテキスト形式で出力します。
出力された項目の定義は[report]を参考にしてください。


### Config

#### Definitions

##### Root

| Key    | Type   | Description                                       | Example | Default |
|--------|--------|---------------------------------------------------|---------|---------|
| sysout | (bool) | ファイルではなく標準出力を使うか  | true    | false   |

!!! info "sysout"

    * trueでない場合はファイルが作成されます
    * ファイルはconfigの[response_dir]で指定されたディレクトリの中に`summary.txt`という名前で作成されます


#### Examples

##### 結果の概要ファイルを出力する

```yaml
  final:
    - name: summary
```

##### 結果の概要を標準出力に出力する

```yaml
  final:
    - name: summary
      config:
        sysout: true
```


json
------------------------

[json]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/json.rs

[ソースコード][json]

結果のレポートをjson形式で出力します。
出力されたjsonの定義は[report]を参照してください。


### Config

#### Definitions

##### Root

| Key    | Type   | Description                                       | Example | Default |
|--------|--------|---------------------------------------------------|---------|---------|
| sysout | (bool) | ファイルではなく標準出力を使うか  | true    | false   |
| indent | (int)  | インデント                        | 2       | -       |

!!! info "sysout"

    * trueでない場合はファイルが作成されます
    * ファイルはconfigの[response_dir]で指定されたディレクトリの中に`report.json`という名前で作成されます

!!! info "indent"

    未指定だと1行で出力されます。

#### Examples

##### 結果のレポートをjsonファイルで出力する

```yaml
  final:
    - name: json
```

##### 結果のレポートをインデントサイズ4で標準出力に出力する

```yaml
  final:
    - name: json
      config:
        sysout: true
        indent: 4
```


miroir
----------------------------

[miroir]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/miroir.rs

[ソースコード][miroir]

Miroir参照用にデータをAWSに登録します。

Cargo feature `miroir` は既定で有効です。GitHub Releasesの配布バイナリにも含まれます。

- `--no-default-features` でビルドした場合、`final/miroir` の設定はエラーになります。
    - 条件: `when` でスキップされる設定でも拒否します。
- 再び有効にするには `--features miroir` を付けてビルドしてください。

### 認証と接続

AWS SDK for Rustの認証プロバイダを使います。
環境変数、共有プロファイル、Web Identity、コンテナ、IMDSv2に対応しています。
`assumed_role_arn` を指定するとAssumeRoleで取得した認証情報を使います。

| 項目 | Rust版 | 基準Python版 |
| --- | --- | --- |
| リージョンの優先順 | `AWS_REGION` → `AWS_DEFAULT_REGION` → 共有設定 → IMDS | 通常の環境変数は `AWS_DEFAULT_REGION` |
| 最大試行回数 | standardで3回 | legacyでDynamoDBは10回、S3は5回 |
| 接続タイムアウト | 3.1秒 | 60秒 |
| 操作全体と1試行全体のタイムアウト | 既定では未設定 | botocoreの設定に従う |

- 試行回数には初回を含みます。
    - 条件: SDKの環境変数や共有プロファイルで上書きしていない場合の値です。
- `AWS_REGION` と `AWS_DEFAULT_REGION` が異なる場合は、Python版と接続先のリージョンが変わります。
- リージョンを解決できない場合は、AWSへ送信する前に `NoRegionError` で停止します。
- API比較用のCLIタイムアウトはAWS SDKへ適用しません。
- S3のendpointを上書きした場合はpath-styleで接続します。
    - 対象: アドオン設定、環境変数、共有プロファイルによる上書き。
    - 例: `http://localhost:9000/<bucket>/<key>`。
    - 既定: endpointの上書きがない場合はSDK既定のaddressingに従います。
- `endpoints` はサービス単位でSDKの設定より優先します。
    - 省略したサービス: SDK既定のendpointを使います。
    - STS: `local_stack` と `endpoints` の対象外です。`AWS_ENDPOINT_URL_STS` などSDKの設定を使います。


### Config

#### Definitions

##### Root

|       Key        |           Type            |                     Description                     |     Example      | Default |
| ---------------- | ------------------------- | --------------------------------------------------- | ---------------- | ------- |
| table            | string                    | 転送先DynamoDBのテーブル名                          | miroir           |         |
| bucket           | string                    | 転送先S3のBucket名                                  | mamansoft-miroir |         |
| prefix           | (string)                  | S3の `results/<key>/` より前に付けるprefix          | test             |         |
| cache_max_age    | (int)                     | S3に転送したレスポンスのキャッシュ生存期間(秒)      | 3600             | 0       |
| with_zip         | (bool)                    | ReportとレスポンスをzipしたファイルをS3に転送するか | false            | true    |
| assumed_role_arn | (string)                  | AssumeRoleで使うIAMロールのARN                      |                  |         |
| checklist        | (string)                  | DynamoDBへ保存するchecklist属性                     |                  |         |
| local_stack      | [LocalStack](#localstack) | LocalStackを使用する場合に設定する                  |                  |         |
| endpoints        | [Endpoints](#endpoints)   | DynamoDB/S3のエンドポイントを個別指定する           |                  |         |
| when             | (When[])                  | すべて満たした場合にMiroirへ転送する条件            | `[not_empty]`    | `[]`    |

`prefix` が未指定、null、空文字の場合は `results/<key>/` に保存します。
`prefix: test` の場合は `test/results/<key>/` に保存します。

`cache_max_age`、`with_zip`、`when` にnullを指定した場合も、上の既定値になります。

??? info "when"

    |   Name        |               Description               |
    | ------------- | --------------------------------------- |
    | not_empty     | 結果が空ではないとき                    |
    | has_different | 結果にdifferentが存在するとき           |

    * 全ての条件を満たす場合のみ転送します
    * 未指定の場合は必ず転送します
    * 条件を満たさない場合は認証情報を読み込みません

##### LocalStack

|   Key    |   Type   |        Description         |      Example      |     Default      |
| -------- | -------- | -------------------------- | ----------------- | ---------------- |
| use      | bool     | LocalStackを使用するか     | true              |                  |
| endpoint | (string) | LocalStackのエンドポイント | http://localstack | http://localhost |

`use: true` の場合、DynamoDBは `<endpoint>:4569`、S3は `<endpoint>:4572` に接続します。

##### Endpoints

|   Key    |   Type   |           Description            |      Example      | Default |
| -------- | -------- | -------------------------------- | ----------------- | ------- |
| dynamodb | (string) | DynamoDBのエンドポイント         | http://localhost:3456 |         |
| s3       | (string) | S3のエンドポイント               | http://localhost:3456 |         |

!!! warning

    `local_stack` と `endpoints` が両方非nullの場合は設定読み込み時にエラーになります。
    `local_stack.use: false` の場合も同じです。

#### Examples

##### キャッシュ1時間で保存する

```yaml
  final:
    - name: miroir
      config:
        table: miroir
        bucket: mamansoft-miroir
        cache_max_age: 3600
```

##### キャッシュなしでprefixを指定して保存する

Bucketの `test/results/<key>/` 配下にデータが保存されます。

```yaml
  final:
    - name: miroir
      config:
        table: miroir
        bucket: mamansoft-miroir
        prefix: test
```

##### LocalStackの旧ポート構成を使ってキャッシュ2分で保存する

```yaml
  final:
    - name: miroir
      config:
        table: miroir
        bucket: mamansoft-miroir
        cache_max_age: 120
        local_stack:
          use: true
          endpoint: http://localhost
```

##### DynamoDBとS3をLocalStackの統合ポートへ送信する

```yaml
  final:
    - name: miroir
      config:
        table: miroir
        bucket: mamansoft-miroir
        endpoints:
          dynamodb: http://localhost:4566
          s3: http://localhost:4566
```

この例では `local_stack` を指定しません。
S3は `http://localhost:4566/<bucket>/<key>` へ送信します。

##### 結果が空でないときだけ保存する

```yaml
  final:
    - name: miroir
      config:
        table: miroir
        bucket: mamansoft-miroir
        when:
          - not_empty
```


notify
----------------------------

[notify]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/notify.rs

[ソースコード][notify]

実行結果を通知します。


### Prerequirements

--8<-- "ja/notify-prerequirements.md"


### Config

#### Definitions

##### root

|   Key    |         Type          |        Description         | Example | Default |
| -------- | --------------------- | -------------------------- | ------- | ------- |
| notifies | ([Notify[]](#notify)) | 通知設定                    |         |         |

##### Notify

| Key      | Type   | Description                              | Example                                                 | Default |
| -------- | ------ | ---------------------------------------- | ------------------------------------------------------- | ------- |
| notifier | string | 使用する通知設定の名前   | jumeaux                                                 |         |
| message  | string | 送信するメッセージ       | <pre>{{ title }}が完了しました</pre>                    |         |
| when     | str    | 通知条件式               | <pre>'summary.status.different > 0'</pre>               |         |


!!! info "notifierについて"

    通知設定の例は [config/examples] を参考にしてください。定義は [notifier] の通りです。

!!! info "messageについて"

    [Template表記]に対応しています。
    プロパティは[Report](../getstarted/report.md)で定義されたものを使用できます。

!!! info "whenで指定できるプロパティ"

    [Template表記]に対応しています。
    プロパティは[Report](../getstarted/report.md)で定義されたものを使用できます。

#### Examples

紹介する例は`jumeaux`という名前の[notifier]が設定されている必要があります。

??? note "設定例"
    ```yaml
    notifiers:
      jumeaux:
        type: slack
        version: 2
    ```

##### 通知設定`jumeaux`を使って終了時通知する

```yaml
  final:
    - name: notify
      config:
        notifies:
          - notifier: jumeaux
            message: "{{ title }} is Finish!! There are {{ summary.status.different }} diffs.."
```

##### differentのstatusが存在するときのみ通知する

```yaml
  final:
    - name: notify
      config:
        notifies:
          - notifier: jumeaux
            message: "There are {{ summary.status.different }} diffs.."
            when: "summary.status.different > 0"
```

csv
----------------------

[csv]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/csv.rs

[ソースコード][csv]

レポートの`trials`をCSVファイル形式で追加出力します。


### Config

#### Definitions

|     Key      |   Type   |               Description                |        Example        | Default |
| ------------ | -------- | ---------------------------------------- | --------------------- | ------- |
| column_names | string[] | 出力する要素名のリスト   | `[seq, name, status]` |         |
| output_path  | string   | 出力するCSVファイルのパス                | result.csv            |         |
| with_header  | (bool)   | ヘッダ行を出力するか                     | true                  | false   |

??? info "column_names"

    以下の要素が有効です。

    |        Name        |            Description            |
    | ------------------ | --------------------------------- |
    | seq                | シーケンス                        |
    | name               | 名称                              |
    | method             | HTTPメソッド                      |
    | path               | パス                              |
    | headers            | ヘッダ(JSON文字列)                |
    | queries            | クエリ(JSON文字列)                |
    | request_time       | リクエスト日時                    |
    | status             | ステータス                        |
    | one.url            | oneのリクエストURL                |
    | one.status         | oneのステータスコード             |
    | one.byte           | oneのレスポンスサイズ             |
    | one.response_sec   | oneのレスポンスタイム(秒)         |
    | one.content_type   | oneのコンテントタイプ             |
    | one.encoding       | oneのレスポンスエンコーディング   |
    | other.url          | otherのリクエストURL              |
    | other.status       | otherのステータスコード           |
    | other.byte         | otherのレスポンスサイズ           |
    | other.response_sec | otherのレスポンスタイム(秒)       |
    | other.content_type | otherのコンテントタイプ           |
    | other.encoding     | otherのレスポンスエンコーディング |


#### Examples

##### `seq` `name` `status` の要素を出力する

```yaml
  final:
    - name: csv
      config:
        column_names:
          - seq
          - name
          - status
        output_path: result.csv
```

##### `seq` `name` `status`, `one.response_sec`, `other.response_sec` の要素をヘッダ付きで出力する

```yaml
  final:
    - name: csv
      config:
        with_header: true
        column_names:
          - seq
          - name
          - status
          - one.response_sec
          - other.response_sec
        output_path: result.csv
```


viewer
----------------------------

[viewer]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/final/viewer.rs

[ソースコード][viewer]

結果をGUIで確認するためのHTMLを出力します。

出力される`index.html`は同じディレクトリに以下のエントリがいる前提で動作します。

* oneディレクトリ
* otherディレクトリ
* report.json

`report.json`を作成するには`final/json`アドオンを指定してください。

### Config

#### Definitions

Config設定はありません。

#### Examples

##### 結果をGUIで確認するためのviewerを同梱する

```yaml
  final:
    - name: json  # report.jsonが必要なため
    - name: viewer
```


[Template表記]: ../template.md
[report]: ../getstarted/report.md
[response_dir]: ../getstarted/configuration.md#outputsummary
[notifier]: ../models/notifier.md
[config/examples]: ../getstarted/configuration.md#examples
