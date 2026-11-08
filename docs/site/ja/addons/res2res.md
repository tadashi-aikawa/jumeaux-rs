res2res [:fontawesome-brands-github:][s1]
=========================

[s1]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/res2res

APIから返却されたレスポンスを判定前に変換します。


[:fontawesome-brands-github:][json] json
------------------------

[json]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/res2res/json.rs

任意のレスポンスをJSONへ変換するアドオンです。

!!! warning "準備中"

    Pythonの `module` と `function` を指定するtransformerはRust版では使用できません。
    現在この設定はエラーになります。外部コマンド方式への移行を準備中です。

詳細: [カスタムアドオン](custom.md#transformer)


[:fontawesome-brands-github:][json-sort] json_sort
----------------------------------

[json-sort]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/res2res/json_sort.rs

JSONレスポンスの並び順をソートします。

!!! warning "処理がスキップされるケース"

    `content-type` が `text/json` や `application/json` でない場合はレスポンスがJSONでないとみなされ処理がスキップされます。


### Config

#### Definitions

##### Root

|       Key        |        Type         |                    Description                     | Example | Default |
| ---------------- | ------------------- | -------------------------------------------------- | ------- | ------- |
| items            | [Sorter[]](#sorter) | ソート設定のリスト                                 |         |         |
| footprints_tag   | (string)            | 本アドオンで結果が変わったときに足跡として追加するタグ名 | sorted |          |


##### Sorter

|   Key   |        Type         |       Description       |          Example          | Default |
| ------- | ------------------- | ----------------------- | ------------------------- | ------- |
| when    | str                 | 条件式 :fontawesome-solid-circle-exclamation: | <pre>"qs.id.0 == 1"</pre> |         |
| targets | [Target[]](#target) |                         |                           |         |

??? info "when"

    [jinja2の表現](http://jinja.pocoo.org/docs/2.10/templates)を利用できます。
    プロパティは[request]で定義されたものを使用できます。


##### Target

|    Key    |    Type    |                            Description                             |         Example          |     Default      |
| --------- | ---------- | ------------------------------------------------------------------ | ------------------------ | ---------------- |
| path      | string     | ソート対象となるプロパティの正規表現                               | root<'dict1'><'list1-1'> |                  |
| sort_keys | (string[]) | pathで指定したプロパティがObjectだった場合のソートプロパティリスト | `[id, name]`             | :fontawesome-solid-circle-exclamation: |

!!! info "`sort_keys`が未指定の場合"

    指定プロパティの値でソートします。Objectの場合はJson Encodeした結果でソートします。

#### Examples

##### pathが`/filter`である場合 `dict1.list1-1` のリストをソートする

```yaml
  res2res:
    - name: json_sort
      config:
        items:
          - when: "path == '/filter'"
            targets:
              - path: root<'dict1'><'list1-1'>
```

##### pathが`/filter`である場合 `list2` のリストをid, nameの優先順にソートする

```yaml
  res2res:
    - name: json_sort
      config:
        items:
          - when: "path == '/filter'"
            targets:
              - path: root<'list2'>
                sort_keys: [id, name]
```


[:fontawesome-brands-github:][type] type
------------------------

[type]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/res2res/type.rs

レスポンスのtypeを変更します。
typeはJumeauxのアドオンや連携先アプリケーションでファイル形式を判定するために使用されます。

### Config

#### Definitions

##### Root

| Key        | Type                      | Description  | Example | Default |
|------------|---------------------------|--------------|---------|---------|
| conditions | [Condition[]](#condition) | 変更値と条件 |         |         |

##### Condition

| Key  | Type | Description             | Example                             | Default |
|------|------|-------------------------|-------------------------------------|---------|
| type | str  | 変更後のtype            | json                                |         |
| when | str  | 条件式 :fontawesome-solid-circle-exclamation: | <pre>"res.status_code == 200"</pre> |         |

!!! info "whenで指定できるプロパティ"

    [Template表記]に対応しています。
    プロパティは以下を使用できます。

    | key | Type                 | Description    |
    |-----|----------------------|----------------|
    | req | [Request][request]   | リクエスト情報 |
    | res | [Response][response] | レスポンス情報 |


#### Examples

##### pathに`target`という文字列が含まれる場合に`json`へtypeを変更する

```yaml
  res2res:
    - name: type
      config:
        conditions:
          - type: json
            when: "'target' in req.path"
```

[Template表記]: ../template.md
[request]: ../models/request.md
[response]: ../models/response.md
