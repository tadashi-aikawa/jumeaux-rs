Configuration
=============

設定ファイル(デフォルト`config.yml`)について説明します。

## Definitions

### Config

| Key                         | Type                            | Description                                 | Example                        | Default                      |
| --------------------------- | ------------------------------- | ------------------------------------------- | ------------------------------ | ---------------------------- |
| one                         | [AccessPoint][access-point]     | 比較元のアクセス先情報                      |                                |                              |
| other                       | [AccessPoint][access-point]     | 比較先のアクセス先情報                      |                                |                              |
| output                      | [OutputSummary](#outputsummary) | 出力に関する設定                            |                                |                              |
| threads                     | (int)                           | 同時実行するtrial数                          | 2                              | 1                            |
| processes                   | (int)                           | threadsより優先する並列数                    | 2                              |                              |
| max_retries                 | (int)                           | 接続エラー時の最大リトライ数                | 0                              | 3                            |
| title                       | (string)                        | タイトル                                    | Test                           | No title                     |
| description                 | (string)                        | 説明                                        | Running for test               |                              |
| tags                        | (string[])                      | タグ                                        | <pre>- test<br>- jumeaux</pre> |                              |
| notifiers                   | (dict[[Notifier][notifier]])    | 通知設定                    |                                |                              |
| judge_response_header       | (bool)                          | レスポンスヘッダも判定に加えるかどうか      | `true`                         | `false`                      |
| ignore_response_header_keys | (string[])                      | レスポンスヘッダの比較で無視するkeyのリスト | `["Date", "Server"]`           | `["Content-Length", "Date"]` |
| addons                      | [Addons][addons]                | 利用するアドオンの設定                      |                                |                              |

!!! info "並列実行"

    Rust版では `threads` を同時実行するtrial数として扱います。oneとotherへのリクエストは同時に送信します。

!!! warning "threadsとprocessesを指定した場合"

    有効な `processes` の指定がある場合は、そちらを並列数として使います。
    レポートのconcurrencyは `threads: 1` と指定した `processes` になります。

未知の設定キーと非nullの `output.logger` はエラーになります。

詳細: [Python版からの移行](migration.md)

!!! info "notifiers"

    アドオンなどで通知が必要な場合、notifiersのキーを指定します。

### OutputSummary


|     Key      |       Type       |              Description               |    Example     | Default |
| ------------ | ---------------- | -------------------------------------- | -------------- | ------- |
| response_dir | string           | レスポンスを格納するディレクトリのパス | test/responses |         |
| encoding     | (string)         | 出力するレポートのエンコーディング     | euc-jp         | utf8    |


## Examples

### リクエストが空だと通知するconfigの例

```yaml
title: Test
description: Running for test

one:
  name: One
  host: https://raw.githubusercontent.com/tadashi-aikawa/jumeaux-toolbox/master/vagrant/ignore/one

other:
  name: Other
  host: https://raw.githubusercontent.com/tadashi-aikawa/jumeaux-toolbox/master/vagrant/ignore/other

notifiers:
  test-notifier:
    type: slack
    version: 2

output:
  response_dir: responses

addons:
  log2reqs:
    name: plain

  reqs2reqs:
    - name: empty_guard
      config:
        notifies:
          - notifier: test-notifier
            message: "{{ title }} notify!"
```

[addons]: ../addons/index.md#configuration-definitions
[notifier]: ../models/notifier.md
[access-point]: ../models/access-point.md
