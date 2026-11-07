# Jumeaux Rust版の互換仕様

## 基準

Python版 `v7.0.0`、コミット `dc276b8e3a0486bd8d4de2c8e3900aa36cbb7a66` と、その `uv.lock` を基準とする。Rust版は `0.1.0` から開始する。

- 決定の正本: owlery `shared/tasks/Jumeaux Rustリプレイス.md` の「決定事項」。
- 検証データ: 同梱テンプレート16種と既存ユニットテストの入力。
  - 条件: 実利用の設定ファイルは提供されていない。
- 未決事項は承認済み仕様として実装しない。

## 互換に保つ外部IF

| 外部IF | 契約 |
| --- | --- |
| CLI | `run`、`retry`、`init`、`server`、`viewer` の引数・既定値・終了コード、help、ログレベル |
| 標準出力 | `final/summary`・`final/json` の `sysout`。`final/csv` の任意パス出力も保存成果物として比較 |
| config.yml | 全キーと既定値、指定順のマージ、`include` の相対パス、アドオンのタグ除外と実行順 |
| requests | `plain`、`csv`、`json`、`yaml` の4形式。名前、メソッド、パス、query、ヘッダ、raw/form/json本文、URL文字コード |
| テンプレート式 | `when` の値と型、描画文字列、未定義値・エラー、Python文字列メソッド、Responseの公開プロパティ、独自filterと関数、`$DATETIME` |
| 正規表現 | `reg`、差分パスの完全一致、`json_sort` のpath、AccessPointのpath置換。Pythonの `\1`、`\2` 置換を含む |
| report.json | キー、型、null、配列順、集計値、trial、差分パス、参照ファイル、再実行情報。限定した可変値と承認済み差異のみ例外 |
| responses/ | keyディレクトリ、`latest`、`one/`、`other/`、`one-props/`、`other-props/`、report、Viewer、各アドオンの保存ファイルと内容 |
| テンプレート16種 | all_same、encoding_hell、force_json、html、ignore、ignore_order、judge_response_headers、notifier、path_custom、path_empty、post、query_custom、request_headers、root_array、simple、xml |
| Viewer | 同梱HTML、reportとレスポンスの相対参照、静的配信、report更新時のリロード、ブラウザ起動 |
| 通知 | Slack v1・v2のpayload、設定、エラーの扱い。受入試験はローカル記録用Webhookへ送る |
| その他 | `final/miroir` と `output.encoding` も実装する。Python依存機能は後述の代替方式 |

### configとconcurrency

- トップレベル: `one`、`other`、`output`、`threads`、`processes`、`max_retries`、`title`、`description`、`tags`、`input_files`、`notifiers`、`addons`、`judge_response_header`、`ignore_response_header_keys`。
- endpoint: `name`、`host`、`path.before/after`、`query.overwrite/remove`、`proxy`、`default_response_encoding`、`headers`。
- output: `response_dir`、`encoding`、旧 `logger`。
- 複数configは後のトップレベル値を優先する。addonsはレイヤーごとに更新し、リストは連結しない。
- CLI指定はPython版 `merge_args2config` と同じ条件で上書きする。
- `--processes` は受理し、並列数として扱う。
  - 規則: 有効なprocesses指定があれば `concurrency.threads = 1`、`concurrency.processes = 指定値`。
  - 規則: processesがなければ `concurrency.threads = threads`、`concurrency.processes = 1`。threads既定値は1。
  - 例: `--threads 4 --processes 2` は並列数2、レポートは `{"threads":1,"processes":2}`。

### 関数とHTTPの契約

- DeepDiff 8.6.2の `type_changes`・`values_changed` をchanged、辞書とiterableの追加をadded、削除をremovedに写す。各パスはソートする。
- 差分パスは `root<'items'><0><'id'>` 形式とする。
- scalar配列の挿入・削除、重複、辞書の配列、入れ子配列、辞書の全面置換、数値型変更をfixtureで固定する。
- HTMLはBeautifulSoup+lxmlの補完とprettify後のdictを基準にする。
  - 内容: 属性 `#attr`、本文 `##value`、同名タグのリスト、空タグ、script、混在テキストの空白。
- XMLはxmltodictのdictを基準にする。
  - 内容: 属性 `@attr`、本文 `#text`、空要素null、同名要素のリスト、名前空間、CDATA、エンティティ。
- 条件式は描画後の `ast.literal_eval` に相当する値を返す。文字列描画と区別する。
  - 例: 描画された `False` はboolのfalse。未定義値の単独条件評価はエラー。
- 独自拡張は `reg`、`calc_distance_km`、`equals_without_host`、`$DATETIME` を含む。
- `Response.text`、`Response.ok` などの公開プロパティも式への入力契約とする。
- HTTPはCookie共有、gzip展開、リダイレクト、proxy環境変数、証明書検証、リトライ対象、通信失敗分類を全体受入の対象とする。
- HTTP記録はメソッド、URL、ヘッダ、本文bytes、同一リクエストの回数を比較する。
  - 条件: 並行したone/other間の到着順に依存せず、多重集合と回数で検証する。

### 文字コード

レスポンスencodingは次の順で決める。

1. Content-Typeに `octet-stream` があればencodingなし。
2. requestsが選んだencoding。
   - 例外: Content-Typeにtextを含み、encodingが `ISO-8859-1` なら読み飛ばす。明示charsetでも同じ。
3. HTML metaまたはXML宣言。
4. `default_response_encoding`。
5. charset-normalizerの推定。

- charsetなしの `application/json` はrequestsのUTF-8が優先する。
- 入力・HTTP応答・URL・保存の各経路で別名、BOM、推定、変換失敗を検証する。
- `candidate_for_url_encodings`、`dump/encoding`、`output.encoding` を保つ。
- Response.textと保存時の変換は `errors="replace"`。
- 入力ファイル読み込みと `dump/encoding` は変換失敗をエラーにする。

## 比較除外と許容差

数値の近似許容差は設けない。次の意味を持つフィールドだけ値の完全一致を要求しない。同名の任意JSONキーを再帰的に除外してはならない。

| フィールド | 検証 |
| --- | --- |
| version | 存在と文字列型。Python版とRust版の製品版番号の差を許容 |
| key | 空でない文字列。実行ディレクトリ名と一致し、latestがそこを指すこと |
| time | summaryの開始・終了が日時で、終了が開始以降 |
| request_time | trialのリクエスト時刻がISO日時 |
| response_sec | endpointの応答時間が非負の数値 |
| elapsed_sec | 経過時間が非負の数値 |
| retry_hash | 通常runはキーなし。retryは入力に使った元reportのkeyと一致 |

- stdout/stderrとsummary.txt中の同じ可変値も、対応する意味に限定して比較する。
- API本文、dict化結果、通知本文の任意の同名キーは除外しない。
- 日時で変化する入力はハーネスで固定したテスト入力を使う。`$DATETIME` 自体は固定時刻の関数fixtureで検証する。
- モックのDate、Last-Modified、Serverは固定する。これらのレスポンス値を除外しない。
- 比較規則は `compat/harness/comparison.json` に集約する。
- API・通知のリクエストヘッダは `request_header_rules` に従う。
  - 比較対象: configのheaders、requestsのheadersに指定したヘッダ、およびContent-Type。
  - 規則: ヘッダ名は大小無視、並び順も無視する。値は完全一致とする。
  - 比較対象外: HTTPクライアントが自動付与したUser-Agent、Accept-Encoding、Accept、Connection、Content-Length、Hostなど。
  - 例外: 同じ名前でもconfigやrequestsで明示したヘッダは比較する。
  - 維持: メソッド、URL、本文bytes、回数は完全比較する。原記録には自動ヘッダも残す。
- PythonランタイムのDeprecationWarningは基準採取時に抑制する。
  - 方法: `PYTHONWARNINGS=ignore`。依存ライブラリが警告設定を上書きするため、Pythonラッパーはimport後にも再設定する。
- verboseログの計測時間はresponse_sec・elapsed_secに対応する箇所だけ正規化する。
- urllib3がrunのstderrへ出す接続・応答ログは比較から外す。
  - 対象: `Starting new HTTP connection` と `http://... "GET ..." 200 ...` 形式の行。具体的なパターンは `excluded_transport_lines` に列挙する。
  - 理由: HTTPライブラリ固有の出力であり、Rust版に同じ行を要求しない。API記録の比較は維持する。
- User-Agentログの `jumeaux/[0-9.]+` は `jumeaux/<version>` へ正規化する。
  - 扱い: 製品版番号の差としてversionの比較除外に含める。
- 作業ディレクトリ全体のツリーとファイルを比較する。
  - 規則: responses外のCSVなども対象とし、余分なファイル・ディレクトリ・symlinkも不一致にする。

## 意図的に変える差異

- Rust版はinitのテンプレート名一覧をソート順で出す。
  - Python版: ファイルシステムの列挙順で出す。一覧の並びは互換対象にしない。
  - 比較: `✨ [Valid names] ✨` 直後のテンプレート名の区間だけを順序なしの行として比較する。名前の欠落・追加・重複は許容しない。
- retry再構築で `raw`、`judge_response_header`、`ignore_response_header_keys` が失われる不具合を直す。
  - 検証: Pythonの観測値と修正後の期待値を分ける。欠落したままの実装は合格にしない。
- 差分パスの値取得でキーに `root` やブラケットなどを含む場合の不具合を直す。
  - 検証: キーとindexの構造を辿った値を期待値にする。
- レスポンスヘッダ名は小文字へ正規化してよい。
  - 規則: 除外設定と条件式ではヘッダ名の大小を無視する。本文・ヘッダ値・通常JSONキーには適用しない。
- Rust版で解釈できない設定はエラーで停止する。
  - 規則: エラーに直すべき設定キーを含める。警告だけで無視しない。
- Pythonカスタムアドオンと `res2res/json` のPython transformerは外部コマンド方式で代替する。
  - 規則: 機能は廃止しない。Pythonモジュール名を暗黙に直接実行しない。
- 並列実装方式は変えてよい。concurrencyの出力は前述の規則を保つ。

## 未決

- HTML/XMLの先行実装では203fixture中168件がPythonと完全一致する。
  - 差異: 以下の35件は許容を決定したものではない。`tests/markup_differences.json`にRustの現出力を固定し、Python期待値との不一致も検証する。
  - 維持: レビュー前の110件のPython期待値は変更していない。
- APIは`markup::html::{to_dict, pretty}`と`markup::xml::{to_dict, pretty}`。
  - 入出力: デコード済み文字列を受け取り、dictはJSON値、prettyは末尾改行を含む文字列を返す。
  - 境界: encoding、force、適用判断とCLI接続は後続タスク。条件式付き2fixtureは生成dictの参照と比較を検証し、条件式エンジン自体の統合は含めない。
- table内テキストの移動と不正なインラインタグの複製は修正済み。
  - 検証: 以前の4差異fixtureもPython期待値と一致する。

以下の出力はfixtureと同じ`value`または`error`のJSONで示す。dump文字列の改行・空白はJSONエスケープで保持する。

### XMLのDTD既定属性

- 入力: `<!DOCTYPE r [<!ATTLIST r a CDATA "default">]><r/>`
- 理由: DTDのATTLIST既定値を補完しない。

dictのPython出力:

```json
{"value":{"r":{"@a":"default"}}}
```

dictのRust出力:

```json
{"value":{"r":null}}
```

prettyのPython出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r a=\"default\"/>\n"}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r/>\n"}
```

### XMLのDTD #FIXED属性

- 入力: `<!DOCTYPE r [<!ATTLIST r a CDATA #FIXED "fixed">]><r/>`
- 理由: DTDのATTLIST #FIXED値を補完しない。

dictのPython出力:

```json
{"value":{"r":{"@a":"fixed"}}}
```

dictのRust出力:

```json
{"value":{"r":null}}
```

prettyのPython出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r a=\"fixed\"/>\n"}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r/>\n"}
```

### XMLのタグを含むエンティティのdump

- 入力: `<!DOCTYPE r [<!ENTITY x "<c>hello</c>">]><r>&x;</r>`
- 理由: エンティティ置換内のタグを要素として再パースしない。

prettyのPython出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r>\n    <c>hello</c>\n</r>\n"}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r>&lt;c&gt;hello&lt;/c&gt;</r>\n"}
```

### XMLの非ASCII要素名のdump

- 入力: `<日本語>本文</日本語>`
- 理由: PythonのElementTree→minidom経路は非ASCII要素名でExpatErrorになる。

prettyのPython出力:

```json
{"error":"ExpatError"}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<日本語>本文</日本語>\n"}
```

### XMLのエンティティ値中の`&#38;`

- 入力: `<!DOCTYPE r [<!ENTITY x "&#38;amp;">]><r a="&x;">&x;</r>`
- 理由: エンティティ宣言中の`&#38;`を展開後に再解釈する段階が異なる。

dictのPython出力:

```json
{"value":{"r":{"@a":"&"}}}
```

dictのRust出力:

```json
{"value":{"r":{"@a":"&amp;"}}}
```

prettyのPython出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r a=\"&amp;\">&amp;</r>\n"}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r a=\"&amp;amp;\">&amp;amp;</r>\n"}
```

### HTMLのコメント前後の空白

- 入力: `<p>a<!--  comment  -->b</p>`
- 理由: dump/htmlでコメント前後の空白をtrimする。

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <!--  comment  -->\n   b\n  </p>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <!--comment-->\n   b\n  </p>\n </body>\n</html>\n"}
```

### HTMLの空コメント

- 入力: `<p>a<!---->b</p>`
- 理由: dump/htmlで空コメントに空白を補わない。

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <!-- -->\n   b\n  </p>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <!---->\n   b\n  </p>\n </body>\n</html>\n"}
```

### HTMLのSVG名の大小

- 入力: `<svg viewBox="0 0 10 10"><linearGradient/></svg>`
- 理由: HTML5はSVGの要素名・属性名をキャメルケースへ補正する。

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <svg viewbox=\"0 0 10 10\">\n   <lineargradient>\n   </lineargradient>\n  </svg>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <svg viewBox=\"0 0 10 10\">\n   <linearGradient>\n   </linearGradient>\n  </svg>\n </body>\n</html>\n"}
```

### HTMLのMathML名の大小

- 入力: `<math definitionURL="x"><mi>x</mi></math>`
- 理由: HTML5はMathMLの属性名をキャメルケースへ補正する。

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <math definitionurl=\"x\">\n   <mi>\n    x\n   </mi>\n  </math>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <math definitionURL=\"x\">\n   <mi>\n    x\n   </mi>\n  </math>\n </body>\n</html>\n"}
```

### HTML5の木構造: h1内のh2

- 入力: `<h1>a<h2>b</h2>c</h1>`
- 理由: HTML5はh2開始時にh1を閉じる。

dictのPython出力:

```json
{"value":{"html":{"body":{"h1":{"##value":"a\n   \n   c","h2":{"##value":"b"}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"c","h1":{"##value":"a"},"h2":{"##value":"b"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <h1>\n   a\n   <h2>\n    b\n   </h2>\n   c\n  </h1>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <h1>\n   a\n  </h1>\n  <h2>\n   b\n  </h2>\n  c\n </body>\n</html>\n"}
```

### HTML5の木構造: 入れ子のform

- 入力: `<form id="a"><form id="b">x</form>y</form>`
- 理由: HTML5は入れ子のform開始タグを無視する。

dictのPython出力:

```json
{"value":{"html":{"body":{"##value":"y","form":[{"#id":"a"},{"##value":"x","#id":"b"}]}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"y","form":{"##value":"x","#id":"a"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <form id=\"a\">\n  </form>\n  <form id=\"b\">\n   x\n  </form>\n  y\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <form id=\"a\">\n   x\n  </form>\n  y\n </body>\n</html>\n"}
```

### HTML5の木構造: table内のform

- 入力: `<table><form><input name="x"></form></table>`
- 理由: HTML5はtable内のformを即座に閉じる。

dictのPython出力:

```json
{"value":{"html":{"body":{"table":{"form":{"input":{"##value":"","#name":"x"}}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"table":{"form":{},"input":{"##value":"","#name":"x"}}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <table>\n   <form>\n    <input name=\"x\"/>\n   </form>\n  </table>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <table>\n   <form>\n   </form>\n   <input name=\"x\"/>\n  </table>\n </body>\n</html>\n"}
```

### HTML5の木構造: table外のtr・td

- 入力: `<tr><td>x</td></tr>`
- 理由: HTML5はtable外のtr・tdを無視する。

dictのPython出力:

```json
{"value":{"html":{"body":{"tr":{"td":{"##value":"x"}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"x"}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <tr>\n   <td>\n    x\n   </td>\n  </tr>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  x\n </body>\n</html>\n"}
```

### HTML5の木構造: image

- 入力: `<image src="x">after`
- 理由: HTML5はimageをimgとして扱う。

dictのPython出力:

```json
{"value":{"html":{"body":{"image":{"##value":"after","#src":"x"}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"after","img":{"##value":"","#src":"x"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <image src=\"x\">\n   after\n  </image>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <img src=\"x\"/>\n  after\n </body>\n</html>\n"}
```

### HTML5の木構造: isindex

- 入力: `<isindex prompt="x">after`
- 理由: isindexの空要素判定がlxmlと異なる。

dictのPython出力:

```json
{"value":{"html":{"body":{"##value":"after","isindex":{"##value":"","#prompt":"x"}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"isindex":{"##value":"after","#prompt":"x"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <isindex prompt=\"x\"/>\n  after\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <isindex prompt=\"x\">\n   after\n  </isindex>\n </body>\n</html>\n"}
```

### HTML5の木構造: 入れ子のnobr

- 入力: `<nobr>a<nobr>b</nobr>c</nobr>`
- 理由: HTML5はnobrの入れ子を解除する。

dictのPython出力:

```json
{"value":{"html":{"body":{"nobr":{"##value":"a\n   \n   c","nobr":{"##value":"b"}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"c","nobr":[{"##value":"a"},{"##value":"b"}]}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <nobr>\n   a\n   <nobr>\n    b\n   </nobr>\n   c\n  </nobr>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <nobr>\n   a\n  </nobr>\n  <nobr>\n   b\n  </nobr>\n  c\n </body>\n</html>\n"}
```

### HTML5の木構造: 2つ目のbody

- 入力: `<body id="first">a<body class="second">b</body>`
- 理由: HTML5は2つ目のbodyの属性を最初のbodyへ合流する。

dictのPython出力:

```json
{"value":{"html":{"body":{"##value":"ab","#id":"first"}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"##value":"ab","#class":"second","#id":"first"}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body id=\"first\">\n  ab\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body class=\"second\" id=\"first\">\n  ab\n </body>\n</html>\n"}
```

### HTML5の木構造: plaintext

- 入力: `<plaintext>a<b>b</b>`
- 理由: Pythonのdictは`__parent__`を残す循環参照になり、JSON保存でValueErrorになる。
- 観測: Pythonのdictは循環オブジェクトを返す。fixtureの`serialize: true`でJSON保存まで実行した結果を示す。dumpは一致する。

dictのPython出力:

```json
{"error":"ValueError"}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"plaintext":{"##value":"a<b>b</b>"}}}}}
```

### HTMLのbody開始前のtemplate

- 入力: `<template><p>x</p></template>`
- 理由: body開始前のtemplateをHTML5はheadへ配置する。

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <template>\n   <p>\n    x\n   </p>\n  </template>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <head>\n  <template>\n   <p>\n    x\n   </p>\n  </template>\n </head>\n</html>\n"}
```

dictのPython出力:

```json
{"value":{"html":{"body":{"template":{"p":{"##value":"x"}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"head":{"template":{"p":{"##value":"x"}}}}}}
```

### HTML/XMLの深さ上限

- RustはXMLを1000要素まで、HTMLをルートのhtmlを0とする深さ1000まで処理する。
  - HTMLの例: bodyが補完されるdivの入れ子は999個まで成功する。
  - エラー: XMLのdictはExpatError相当、dumpは`Include invalid token\n\n        {input}\n        `、HTMLは`LimitExceeded`を返す。
- PythonのXML dictは1001要素でも成功する。XML dumpは999要素以上の実測で`RecursionError`となる。
- PythonのHTML dumpは1001個のdivでも成功する。
- fixtureは深さ1000・1001の比較を明示する。
  - 入力生成: XMLは`"<r>" * depth + "x" + "</r>" * depth`、HTMLは`"<div>" * depth + "x" + "</div>" * depth`。
  - 出力要約: dictはrを辿った深さと末端値、dumpはinvalid token判定と文字数。巨大な整形文字列そのものはfixtureに格納しない。
  - 理由: Pythonの再帰上限とRustの明示上限は異なる。以下の4差異の扱いは未決。

- 入力パラメーター: `{"format":"xml","operation":"dict","summary":"depth","depth":1001}`
  - Python: `{"value":{"depth":1001,"leaf":"x"}}`
  - Rust: `{"error":"ExpatError"}`
- 入力パラメーター: `{"format":"xml","operation":"pretty","summary":"depth","depth":1000}`
  - Python: `{"error":"RecursionError"}`
  - Rust: `{"value":{"invalid":false,"length":4001027}}`
- 入力パラメーター: `{"format":"xml","operation":"pretty","summary":"depth","depth":1001}`
  - Python: `{"error":"RecursionError"}`
  - Rust: `{"value":{"invalid":true,"length":7048}}`
- 入力パラメーター: `{"format":"html","operation":"pretty","summary":"depth","depth":1000}`
  - Python: `{"value":{"invalid":false,"length":1017036}}`
  - Rust: `{"error":"LimitExceeded"}`

### HTML/XMLの残る検証範囲

- 全HTMLのlxml互換やDTD全体の互換を保証するものではない。
- XMLの一般エンティティ展開量は、本文と全属性で1つのカウンタを共有する。
  - 判定: 消費入力と間接展開の合計が8MiBを超え、消費入力の100倍も超えた場合にエラーにする。
  - 根拠: [Expatの増幅率と有効化閾値](https://libexpat.github.io/doc/api/latest/#XML_SetBillionLaughsAttackProtectionMaximumAmplification)。
  - 精度: 消費入力はquick-xmlのイベント終端で計測する。Expatのバイト単位の境界判定まで同一とは保証しない。
  - 検証: 7MB属性、9MB属性、属性と本文の合算、本文のみ、低増幅率の9MB属性をdictとdumpの10fixtureで比較する。
  - dict本文: xmltodictに合わせて一般エンティティ展開を無効にする。この経路の本文は展開量に加算しない。
  - 上限: エンティティ再帰は1000まで。循環はエラーとする。
- 外部DTD・外部エンティティはネットワークやファイルから読み込まない。
  - 互換: SYSTEM/PUBLIC付き外部DTDで未定義参照がある場合、dictは空にし、dumpはinvalid token文字列を返す。
- DTD内のパラメーターエンティティ宣言は読み飛ばす。
  - 境界: DTD全体やパラメーターエンティティの展開を実装したものではない。

### その他の未決

- clapが追加する `help` サブコマンドを残すか。
- clapの `-V` を残すか。
- `--version` の出力形式。
  - 現状: Python版は `7.0.0` のみ、Rust版の骨組みは `jumeaux 0.1.0`。
- 外部コマンドアドオンの設定・入出力・エラー・タイムアウトの契約と、`cls_name`・`init addon` の移行方法。
- retry用ヘッダ判定設定のreport内の保存位置と、その情報を持たない旧reportの扱い。
  - 根拠: Python版Reportには `judge_response_header` と `ignore_response_header_keys` がなく、旧reportだけから元の値を復元できない。保存形式の追加は本タスクで決定しない。
- Python固有のtracebackをRust版のstderrでどこまで一致させるか。
  - 実測: encoding_hellは `Sorter.conditions` のUnknownPropertiesErrorで終了コード1。reportとresponsesは生成されない。この結果を基準に保存し、成功する設定へ独断で修正しない。
- `encoding_hell` テンプレート自体を修正して同梱するか。
  - 検証: ハーネスの変種だけjson_sortを `when` へ直し、CSVに欠落しているGETも補う。同梱テンプレートの基準は失敗を維持する。

## 受入の段階

- CLI統合前は5コンポーネントのJSON fixtureを使う。
- CLI統合後は16テンプレートのreport、保存ファイル、stdout、stderr、終了コードとAPI記録を比較する。
- 通知は追加ケースでSlack v1・v2をローカル受信先に送って比較する。
- 既存不具合は `python_mismatch: true`、理由、Python観測値、修正後のexpectedを持つfixtureで検証する。
- ヘッダ除外の大小無視は、Pythonの観測ケースと正しい大小表記の対照ケースを両方採取する。
  - 規則: Pythonラッパーの自己比較は観測値と一致させる。Rustバイナリは対照ケースの出力を期待値とし、大小を変えた入力configで成功することを検証する。
- Viewer目視、実OSでのHTTP環境依存挙動、Miroir実接続は後続の全体受入で行う。
