did_challenge
===============================

[s1]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/did_challenge

[ソースコード][s1]

次のchallengeに移る前に処理をします。


sleep
--------------------------

[sleep]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/did_challenge/sleep.rs

[ソースコード][sleep]

指定した時間、待機します。


### Config

#### Definitions

| Key | Type  |            Description             | Example | Default |
| --- | ----- | ---------------------------------- | ------- | ------- |
| min | float | 待機時間  の下限値 | 0.1     |         |
| max | float | 待機時間  の上限値 | 1.0     |         |

!!! info "待機時間について"

    待機時間は`min`から`max`の範囲でランダムに決定されます。
    固定値にしたい場合は`min`と`max`に指定したい値を設定してください。

#### Examples

##### ランダムで0.1～1.0秒待機する

```yaml
  did_challenge:
    - name: sleep
      config:
        min: 0.1
        max: 1.0
```

##### 0.5秒待機する

```yaml
  did_challenge:
    - name: sleep
      config:
        min: 0.5
        max: 0.5
```


tag
----------------------

[tag]: https://github.com/tadashi-aikawa/jumeaux-rs/tree/main/src/addons/did_challenge/tag.rs

[ソースコード][tag]

リクエスト結果ごとにタグを設定します。


### Config

#### Definitions

##### Root

| Key        | Type                      | Description    | Example | Default |
|------------|---------------------------|----------------|---------|---------|
| conditions | [Condition[]](#condition) | タグの値と条件 |         |         |

##### Condition

| Key  | Type | Description                   | Example                           | Default |
|------|------|-------------------------------|-----------------------------------|---------|
| tag  | str  | 付与するタグ  | `tag: {{ trial.one.type }}`       |         |
| when | str  | 条件式        | <pre>"trial.name == 'hoge'"</pre> |         |


!!! info "tagおよびwhenについて"

    [Template表記]に対応しています。
    プロパティは以下を使用できます。

    | key             | Type                 | Description                 |
    |-----------------|----------------------|-----------------------------|
    | trial           | [Trial][trial]       | テスト結果                  |
    | res_one         | [Response][response] | oneのレスポンス情報         |
    | res_other       | [Response][response] | otherのレスポンス情報       |
    | res_one_props   | (dict)               | oneのレスポンスプロパティ   |
    | res_other_props | (dict)               | otherのレスポンスプロパティ |


#### Examples

##### Trialのnameが`json`のとき、`json`というタグを付ける

```yaml
  did_challenge:
    - name: tag
      config:
        conditions:
          - tag: json
            when: "trial.name == 'json'"
```

##### それぞれのレスポンスタイプをタグとして付ける

```yaml
  did_challenge:
    - name: tag
      config:
        conditions:
          - tag: "tag:{{ trial.one.type }}"
          - tag: "tag:{{ trial.other.type }}"
```

##### oneのレスポンスプロパティ`items[0].type` の値をタグにつける

```yaml
  did_challenge:
    - name: tag
      config:
        conditions:
          - tag: "{{ res_one_props.items[0].type }}"
```

[Template表記]: ../template.md
[trial]: ../models/trial.md
[response]: ../models/response.md
