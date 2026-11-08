reqs2reqs
===========================

[s1]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs

[ソースコード][s1]

リクエストを変換します。


head
------------------------

[head]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/head.rs

[ソースコード][head]

リクエストの先頭部分を抽出します。

### Config

#### Definitions

| Key  | Type |      Description       | Example | Default |
| ---- | ---- | ---------------------- | ------- | ------- |
| size | int  | 抽出するリクエストの数 | 10      |         |


#### Examples

##### 先頭の10リクエストを抽出する

```yaml
  reqs2reqs:
    - name: head
      config:
        size: 10
```


filter
----------------------------

[filter]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/filter.rs

[ソースコード][filter]

条件に一致するリクエストのみを抽出します。

### Config

#### Definitions

##### Root

| Key  | Type |       Description       |          Example          | Default |
| ---- | ---- | ----------------------- | ------------------------- | ------- |
| when | str  | 条件式  | <pre>"qs.id.0 == 1"</pre> |         |


??? info "when"

    [jinja2の表現](http://jinja.pocoo.org/docs/2.10/templates)を利用できます。
    プロパティは[request]で定義されたものを使用できます。


#### Examples

##### pathに`ok`が含まれ かつ nameに`OK`が含まれるリクエストのみ抽出する

```yaml
  reqs2reqs:
    - name: filter
      config:
        when: ('ok' in path) and ('OK' in name)
```


add
----------------------

[add]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/add.rs

[ソースコード][add]

先頭または末尾にリクエストを追加します。

### Config

#### Definitions

##### root

|   Key    |          Type           |           Description           | Example | Default |
| -------- | ----------------------- | ------------------------------- | ------- | ------- |
| reqs     | [Request[]][request]    | 本設定を反映させるRequestの条件 |         |         |
| location | ([Location](#location)) | 先頭と末尾どちらに追加するか    | tail    | head    |


##### Location

| Name | Description |
| ---- | ----------- |
| head | 先頭        |
| tail | 末尾        |


#### Examples

##### 末尾に`/rest/sample?word=hoge`というリクエストを追加する

```yaml
  reqs2reqs:
    - name: add
      config:
        reqs:
          - path: /rest/sample
            qs: {word: [hoge]}
        location: tail
```


replace
------------------------------

[replace]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/replace.rs

[ソースコード][replace]

条件に一致するリクエストを置換します。

### Config

#### Definitions

##### Root

|  Key  |          Type           |   Description    | Example | Default |
| ----- | ----------------------- | ---------------- | ------- | ------- |
| items | [Replacer[]](#replacer) | 置換設定のリスト |         |         |

##### Replacer

| Key     | Type         | Description                     | Example                                 | Default |
| ------- | ------------ | ------------------------------- | --------------------------------------- | ------- |
| when    | (str)        | 条件式          | <pre>"qs.id.0 == 1"</pre>               |         |
| queries | dict[str[]]) | 置換するクエリ  | <pre>{"a": [1], "b": [2, 3]}</pre>      |         |
| headers | dict[str]    | 置換するヘッダ                  | <pre>{"header1": 1, "header2": 2}</pre> |         |

??? info "when"

    [jinja2の表現](http://jinja.pocoo.org/docs/2.10/templates)を利用できます。
    プロパティは[request]で定義されたものを使用できます。

??? info "queriesで使える$DATETIMEマクロについて"

    値に`$DATETIME`マクロを使うと現在時刻からの相対時刻に任意のフォーマットで置換できます。

    - フォーマットは`$DATETIME(フォーマット)(相対秒)`
    - 現在時刻が2021年4月18日20時52分00秒のとき
        - `$DATETIME(%Y-%m-%dT%H:%M:%S)(3600)`で`2021-04-18T21:52:00`になる
        - `$DATETIME(%Y/%m/%d)(-86400)`で`2021/04/17`になる

!!! info "`queries`と`headers`について"

    リクエストに存在しないクエリやヘッダの場合は、設定した値が追加されます

#### Examples

##### pathが`/target`と一致するリクエストのクエリを置換する (`id=dummay_id`, `time=dummy_date`)

```yaml
  reqs2reqs:
    - name: replace
      config:
        items:
          - when: "path == '/target'"
            queries:
              id: ["dummy_id"]
              time: ["dummy_date"]
```

##### queryに`time`が含まれるとき、現在時刻1時間後の`%Y-%m-%dT%H:%M:%S`フォーマットで上書きする

```yaml
  reqs2reqs:
    - name: replace
      config:
        items:
          - when: "'time' in qs"
            queries:
              time: ["$DATETIME(%Y-%m-%dT%H:%M:%S)(3600)"]
```

shuffle
------------------------------

[shuffle]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/shuffle.rs

[ソースコード][shuffle]

リクエストの順序をシャッフルします。

### Config

#### Definitions

Config設定はありません。

#### Examples

##### リクエストをシャッフルする

```yaml
  reqs2reqs:
    - name: shuffle
```


repeat
----------------------------

[repeat]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/repeat.rs

[ソースコード][repeat]

リクエスト全体を指定回数だけ複製します。

### Config

#### Definitions

|  Key  | Type | Description | Example | Default |
| ----- | ---- | ----------- | ------- | ------- |
| times | int  | 複製する数  | 10      |         |


#### Examples

##### リクエスト全体を10回複製する

```yaml
  reqs2reqs:
    - name: repeat
      config:
        times: 10
```


empty_guard
--------------------------------------

[empty_guard]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/empty_guard.rs

[ソースコード][empty_guard]

リクエストが空の場合に処理を中断します。


### Prerequirements

--8<-- "ja/notify-prerequirements.md"


### Config

#### Definitions

##### root

|   Key    |         Type          |        Description         | Example | Default |
| -------- | --------------------- | -------------------------- | ------- | ------- |
| notifies | ([Notify[]](#notify)) | 処理を中断する時の通知設定 |         |         |

##### Notify

|   Key    |  Type  |               Description                |             Example              | Default |
| -------- | ------ | ---------------------------------------- | -------------------------------- | ------- |
| notifier | string | 使用する通知設定の名前   | jumeaux                          |         |
| message  | string | 送信するメッセージ       | <pre>{{ title }}を中断しました</pre> |         |


!!! info "notifierについて"

    通知設定の例は [config/examples] を参考にしてください。定義は [notifier] の通りです。

!!! info "messageついて"

    [Template表記]に対応しています。
    プロパティは[Configuration](../getstarted/configuration.md)で定義されたものを使用できます。


#### Examples

##### リクエストが空の場合に処理を中断する

```yaml
  reqs2reqs:
    - name: empty_guard
```

##### リクエストが空の場合にnotifier jumeauxを使用して通知 および 処理の中断をする

```yaml
  reqs2reqs:
    - name: empty_guard
      config:
        notifies:
          - notifier: jumeaux
            message: "{{ title }} notify!"
```

!!! hint "`notifier: jumeaux`について"

    Jumeauxのconfigに以下のような設定が必要です。

    ```yaml
    notifiers:
      jumeaux:
        type: slack
        version: 2
    ```


rename
----------------------------

[rename]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/reqs2reqs/rename.rs

[ソースコード][rename]

条件に一致するリクエストの名称を変更します。

### Config

#### Definitions

##### Root

| Key        | Type                      | Description  | Example | Default |
|------------|---------------------------|--------------|---------|---------|
| conditions | [Condition[]](#condition) | 変更値と条件 |         |         |

##### Condition

| Key  | Type |          Description          |              Example               | Default |
| ---- | ---- | ----------------------------- | ---------------------------------- | ------- |
| name | str  | 変更後の名称  | <pre>{{ name }} ({{ path }})</pre> |         |
| when | str  | 条件式        | <pre>"qs.id.0 == 1"</pre>          |         |

!!! info "nameおよびwhenについて"

    [jinja2の表現](http://jinja.pocoo.org/docs/2.10/templates)を利用できます。
    プロパティは[request]で定義されたものを使用できます。

    **nameはテンプレート部分を`{{ }}`で囲む必要があります。**
    一方、**whenは式であるため`{{ }}`で囲む必要はありませんが、代わりに文字列はクォートで囲って下さい。**


#### Examples

##### pathに`target`という文字列が含まれる場合に`renamed`へ名称を変更する

```yaml
  reqs2reqs:
    - name: rename
      config:
        conditions:
          - name: renamed
            when: "'target' in path"
```

##### 複雑な条件

1. pathが小文字のアルファベット3文字である場合は`GOOD`へ名称を変更する
2. 1に該当せず`id`が1つだけ指定されており、かつ2より大きい場合は`<クエリのid>: <元の名称>`へ名称を変更する
  2-1. 例えば`id=4`で名称が`hoge`のとき、新しい名称は`4: hoge`となる
3. 1と2に該当しない場合は名称を変更しない

```yaml
  reqs2reqs:
    - name: rename
      config:
        conditions:
          - name: "GOOD"
            when: "path|reg('[a-z]{3}')"
          - name: "{{ qs.id.0 }}: {{ name }}"
            when: "qs.id|length == 1 and qs.id.0|int > 2"
```

[Template表記]: ../template.md
[request]: ../models/request.md
[notifier]: ../models/notifier.md
[config/examples]: ../getstarted/configuration.md#examples
