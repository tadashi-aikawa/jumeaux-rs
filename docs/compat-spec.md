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
- output: `response_dir`、`encoding`。旧 `logger` の扱いは「意図的に変える差異」を参照する。
- 複数configは後のトップレベル値を優先する。addonsはレイヤーごとに更新し、リストは連結しない。
- CLI指定はPython版 `merge_args2config` と同じ条件で上書きする。
  - `threads`・`max_retries`: 未指定以外を上書きする。0も含む。
  - `processes`: 0と未指定は元設定を維持する。
  - `title`・`description`: 空文字と未指定は元設定を維持する。
  - `tag`・`files`: `TOption.any()`は値の有無だけを判定する。明示した空配列も上書きする。
  - コマンド境界: runはタグ未指定をNoneへ、retryは空配列へ変換する。retryのタグ未指定は元のタグを消す。
- Pythonの設定マージには、空のレイヤーを上書きしない例外がある。
  - 実測: `reqs2reqs: []`やタグ除外で全件消えたレイヤーは、前のconfigのレイヤーを維持する。
  - 契約: Python互換を維持する。明示した空配列でも前レイヤーをクリアしない。
- YAMLのアンカーとマージキー `<<` を展開してから設定をマージする。
  - 重複キー: トップレベルとアドオンconfig内のどちらも後勝ちにする。
  - 優先順位: 明示キーをマージ元より優先する。`<<` の値がリストなら先頭のマッピングを優先する。
- 既定値を持つフィールドは、キー欠落と明示的なnullのどちらでも既定値を使う。
  - 対象: 設定、モデル、各アドオンの型付きConfig。必須フィールドのnullはエラーとする。
- タグ除外は各configのinclude解決より前に行う。
  - 対象: log2reqs以外の8レイヤー。Pythonは単体のlog2reqsをタグ除外しない。
  - include: 宣言したconfigのディレクトリを基準に1段だけ解決し、アドオン指定全体を置き換える。include先のタグは再判定しない。
  - エラー: 未解決の再includeはキーを示して停止する。
- `--processes` は受理し、並列数として扱う。
  - 規則: 有効なprocesses指定があれば `concurrency.threads = 1`、`concurrency.processes = 指定値`。
  - 規則: processesがなければ `concurrency.threads = threads`、`concurrency.processes = 1`。threads既定値は1。
  - 例: `--threads 4 --processes 2` は並列数2、レポートは `{"threads":1,"processes":2}`。
  - 検証: 実際に使う並列数が正でなければ、そのキーを示して停止する。processesを使う場合はthreadsの値を使わない。

### 設定・モデル・アドオンの実装境界

- `config`: serde_yaml_ngによる読み込み、CLI上書き、concurrency生成。
  - 検証: includeをファイル単位で解決し、マージ後の設定だけを型付きモデルへデシリアライズする。後のレイヤーで差し替えられたアドオンの未知名・未知キーは検証しない。
  - 対象: トップレベル、endpoint、output、notifier、9レイヤー、各アドオンの入れ子の設定。任意JSON本文・ヘッダ名・query名は未知キー検査の対象外。
  - エラー: 未知名、Pythonモジュール名、`cls_name`のExecutor以外、Python transformerは設定パスを示して停止する。由来を追える項目にはconfigまたはinclude先のファイル名も付ける。
  - `output.logger`: 設定ファイルでは非null値を拒否する。nullと値のない `logger:` は未指定と同じ扱いにする。reportの非null値は警告として返して捨てる。
- `models`: Request、Response、HttpMethod、Trial、Report、Summary、ResponseSummary、Addons、設定の値オブジェクト。
  - 共有: DiffKeysは`diff`の型を再利用する。式入力用アダプターは`compat::values`に残す。
  - 移動: `plain_requests`は`addons::log2reqs`へ移す。既存のcompat入口は再exportしてfixtureを維持する。
- `addons`: 9レイヤー・34名の静的登録と、型付きStage・AddonExecutor・Factory・Chain。
  - 順序: 各レイヤーは設定配列の先頭からreduceし、戻り値を次へ渡す。エラーで停止する。
  - 設定: 各Config型は `addons/<layer>/<name>.rs` に置く。名前・設定型・生成は1つの登録表から選ぶ。`serde_path_to_error` で必須キー・型・未知キーを検証する。
  - 生成: FactoryへAddon全体を渡す。スケルトンは登録表から生成した型付きConfigを保持する。保存用のAddonではNoneと空configを区別する。
  - 実装済み: res2resのjson_sort・type、res2dictの4種、judgementの2種は各レイヤーの`Factory`から生成する。条件式と正規表現は生成時にコンパイルする。
  - 入力系: log2reqsのInputFactoryとreqs2reqsのPipelineは本体を実行する。条件式とempty_guardの通知テンプレートはPipeline生成時にコンパイルする。
  - 保存系: store_criterion/free、dumpの4種、did_challengeの2種も各レイヤーのFactoryから生成する。`executor::Pipeline`が実装済み6レイヤーを`Chain::build`で接続する。
  - 残件: final本体は後続タスク。SkeletonFactoryとExecutor::skeletonは副作用を持たない検証用入口として残す。
- `output.encoding` はreport、stdout、`summary.txt`、stderrへの直接書き込みに適用する。
  - ログ: Pythonのロガーは起動時のstderrを保持する。差し替え後もログ行はUTF-8で出力する。直接書き込みとtracebackは差し替えたwrapperを通る。
  - ストリーム: stdoutと直接書き込み用stderrはプロセス内でencodingごとにwriterを共有する。取得し直してもBOMやISO-2022-JPの状態をリセットしない。
  - ハンドル: `write_text(&self)`・`flush(&self)` の内部だけでロックする。同じスレッドでの再取得と別encodingの併用を受理する。
  - 変換: compatのstrictエンコーダーを使う。BOMは最大1回。Python同様、seek不能なパイプへのUTF-16・UTF-32出力はBOMなし、UTF-8-SIGはBOM付きとなる。
  - JSON: キーを再帰的にソートし、Pythonの区切り文字に合わせる。CLIへの接続とfinal/jsonのindent指定は後続タスク。

### 入力ファイルとreqs2reqs

- `addons::log2reqs::read`はバイト列、`InputFactory`はファイルを受け取る。
  - 接続: `addons::log2reqs::prepare(config, context)`がinput_filesを指定順に連結してからreqs2reqsを実行する。
  - codec: 全形式で`compat::encoding::decode_input`を使う。BOMの扱いと改行の正規化もこの境界で行う。
- plainは改行だけの空行を除外し、空白だけの行は末尾空白を除いてpathが空のRequestとして残す。
  - query: 最初の`?`の直後だけを解析する。`keep_blank`はquery値に適用する。
  - URL encoding: queryが存在する場合だけ候補を先頭からstrictで試す。候補が通らなければutf-8にする。
- csvは固定5列のname・method・path・qs・headersを読む。
  - dialect: excelとunixはカンマ、excel-tabはタブ。reader側のquote処理は3種とも同じ。
  - queryとheaders: UTF-8のparse_qs互換。重複ヘッダは最初の値を使う。欠落列のqsとheadersは空辞書にする。
  - 境界: 空行は除外する。引用符内改行・二重引用符・閉じない引用符はPythonのstrict=Falseと同じく受理する。余分な列はValueError、path欠落はRequiredError。
  - 上限: Pythonの既定値と同じく、fieldは131072文字までとする。
- JSONとYAMLはRequestの配列を読む。空のオブジェクトはPython同様に空配列となる。
  - キー正規化: Request直下のキーへOwlMixinのto_snake相当を適用する。`Path`は`path`、`urlEncoding`・`url-encoding`は`url_encoding`になる。正規化後の衝突は後勝ちとし、qs・headers・json・form内のキーは変換しない。
  - 未知フィールド: 正規化後に判定する。Pythonの入口に合わせてJSONは無視、YAMLはUnknownPropertiesErrorとする。
  - 空ファイル: JSONはJSONDecodeError、YAMLはValueError。pathの欠落・nullはRequiredError。
  - BOM: utf8指定のJSONはBOMを拒否し、YAMLは受理する。utf-8-sig指定なら両方とも除去する。
  - YAML: アンカーとマージキーはconfigと同じ展開処理を再利用する。
  - 非有限数: `.inf`・`-.inf`・`.nan`は入れ子を含めてValueErrorにする。JSONで表現できない数値をnullへ変換してキーごと省略しない。引用符で囲んだ文字列は保持する。
  - 整数: i64・u64の範囲を超えても、i128・u128で読める値はarbitrary_precisionのJSON numberとして桁を落とさず保持する。
  - 独自タグ: 値に付けた`!foo x`などのローカルタグはConstructorErrorにする。キーに付けた独自タグと標準タグの解釈差は「許容した差異」に記載する。
- reqs2reqsは設定順に実行する。filter・replace・renameのwhenはRequest.to_dict相当をルートへ渡す。
  - 検査: 条件式の前にRequestをCheckedContextへ変換する。json・form内の非有限数や、f64変換で非有限になる巨大整数はValueErrorにする。addで追加したRequestも後続条件式で同じ検査を通す。
  - 条件: 未設定のnameなどは未定義値になる。テンプレート描画後のliteral_eval相当の値をtruthinessで判定する。
  - head: 負数もPythonのslice終端として扱う。
  - repeat: 0と負数は空配列になる。
  - add: 既定はhead。tailは末尾へ連結する。
  - replace: 条件に合う全itemを順に適用する。後続条件は置換後のRequestを見る。queriesの値だけで$DATETIMEを展開し、headersでは展開しない。
  - rename: 最初に一致した条件だけを適用し、nameをテンプレート描画する。空文字のnameはNoneとして省略する。
  - shuffle: seed未指定時はランダムに並べ替える。比較用APIの`Config::apply(requests, Some(seed))`はu64整数seedのCPython random.shuffleと同じ順序を返す。設定ファイルにseedキーは追加しない。
- empty_guardは空配列を検出したら通知して終了コード1で止める。
  - 戻り値: asyncのPipeline::execはrequests・notifications・logs・exit_codeを返す。後続アドオンは実行しない。
  - ログ: Logはlevelとmessageを持つ。空入力のwarning、`Notify using slack@vN`のinfo、通知結果のログの順に返す。エラー時はFailure.logsにそれ以前のログを保持し、CLIはエラーより先に出力する。
  - テンプレート: config.to_dict(ignore_none=False)相当を渡す。入れ子の未設定フィールドもnullで公開する。
  - CLI: tokioのruntimeからblock_onで入力段階を実行する。infoは本文のみ、warningは黄色の`[WARNING]`、errorは赤色の`[ ERROR ]`でstderrへ出す。通常のrun・retry・initの全体実行は後続のCLI統合タスクで接続する。

### 通知基盤

- `notification::{payload, notify}`はSlack v1・v2を扱う。
  - v1: channel・username・icon_emoji・icon_urlを送る。icon_emojiの前後にコロンを付ける。
  - v2: use_blocks=falseはtext、trueはmessageをJSONとして解析してblocksへ渡す。`null`はblocksを省略し、配列直下のnullはRequiredErrorにする。SLACK_THREAD_TSが非空ならthread_tsを送る。
  - 共通: link_namesは1。Noneと任意JSON内のnullは再帰的に除外する。
- 通知のHTTP送信はasyncのreqwestを使う専用関数へ閉じる。
  - runtime: ライブラリ内でruntimeを生成・block_onしない。HTTP転送側のtokio runtime内からそのままawaitできる。クライアント共有は統合タスクで接続する。
  - 本文: Python同様にキーを再帰ソートし、UTF-8のJSON bytesを送る。Content-Typeは自動付与しない。
  - 結果: 基準Jumeauxのmodels.Response.okに合わせてHTTP 200だけを成功とし、それ以外の本文をOutcome.errorへ返す。通信例外はConnectionErrorとして返す。
  - User-Agent: 転送と同じ`jumeaux/<version>`を使う。
- SLACK_INCOMING_WEBHOOKS_URLが無い場合は送信をスキップし、3件のwarningログをOutcome.logsへ返す。
  - 検証順: notifierのversionを先に検証する。未対応のversionはWebhook未設定でも`Type slack@vN is not valid.`でエラーにする。
  - ログ: ライブラリは直接出力しない。表示とプロセス終了はCLI側の責務とする。
- 関数比較基準は`compat/gen/inputs.py`でPython実装を実行して採取する。
  - 検証: `compat/fixtures/inputs.json`の入力変換・reqs2reqs・ローカル通知を`tests/inputs.rs`で比較する。通知はpayloadだけでなく本文bytesとContent-Typeも照合する。

### reportの省略・空値・null

- TOptionのNoneはキーを省略する。
  - 読込: 文字列のTOptionは空文字もNoneになる。必須の文字列は空文字を保持する。
  - 空値: 空辞書と空配列は保持する。`diffs_by_cognition`の未設定・nullは省略、空辞書は`{}`となる。
- report内のモデルの未知キーは1回の走査で捨て、パス付きの警告を返す。
  - 対象: 入れ子のモデルと旧 `summary.output.logger`。任意JSON本文のキーは保持する。
  - 制限: 型の誤りと必須キー欠落はエラーとする。
  - API: `Report::from_value_with_warnings` と `Config::from_report`・`from_report_with_headers` はモデルと警告リストを返す。表示はCLI側の責務とする。
  - serde: 通常のReportデシリアライズは無出力で、警告リストを破棄する。CLIのreport読込は明示的な警告付きAPIに接続する。
- RequestとTrialの `json`・`form` はObjectに限定する。
- OwlMixinは任意JSON内のnullも再帰的に除外する。
  - 対象: dictのnull値とlist内のnull要素。Request・Trialのform/json、Addon.config、response_headerの保存にも適用する。
  - 保持: false、0、空文字、空辞書、空配列は任意JSON内では残る。メモリ上の入力は変更しない。
- 基準report31件は、モデルの読込・再出力がJSON値とbytesの両方で一致する。
  - 比較: 可変値を除外せず、UTF-16のoutput_encodingも含めて固定する。
  - テンプレート: 15種がreportを生成する。16番目のencoding_hellはPython側の失敗とreport不存在を検証する。
- `Trial::to_request`はretry用にrawも復元する。
  - 設定: `Config::from_report` は `summary.comparison` を使う。旧reportではPythonの既定値を補い、警告を返す。

### 関数とHTTPの契約

- DeepDiff 8.6.2の `type_changes`・`values_changed` をchanged、辞書とiterableの追加をadded、削除をremovedに写す。各パスはソートする。
- 差分パスは `root<'items'><0><'id'>` 形式とする。
- scalar配列の挿入・削除、重複、辞書の配列、入れ子配列、辞書の全面置換、数値型変更をfixtureで固定する。
- scalar配列は連続一致から差分を求め、差分が2件以上なら位置比較と件数を比べる。同数なら位置比較を採る。
  - 型比較: 通常は `1` と `1.0`、`true` と `1` の変更を検出する。scalar配列の連続一致ではPythonの等値判定を使うため、これらが一致扱いになる場合がある。
  - index: changed・removedは変更前、addedは変更後のindexを使う。
- 辞書のキー集合の和が2件以上で、共通キー率が0.33未満なら辞書全体をchangedにする。
  - 除外: `__`で始まるキーはDeepDiffの既定設定に従い比較対象外とする。
- JSONの整数は桁を落とさず保持し、多倍長整数として比較する。小数・指数表記はPythonと同じくf64として比較する。
  - 例: `18446744073709551616` と `18446744073709551617` はchanged、`1e2` と `100.0` は同値。
  - 実装: serde_jsonの `arbitrary_precision` を有効にする。通常の `Value` の等値比較は数値リテラルの表記に依存するため、差分判定には使わない。
  - 保存への影響: 大きな整数はJSON numberのまま往復できる。小数・指数の再出力はserde_jsonで正規化した表記とし、「許容した差異」で固定する。
- HTMLはBeautifulSoup+lxmlの補完とprettify後のdictを基準にする。
  - 内容: 属性 `#attr`、本文 `##value`、同名タグのリスト、空タグ、script、混在テキストの空白。
- XMLはxmltodictのdictを基準にする。
  - 内容: 属性 `@attr`、本文 `#text`、空要素null、同名要素のリスト、名前空間、CDATA、エンティティ。
- APIは`markup::html::{to_dict, pretty}`と`markup::xml::{to_dict, pretty}`。
  - 入出力: デコード済み文字列を受け取り、dictはJSON値、prettyは末尾改行を含む文字列を返す。
  - 境界: res2dictがencoding、force、適用判断を担当する。CLI接続は後続タスク。
- table内テキストの移動と不正なインラインタグの複製は修正済み。
  - 検証: 以前の4差異fixtureもPython期待値と一致する。
- コメント前後の空白とSVG・MathML名の大小は修正済み。
  - 検証: 以前の3差異fixtureもPython期待値と一致する。空コメントは「許容した差異」に記載する。
- 条件式は描画後の `ast.literal_eval` に相当する値を返す。文字列描画と区別する。
  - 例: 描画された `False` はboolのfalse。未定義値の単独条件評価はエラー。
- 独自拡張は `reg`、`calc_distance_km`、`equals_without_host`、`$DATETIME` を含む。
- `Response.text`、`Response.ok` などの公開プロパティも式への入力契約とする。
- HTTPはCookie共有、gzip展開、リダイレクト、proxy環境変数、証明書検証、リトライ対象、通信失敗分類を全体受入の対象とする。
- HTTP記録はメソッド、URL、ヘッダ、本文bytes、同一リクエストの回数を比較する。
  - 条件: 並行したone/other間の到着順に依存せず、多重集合と回数で検証する。

### 文字コード

HTTP転送のPython実測とRustへの接続状況は「HTTP転送の接続契約」に記す。

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

### 応答変換とjudgement

- 実行入口は`addons::res2res::Factory`、`addons::res2dict::Factory`、`addons::judgement::Factory`。
  - 接続: `Chain::<Res2Res>::build(&config.addons, &res2res::Factory)`の形でレイヤーごとに構築する。
  - 設定検証: 条件式と正規表現はFactory生成時にコンパイルする。未実行分岐の不正な設定もエラーとする。
- res2res/typeは最初にwhenが真になる条件だけを適用する。
  - 省略: whenなし・空文字は無条件にする。
  - 式入力: `req`と`res`はPythonの`to_dict()`相当。`res.text`・`res.ok`などのプロパティは公開しない。`res.headers`の参照は大小を無視する。
- json_sortはtypeが`json`のときだけ適用する。
  - 条件: itemsを設定順に処理し、whenにはRequestの`to_dict()`をルートとして渡す。
  - 対象: 子の配列から先に処理する。targetsで最初にpathが完全一致した設定だけを使う。
  - ソート: sort_keysは複数キーの辞書順。未指定のdict・list要素は挿入順を保ったJSON文字列をキーとする。同じキーの順序は保つ。
  - 数値: 多倍長整数の比較では丸めない。boolと数値、整数と小数の等値はPythonに合わせる。
  - 本文: `json.dumps(ensure_ascii=False)`の区切り空白と小数表記に合わせ、元のencodingでreplaceエンコードする。項目なし・whenがすべて偽でも再シリアライズする。
  - タグ: JSON値の内容・配列順が変わったときだけfootprints_tagを末尾へ追加する。既存タグとの重複は除かない。
  - パス: json_sortはPython同様にキーをそのまま`root<'key'>`へ埋め込む。DeepDiffのエスケープ済みパスとは区別する。
- res2dictのjson・xml・htmlはResponse.typeの完全一致で適用する。
  - Content-Type: HTTP側でtypeへ変換した結果を使う。アドオン内ではContent-Typeを再解釈しない。`problem+json`や`JSON`を`json`とは扱わない。
  - force: trueならtypeを無視して変換し、既存resultも上書きする。
  - スキップ: resultが既にあればforce=falseでスキップする。空dict・空listも既存resultとみなす。
  - 空値: JSONのルートnull・空文字はresultなしとなる。入れ子のnullと、ルートのfalse・0は保持する。
- blockはtypeを問わず変換する。
  - 分割: Pythonのsplitlinesと同じ改行を使い、空行と末尾でレコードを確定する。空白だけの行は区切りにしない。
  - 正規表現: header・recordのfindallがちょうど1件の行だけ採る。headerを先に判定する。
  - グループ: recordが複数グループなら先頭2つをキーと値にする。1グループまたはグループなしならマッチ文字列の先頭2文字を使う。値がなければnull。
  - 上書き: 同じレコードキー・ブロック名は後勝ち。ヘッダがないブロックのJSONキーは`"null"`。
- judgementの初期値は`judgement::initial_payload`で組み立てる。
  - エラー伝播: `initial_payload`と`diff::diff`は`compat::Result`を返す。呼び出し元は`?`で入力エラーを伝播する。
  - 停止: `initial_payload`のErrは他のアドオンのエラーと同じく実行を停止する。LimitExceededは本文bytesが同じ場合にも返る。
  - 変換なし: 片側でもdictなしならdiffs_by_cognitionは未設定。本文bytesで比較する。
  - 変換あり: 本文bytesが同じなら差分計算を省略する。それ以外は`diff::diff`でunknownを組み立てる。
  - 空差分: unknownが空なら空dictを保持し、regard_as_same_bodyをtrueにする。
  - ヘッダ: 判定有効時は名前と除外キーのASCII大小を無視する。値は完全一致。無効時はregard_as_same_headerをtrueにする。
- ignoreとsameはregard_as_same_bodyがtrueなら処理をスキップする。
  - ヘッダ: どちらもregard_as_same_headerは変更しない。最終sameはbodyとheaderの両方がtrueの場合だけ。
  - 式入力: reqはname・path・qs・headers。res_one・res_otherはResponseオブジェクト。dict_one・dict_otherとres_one.encoding等のOptionは、`value`属性と`get`・`get_or`・`is_none`メソッドを公開する。
  - Pythonの公開IF: owlmixin 7.0.1のTOptionは`any`・`flat_map`・`get`・`get_or`・`is_none`・`map`・`value`を持つ。`is_some`は持たない。
  - Rustの残件: `any`・`flat_map`・`map`は後続タスクで実装する。
    - 決定: 2026-11-08。
    - 現状: 実装までは式から呼ぶとエラーになる。
  - 入力検査: dictはres2dictとinitial_payloadで検査する。Option生成時の`values::optional`は数値変換中に非有限値を拒否し、Resultを返す。whenごとにdict全体を検査し直さない。
  - whenの入口: TemplateInputは検査済みの`CheckedContext`とその参照だけに実装する。生のserde_json::Value・MiniJinja Valueをwhenへ直接渡すとコンパイルエラーになる。
  - 構築: JSONは`values::from_json_for_when`、MiniJinja値は`values::checked_context`で一度検査する。検査済みの値の結合には`CheckedContext::from_pairs`を使い、各whenでは再検査しない。renderは別のRenderInputで従来の表示互換を保つ。
- ignoreはunknownの差分をtitleへ移す。
  - 条件: conditions[].whenが偽なら、その条件から分類を作らない。addedだけの条件はchanged・removedに影響しない。
  - 値参照: caseのpathが完全一致したあと、差分パスから引いた値をone・otherとしてcase.whenへ渡す。片側に値がなければNone。
  - 順序: 同じignore内の全条件は分類前のunknownを参照する。条件間で重複一致したパスはtitle内にも重複して残る。次のignoreは分類後のunknownを参照する。
  - 統合: 同じtitleは連結する。最後に空の分類を除き、unknownが空ならregard_as_same_bodyをtrueにする。
  - 未設定と空: diffs_by_cognition未設定はスキップ。空dictかつbody=falseを直接渡すとPython同様KeyError。初期化関数が作る空dictはbody=trueなのでスキップする。
  - 特殊キー: 既に決定した差分パス修正を適用する。Pythonで値取得できなかった7ケースはPython観測値とRust期待値を別々に保持する。
    - 追加ケース: パス`root`単独はPythonでNone、Rustで入力全体。キー`rooted`はPythonで値なし、Rustでキーの値を取得する。
- sameはwhen_anyの最初の真でregard_as_same_bodyをtrueにする。
  - 短絡: 以降の式は評価しない。diffs_by_cognitionはそのまま保持する。

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
  - 比較対象: config・Requestに明示したヘッダと、Content-Type・Accept・Accept-Encoding・Connection・Content-Length・Cookie・User-Agent。
  - 規則: ヘッダ名は大小無視、並び順も無視する。値は完全一致とする。
  - 比較対象外: 自動付与のHost。明示した場合は比較する。
  - User-Agent: 自動値は製品名を維持しバージョン部分だけ正規化する。明示値は完全一致とする。
  - 通知の例外: User-Agentは比較しない。`notification_header_exclusions`で通知記録だけに適用する。API記録の比較は維持する。
  - 同一URL: trialごとの明示値の組を保持する。別trialの明示User-Agentを自動値の比較へ適用しない。
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

- trial例外後の後続trialへの着手数はPythonと一致させない。
  - Python版: エラーが入力順のresult_iteratorへ届くまで、worker数に応じて後続trialへ先行着手することがある。
  - Rust版: Errを受け取った時点で新しいspawnを止める。既に実行中のtrialは保存・sleepまで完了を待つ。
  - 比較: 停止時の送信件数・保存件数の差を許容する。入力順の最初のエラーを返す契約は維持する。
- `.netrc`由来のAuthorizationは自動付与しない。
  - Python版: trust_envがtrueならnetrcから認証情報を取得する。
  - Rust版: 設定・Requestで明示したAuthorizationを使う。
- 通知のUser-Agentを`jumeaux/<version>`にする。
  - Python版: `python-requests/<version>`を送る。
  - 比較: 通知のUser-Agentだけを除外し、他の自動ヘッダと本文は比較する。
- RustのOptionは`is_some()`を追加で公開する。
  - Python版: TOptionには存在しない。互換の値参照には`value`・`get()`・`get_or()`を使う。
- 非有限数はRustで表せない入力として拒否する。
  - 対象: `NaN`・`Infinity`・`-Infinity`と、`1e400`等のf64オーバーフロー。配列・辞書内部も同じ扱い。
  - エラー: json_sort・res2dict/json・dump/json・whenの入力値と結果・diffは、`ValueError: non-finite numbers are not supported`を返す。
  - Python版: 非有限floatとして受理する。fixtureは非有限値を`nonfinite`タグに置き換えて観測値を保存する。
  - 連結: 旧Rust実装はjson_sortが`Infinity`を書き出し、後続res2dict/jsonが拒否する場合があった。現在はjson_sortの入口で拒否し、この出力を作らない。
  - 整数: JSONの整数はBigInt相当の精度で保持する。when入力へのf64変換で非有限値になる巨大整数も、whenの入口で同じエラーとする。
  - 小文字: JSON中の`nan`・`inf`・`infinity`はPython同様JSONDecodeErrorとする。whenの描画結果に現れる非有限floatの`nan`・`inf`はValueErrorとする。
- 保存I/Oの汎用例外名はRustで`IOError`、Pythonで`OSError`となる差を許容する。
  - パス: ディレクトリ作成・本文とpropの書き込み・latest操作のエラーには対象パスを含める。OSのエラーメッセージ本文は完全一致の対象外とする。
- lone surrogateを含むJSONはRustで表せない入力としてJSONDecodeErrorにする。
  - 例: `"\ud800"`。Pythonは受理し、json_sortではエンコード時に`?`へ置換する。
- JSONの配列・辞書の深さ上限は1,000コンテナとする。
  - 読み込み: serde_jsonの既定の128段制限を解除する。文字列内のブラケットを除外して構築前に検査し、1,001段は`LimitExceeded`にする。
  - 再帰処理: 深さ64超の読み込み・json_sort・diffは32MiBの専用スタックで実行する。直接渡されたValueにも上限を適用する。
  - Python版: ランタイムの再帰上限に依存する。Rustは固定上限とし、配列・辞書・混在の1,000段を検証する。
- Slack v1もWebhook未設定時は警告して送信をスキップする。
  - Python版: SLACK_INCOMING_WEBHOOKS_URLが無いとKeyErrorになる。
  - Rust版: v2と同じ3件のwarningログを返す。ライブラリから直接ログを出さない。
- clapが追加する `help` サブコマンドと `-V` は残す。
  - 条件: Python版にない入力を受理する追加とし、Python版で通る入力の挙動は変えない。
- `--version` の出力はclapの既定のままとする。
  - Rust版: `jumeaux 0.1.0`。
  - Python版: `7.0.0` のみ。
- Rust版は同梱テンプレート `encoding_hell` の不具合を直す。
  - 修正: `res2res/json_sort` の `conditions` にある廃止済みの `path` キーを `when` へ置き換え、`requests` にメソッド列 `GET` を補う。
  - 実装時期: 後続の「Report集計・final・CLI」タスクで、initの同梱テンプレートを置くときに修正する。
  - 基準: Python版リポジトリは変更せず、同梱テンプレートの失敗を観測値として維持する。
- Rust版はinitのテンプレート名一覧をソート順で出す。
  - Python版: ファイルシステムの列挙順で出す。一覧の並びは互換対象にしない。
  - 比較: `✨ [Valid names] ✨` 直後のテンプレート名の区間だけを順序なしの行として比較する。名前の欠落・追加・重複は許容しない。
- retry再構築で `raw`、`judge_response_header`、`ignore_response_header_keys` が失われる不具合を直す。
  - 検証: Pythonの観測値と修正後の期待値を分ける。欠落したままの実装は合格にしない。
  - 保存: `summary.comparison: {judge_response_header, ignore_response_header_keys}` を追加する。
  - 旧report: comparisonがなければ `false` と `["Content-Length", "Date"]` を補い、警告を返す。
  - モデル: comparisonは省略可能とし、旧reportの単純な読込・再出力では追加しない。retry用Config生成時に補完する。
  - 接続: `executor::Execution.comparison`に`Config::comparison()`の結果を返す。後続のReport組み立てで`summary.comparison`へ保存する。
- 差分パスの値取得でキーに `root` やブラケットなどを含む場合の不具合を直す。
  - 検証: キーとindexの構造を辿った値を期待値にする。
  - 生成: キー内部の `[`・`]` を置換しない。引用符・バックスラッシュ・制御文字はエスケープし、生成したパスから元のキーを参照できるようにする。
  - 値取得: `root`単独は入力全体を返す。存在しないキー・index・不正な構文は値なし、JSON nullはnull値として区別する。
- レスポンスヘッダ名は小文字へ正規化してよい。
  - 規則: 除外設定と条件式ではヘッダ名の大小を無視する。本文・ヘッダ値・通常JSONキーには適用しない。
- Rust版で解釈できない設定はエラーで停止する。
  - 規則: エラーに直すべき設定キーを含める。警告だけで無視しない。
  - 旧logger: 設定ファイルの `output.logger` は `{}` を含む非null値をエラーとする。nullと値のない `logger:` は警告なしで通す。report由来の非null値は破棄し、警告を返す。
  - 空設定: Configを持たないPythonの `reqs2reqs/shuffle`・`final/viewer` でも、Rustは未知の設定キーを拒否する。
- `log2reqs: {}` と入れ子のincludeはエラーにする。
  - Python版: 空のlog2reqs指定を無視する。3段のconfigマージでは残ったincludeを後続config基準で再解決することがある。
  - Rust版: 空のlog2reqsでは必須nameの欠落を通知する。includeの解決は宣言元で1段に限定する。
- Pythonカスタムアドオンと `res2res/json` のPython transformerは外部コマンド方式で代替する。
  - 規則: 機能は廃止しない。Pythonモジュール名を暗黙に直接実行しない。
- 並列実装方式は変えてよい。concurrencyの出力は前述の規則を保つ。
- `head.size: true` など、boolをintとして渡す設定はRustでは型エラーにする。

## 許容した差異

以下はPython版と一致しない差異として許容したもの。

- `Content-Encoding: gzip, identity`をRustではgzip展開する。
  - Python版: この組合せはContentDecodingErrorになる。
- `Content-Type: text/html; charset`のような`=`のないcharsetをRustでは無視する。
  - Python版: boolのパラメータ値にstripを呼び、AttributeErrorになる。
- `charset=\t'utf-8'`のタブと引用符をRustでは除き、`utf-8`を選択する。
  - Python版: 先頭タブと片側の引用符が残ったcodec名を保持する。
- ループバックのHTTPへSecure付きCookieを送ることを許容する。
  - Python版: HTTPでは送らない。Rustのcookie_storeはループバックをsecureな宛先として扱う。
- macOSのシステムproxy設定は取得しない。
  - Rust版: AccessPoint.proxyまたはproxy環境変数を使用する。
- Optionをそのまま条件式に置く`when_any: ["dict_one"]`は、PythonでSyntaxError、Rustで偽となる。
- ignoreのcasesはRustでは最初の一致で評価を打ち切る。
  - Python版: 全caseを評価してから判定する。差は後続caseのエラーが発生するかどうかだけで、正常時の分類は同じ。
- whenへ渡すi128超の整数はf64へ丸める。json_sortは整数を丸めず、整数`-0`はPythonと同じ`0`へ正規化する。
- blockのtupleヘッダはJSONへ保存できない。
  - Python版: アドオン実行は成功し、prop fileのJSON保存時にTypeErrorとなる。
  - Rust版: 保存前のblock実行時にTypeErrorとなる。fixtureは保存時の失敗までを境界契約とする。
- JSON numberはserde_jsonで正規化した表記を使う。
  - 例: `1e2` は `1e+2`。Pythonの `100.0` とは一致しない。`1.2300` はRustで `1.2300`、Pythonで `1.23`。
  - 検証: RequestとReportの数値表記fixture2件で固定する。大きな整数 `18446744073709551617` は両者で一致する。
- YAML 1.1固有の数値表記を受理しない。
  - 例: Pythonの `010` は8、`1_000` は1000となる。Rustの数値フィールドではエラーとする。
- YAML入力のjson・form本文はYAML 1.2相当の文字列解釈を許容する。
  - fixture: Pythonの値と型をexpected、Rustの値と型をrust_expectedへ保存する。

| 本文内の入力値 | Python版 | Rust版 |
| --- | --- | --- |
| `yes` | boolのtrue | 文字列`yes` |
| `010` | intの8 | 文字列`010` |
| `1_000` | intの1000 | 文字列`1_000` |
| `12:30` | intの750 | 文字列`12:30` |
| `2020-01-01` | datetime.date | 文字列`2020-01-01` |
| `1e2` | 文字列`1e2` | floatの100.0 |
| `!!binary aGk=` | 文字列`b'hi'` | 文字列`aGk=` |
| `.inf`・`-.Inf`・`.nan` | 非有限float | ValueError |

- YAML入力では次の寛容さ・キー解釈の差を許容する。
  - 独自タグ: 値に付けたタグはRustもConstructorErrorにする。

| 入力例 | Python版 | Rust版 |
| --- | --- | --- |
| `path: !!python/str a` | ConstructorError | 文字列`a`を受理 |
| `qs: {a: [010]}` | InvalidTypeError | 文字列`010`として受理 |
| `headers: {on: x}` | キー`true`に変換 | キー`on`を保持 |
| `json: {!foo k: v}` | ConstructorError | `json`本文を`{"k": "v"}`として受理 |

- YAML入力のi128・u128の範囲を超える整数は、Rustではf64へ丸める。
  - 例: `-170141183460469231731687303715884105729`、`2^128`。
  - Python版: 正確な整数として保持する。
  - 理由: serde_yaml_ngがこの範囲の整数をf64として渡す。

- 通知blocksの`[NaN]`はRustではJSONDecodeErrorにする。
  - Python版: json.loadsがNaNを受理する。応答変換と同様、RustはJSONで表現できない値を受理しない。
- CSV入力では、複数の不備がある場合のエラー検出順を一致させない。
  - Python版: 全行を辞書へ読み込んでからRequestへ変換するため、後続行の列数エラーが先行行のモデル検証エラーより先になる場合がある。
  - Rust版: 各行を順にRequestへ変換する。field長・dialect・codecの同時不備も先に検出するエラーが異なる場合がある。
- f64の指数表記はPythonと一致させない。
  - 例: Pythonの `1e-05` に対しRustは `0.00001`。
  - 影響: 現在の時間値は小数2桁に丸めるため、この差異は出ない。
- `did_challenge/sleep` の `min`・`max` は、Rustでは整数も受理する。

HTML/XMLと式・正規表現・文字コードの差異は2026-11-07の質問票で許容した。

- Q1: HTML/XMLの42件と、以下に記載する互換範囲の制限を許容する。
- Q2: テンプレート式・正規表現・文字コードの65件と、以下に記載する互換範囲の制限を許容する。
- 実利用で問題になったときに個別に再検討する。
- fixtureはPython期待値と`rust_expected`の両方を保持し、黙った除外にしない。
  - HTML/XML: Python期待値は`compat/fixtures/markup/cases.json`、Rust期待値は`tests/markup_differences.json`の`expected`に保持する。
  - 文字コード推定名: `detect-euc`はテスト内でPython期待値とRust結果を明示検証する。
- 既存の互換アダプターによる補正と、一致するfixtureの検証は維持する。

- 文字コード推定名の差異。
  - 方針: 推定したencoding名はPythonのcodec名との一致を保証しない。EUC-JPの本文写像は合わせる。
  - 実測: `encoding/detect-euc`はPython charset-normalizerが`euc_jis_2004`、Rust chardetngが`EUC-JP`を返す。元の期待値は変えず、Rustの差異をテスト内で明示する。
  - 制限: chardetngはJIS X 0213のcodecを推定しない。同名への機械的な置換で一致扱いにはしない。
  - 詳細: [文字コードの互換境界](encoding.md)
- encoding_rsのencodeを直接使う場合の差異。
  - 差異: 表現できない文字はPythonの`?`と異なり数値文字参照になる。UTF-16のencodeも提供しない。
  - 現実装: 互換アダプターで`?`置換とUTF-16のendian・BOMを補い、fixtureでは一致する。この補正は維持する。
- Python固有codecと、Encoding Standardの対応表の外側の扱い。
  - 現実装: EUC-JIS-2004等はLookupError。UTF-32には専用アダプターを置く。主要codecの対応範囲・別名は[文字コード表](encoding.md#codecと別名)に固定する。
  - 差異: Pythonが処理できる`koi8-r`・`gbk`も専用対応がないためLookupError。WHATWGへのfallbackは行わず、fixtureで拒否を固定する。
  - 制限: 主要codecでも全Unicode対応表・全不正bytes列の網羅互換は未検証。
- EUC-JP・CP949・ISO-2022-JPのdecodeの差異。
  - EUC-JP: JIS X 0212の`8FA2B7`はPythonで`~`、Rustで`〜`。末尾の不完全な`8FA2`をreplaceで読むとPythonはU+FFFD 1文字、Rustは2文字。
  - CP949: `8180`・`A180`・`C680`をreplaceで読むとPythonはU+FFFD 2文字、Rustは1文字。読み直し位置が異なる。
  - ISO-2022-JP: NEC拡張のreplaceはPythonのU+FFFD 1文字に対しRustは2文字。`ESC ( I 31 ESC ( B`はPythonのU+FFFD＋`1`に対しRustはU+FFFD 3文字＋`1`。
  - 検証: 上記の差異は各fixtureの`rust_expected`で固定する。strictの受理・拒否がすべて一致するとは保証しない。
- ISO-2022-JPの生のSO・SI・ESCの差異。
  - strict encode: PythonはU+000E・U+000F・U+001Bをそのままbytesへ変換する。RustはUnicodeEncodeError。
  - strict decode: Pythonは単独の0E・0Fをそのまま文字へ変換する。RustはUnicodeDecodeError。
  - 境界: 単独の1Bのdecodeは両者ともUnicodeDecodeError。encodeの受理とは異なる。
- WHATWGの`replacement`へ写るcodecを拒否する差異。
  - 実測: Pythonの`hz-gb-2312`と`iso-2022-kr`はASCIIを変換できる。Rustは読み込み時にLookupErrorとする。strict・replaceのencode/decodeをfixtureで固定する。
- 未定義値・異種比較の差異。
  - 制限: MiniJinjaにPython相当の独自Undefinedを差し込めない。
  - 実測: 下表の結果をfixtureの`rust_expected`で明示検証する。

| fixtureの式 | Python | Rust |
| --- | --- | --- |
| `missing > 1` | UndefinedError | False |
| `missing\|length` | 0 | TypeError |
| `s > 1` | TypeError | True |
| `nil < 1` | TypeError | True |
| `1 / 0` | ZeroDivisionError | TypeError |

- ロード時の未知filter/test検査の差異。
  - 実測: Pythonは未実行分岐内の未知名を許容する。RustはTemplateAssertionErrorを返す。
  - 根拠: レビュー再依頼のロード時検出要件。Python期待値を変更せずfixtureに残す。
- `$DATETIME`の`int()`と`strftime`の差異。
  - 実測: 秒の`1_000`と全角数字はPythonが受理し、RustはValueError。`|int`・`|float`のunderscore対応とは別経路。
  - 実測: `%Q`はPythonが文字列`Q`を返し、RustはValueError。`%3f`はPythonが`3f`、Rustが`000`を返す。Pythonの結果は採取環境macOSのstrftimeに依存する。
  - 差異: 実時刻経路の`%Z`はchronoのoffset名を使うため、Pythonの`JST`等の略称とは一致しない。
- 空白制御・repr・例外種別の差異。
  - 実測: `when`の`x +`はPythonでTemplateSyntaxError、Rustで`x`の値になる。
  - 実測: `(1, 2)`と`range(3)`の描画はRustでリスト表記になる。非表示文字U+200BはPythonの`\\u200b`に対しRustは文字をそのまま描画する。
  - 制限: ZeroDivisionError等のPython例外種別を網羅的には対応付けない。
- 連結・round・abs・リスト乗算・groupbyの差異。
  - 連結: `"n=" ~ 1e20`はPythonで`n=1e+20`、Rustで`n=100000000000000000000.0`。MiniJinja内部の連結はformatterを経由しない。
  - 表示の対応範囲: 直接描画・`string`・`join`・`tojson`ではPythonのfloat reprを使う。`tojson`は各階層のキーをソートする。
  - round: `1|round(2, "floor")`の描画はPythonで`1.0`、Rustで`1`。
  - abs: `true|abs`はPythonで`1`、RustでTypeError。
  - リスト乗算: `[1, 2] * 1.0`はPythonでTypeError、Rustで`[1, 2]`。
  - groupby: 大小を無視したgroupのキーはPythonが最初の値の表記`A`を保つが、Rustは`a`へ正規化する。groupのtupleもRustではリスト表記になる。
- 正規表現のサポート範囲の差異。
  - 明示拒否: POSIXクラス、文字集合の積・差・対称差に見える`&&`・`--`・`~~`、条件付きgroup、`\N{…}`。Pythonで構文として通る例もPatternErrorとする。
  - 実測: Pythonの`\s`はU+001C〜U+001Fにも一致する。Rustは一致しない。
  - 検証: Pythonでも不正なRust固有escape・group名構文の拒否、Pythonで有効な`[[]`・8進escape・`(?a)`の受理もfixtureに含める。
- 正規表現の繰り返し・未閉鎖group参照・フラグの構文差異。
  - Pythonが拒否、Rustが受理: `^*`、`(?P<n>a(?P=n))`、`(?au)a`。未閉鎖groupの自己参照はRustでも採取した`aa`には一致しない。
  - Pythonが受理、Rustが拒否: `(?:)*`、`(?#c)(?i)a`。RustはPatternError。
  - 検証: 5ケースのPython期待値を保持し、`rust_expected`でRustの現出力を固定する。
- i128を超える整数のf64変換による差異。
  - 経路: JSON入力をテンプレート値へ変換するとき、2^127以上の整数はf64になる。Pythonの整数精度を保持しない。
  - 実測: 2^127と2^127+1の描画はRustでどちらも`1.7014118346046923e+38`。Pythonは異なる整数の10進表記を保持する。
  - 境界: 2^127−1はi128のまま描画し、Pythonと一致する。
- HTML/XMLの先行実装では221fixture中179件がPythonと完全一致する。
  - 差異: 以下の42件を許容する。`tests/markup_differences.json`にRustの現出力を固定し、Python期待値との不一致も検証する。
  - 維持: 2周目レビュー前の203件のPython期待値は変更していない。

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

### HTMLのDOCTYPEなしでのp・table

- 入力: `<p>a<table><tr><td>x</td></tr></table></p>`
- 理由: DOCTYPEなしのHTML5 quirksモードではtable開始時にpを閉じない。

dictのPython出力:

```json
{"value":{"html":{"body":{"p":{"##value":"a"},"table":{"tr":{"td":{"##value":"x"}}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"p":{"##value":"a","table":{"tr":{"td":{"##value":"x"}}}}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n  </p>\n  <table>\n   <tr>\n    <td>\n     x\n    </td>\n   </tr>\n  </table>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <table>\n    <tr>\n     <td>\n      x\n     </td>\n    </tr>\n   </table>\n  </p>\n </body>\n</html>\n"}
```

### HTML 4.01 Transitionalでのp・table

- 入力: `<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN"><p>a<table><tr><td>x</td></tr></table></p>`
- 理由: HTML 4.01 Transitionalのシステム識別子なしDOCTYPEはquirksモードになり、table開始時にpを閉じない。

dictのPython出力:

```json
{"value":{"html":{"body":{"p":{"##value":"a"},"table":{"tr":{"td":{"##value":"x"}}}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"p":{"##value":"a","table":{"tr":{"td":{"##value":"x"}}}}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n  </p>\n  <table>\n   <tr>\n    <td>\n     x\n    </td>\n   </tr>\n  </table>\n </body>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <p>\n   a\n   <table>\n    <tr>\n     <td>\n      x\n     </td>\n    </tr>\n   </table>\n  </p>\n </body>\n</html>\n"}
```

### HTMLの開いた要素内にあるbody終了タグ

- 入力: `<body><div>a</body>b</div>`
- 理由: body終了後も開いた子要素へのテキスト追加はhtml直下へ移されない。

dictのPython出力:

```json
{"value":{"html":{"##value":"b","body":{"div":{"##value":"a"}}}}}
```

dictのRust出力:

```json
{"value":{"html":{"body":{"div":{"##value":"ab"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <body>\n  <div>\n   a\n  </div>\n </body>\n b\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <body>\n  <div>\n   ab\n  </div>\n </body>\n</html>\n"}
```

### HTMLの自己終了head後のtitle

- 入力: `<head/><title>t</title>`
- 理由: HTML5はhead終了後のtitleをheadへ戻す。

dictのPython出力:

```json
{"value":{"html":{"head":{},"title":{"##value":"t"}}}}
```

dictのRust出力:

```json
{"value":{"html":{"head":{"title":{"##value":"t"}}}}}
```

prettyのPython出力:

```json
{"value":"<html>\n <head>\n </head>\n <title>\n  t\n </title>\n</html>\n"}
```

prettyのRust出力:

```json
{"value":"<html>\n <head>\n  <title>\n   t\n  </title>\n </head>\n</html>\n"}
```

### XMLのATTLIST既定値中の不正な文字

- 入力: `<!DOCTYPE r [<!ATTLIST r a CDATA "<x>">]><r/>`
- 理由: ATTLIST宣言の既定値を解釈しないため、値に`<`を含む不正な宣言を拒否しない。

dictのPython出力:

```json
{"error":"ExpatError"}
```

dictのRust出力:

```json
{"value":{"r":null}}
```

prettyのPython出力:

```json
{"value":"Include invalid token\n\n        <!DOCTYPE r [<!ATTLIST r a CDATA \"<x>\">]><r/>\n        "}
```

prettyのRust出力:

```json
{"value":"<?xml version=\"1.0\" ?>\n<r/>\n"}
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
  - 理由: Pythonの再帰上限とRustの明示上限は異なる。以下の4差異も許容する。

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
  - 互換: SYSTEM/PUBLIC付き外部DTDの未定義参照は、属性ではdict・dumpとも読み飛ばす。本文ではdictは読み飛ばし、dumpはinvalid token文字列を返す。
  - 検証: `external-dtd-attribute`は属性の`before&missing;after`が`beforeafter`となることを比較する。本文の既存fixtureも継続して検証する。
- DTD内のパラメーターエンティティ宣言は読み飛ばす。
  - 境界: DTD全体やパラメーターエンティティの展開を実装したものではない。

## 式へ渡すRequestの形

| addon | Pythonの入力 | Rustの入口 | Noneの扱い |
| --- | --- | --- | --- |
| res2res/json | `{"req": req, "res": res}` | `req.to_template_value()`を`req`キーへ渡す | `req.raw.is_none()`・`req.name.get()`等のOptionメソッドを公開 |
| reqs2reqs/filter | `request.to_dict()` | `request.to_dict_value()`をルートへ渡す | Noneのキーは欠落 |
| reqs2reqs/rename | `request.to_dict()` | `request.to_dict_value()`をルートへ渡す | Noneのキーは欠落 |
| res2res/json_sort | `payload.req.to_dict()` | `request.to_dict_value()`をルートへ渡す | Noneのキーは欠落 |

`qs`と`headers`は挿入順を保つ。Responseの本文は`Arc<[u8]>`で共有し、未知プロパティは未定義値を返す。

- JSON入力は`values::from_json`でMiniJinjaの値へ再帰変換する。
  - 数値: i64、u64、i128の順で整数を扱い、それ以外をf64へ変換する。`arbitrary_precision`の内部mapを式へ渡さない。
  - 経路: ルートのdict・配列・Requestの`json`と`form`で同じ変換を使う。
- 設定読み込み時は`TemplateEngine::load`で検査する。
  - 再利用: 同じソースのAST検査・コンパイルは一度だけ行い、`render`で保持済みテンプレートを使う。
- 正規表現の設定は用途ごとに読み込む。
  - `search`・置換: `Regex::new`。
  - Jumeauxの`exact_match`・`reg`: `Regex::new_exact`で専用型`ExactRegex`を生成する。Python同様、`^(pattern)$`で包んだ後のフラグ位置とgroup番号を検査する。
  - 型の制約: 通常の`Regex`には`exact_match`を公開しない。`(a)\2`を包む前に検査して拒否する経路をなくす。

## テンプレートのサポート外

| 分類 | サポート外 |
| --- | --- |
| filter | `center`、`forceescape`、`urlize`、`xmlattr` |
| test | `callable` |
| Python文字列メソッド | `rsplit`、`index`、`partition`、`zfill`、`encode` |
| 演算 | `"%d" % n` |

MiniJinjaのASTを走査してfilter/test名を登録済みの名前と照合する。コメント内は対象外とし、未実行分岐・macro・filterブロックも検査する。

- MiniJinja固有のfilter・testを登録から除外する。
  - filter: `bool`、`chain`、`lines`、`split`、`zip`。
  - test: `startingwith`、`endingwith`、`safe`。
- MiniJinjaとminijinja-contribは`=3.0.5`に固定する。
  - 理由: filter/test検査が`unstable_machinery_serde`のAST形式に依存する。

## HTTP転送の接続契約

`compat/fixtures/http-transport.json`に62件のPython観測値を保存する。

- `compat/gen/http_transport.py --rust <http_probe>`でPython実測・保存済みfixture・Rust転送を照合する。
  - 比較: メソッド、URL、自動・明示ヘッダ、本文bytes、再試行回数、エラー分類。
- `compat/harness/http_acceptance.py`で既存31ケースのAPI記録124リクエストも比較する。
  - 入力: 基準Pythonの入力アドオンが生成したRequestをRustへ渡す。one/otherは並行送信する。
  - 接続: `executor::Pipeline::challenge`がHTTPの結果をアドオン・Trial・保存へ接続する。製品CLIとReportの書き出しは後続タスク。

### HTTPモジュールのAPI

| API | 責務 |
| --- | --- |
| `http::prepare` → `PreparedRequest` | pathの正規表現置換、query加工、url_encoding、ヘッダと本文の生成。failure用の未正規化URLも保持 |
| `http::HttpTransport::new` | one/otherのAccessPointとmax_retriesから共有Cookieを持つ転送器を生成 |
| `HttpTransport::with_options` | CA bundle、環境変数の使用、接続・読込timeout、DNS resolverを指定。通常は既定値を使う |
| `send_one`・`send_other` | 単一endpointへ送信し`models::Response`を返す |
| `concurrent_request` | 両側の通信完了を待ち、Responseの組を返す |
| `challenge` → `ChallengeOutcome` | ConnectionError系だけをfailureのTrialへ変換。成功時は`ResponsePair`に両Responseとrequest_timeを返す |

- request_timeはローカル時刻のUTCオフセットを付けたISO日時とする。
  - 小数部: マイクロ秒が0なら省略し、非0なら6桁を出力する。Pythonのisoformatから採取した4件を検証する。

- `src/http/request.rs`に送信データの組み立て、`src/http/transport.rs`に接続処理を閉じる。
- 応答ヘッダは`models::Headers`に小文字キーで保存し、重複値は`, `で連結する。
  - 条件式: 既存の`Response::to_template_value`による大小無視アクセスを使う。HTTPから受け取ったヘッダで検証する。
  - 判定: 後続のignore_response_header_keysも小文字化して比較する。ヘッダ値は変更しない。
- 両側の通信が成功してからencoding・typeを決定する。
  - 理由: 一方の本文・Content-Typeが変換不能でも、他方がConnectionErrorならPythonと同じfailureにする。

| 項目 | Python版の挙動 | Rustの実装・検証 |
| --- | --- | --- |
| Session | exec全体で1個。one/otherと各trialで共有 | 1個のCookieStoreを両endpointで共有。挿入順を保持してpath長の降順で安定ソートし送信 |
| Session.close | 各http_get/http_postのfinallyで呼ぶ。Cookieは残る | idle接続をプールしない。Cookieは独立したCookieStoreに残す |
| gzip・deflate・zstd | Accept-Encodingは`gzip, deflate, zstd`。展開済みbytesと元の圧縮ヘッダを保持 | flate2・zstdで展開。reqwestの圧縮featureは無効。Content-Encodingの大小無視、gzipの末尾ゴミ・途中切れ・複数member、zstdの途中切れを検証 |
| redirect | 最大30回。31回目の応答を受信後にTooManyRedirects | 本文の展開・途中切断エラーは無視して追跡する。ローカル受信31回を実測済み |
| POSTのredirect | 301・302・303ではGETへ変更して本文を消す。307・308ではPOSTと本文を維持 | 5ステータスを実測済み |
| proxy | endpoint.proxyからhttp用は`http://<host>`、https用は`https://<host>`を生成 | 明示proxyを環境変数より優先しNO_PROXYでも除外しない。HTTP proxy・HTTPS proxyをローカルで検証 |
| proxy環境変数 | requests.Sessionのtrust_envは既定true | HTTP_PROXY・HTTPS_PROXY・ALL_PROXY、優先する小文字名、NO_PROXYのホスト・port・IPv4 CIDR・`*`を検証。NO_PROXYは大小無視と先頭`.`を除いた完全一致も扱う。CGI環境の大文字HTTP_PROXYは無視 |
| 証明書 | verifyは既定true。REQUESTS_CA_BUNDLE、次にCURL_CA_BUNDLEを参照 | rustlsとwebpki-rootsで検証。未信頼CA・ホスト名不一致を拒否し、追加CAと両環境変数を検証 |
| timeout | Jumeauxはtimeoutを渡さず、requests既定の無期限 | 実測では例外分類のため0.2秒を注入。他のprobeは3秒。製品の既定値変更ではない |
| retry回数 | Nは初回を除く追加試行回数 | N=0・2の記録一致を確認。reqwestの標準retryを無効にし独自ループで制御 |
| 接続失敗 | GET・POSTとも初回+N回 | DNS・接続拒否・connect timeoutをソケット入口の注入で実測 |
| 応答ヘッダ前の切断 | GETは初回+N回。POSTは1回 | ローカル受信回数と例外を実測済み |
| 応答ヘッダ前のread timeout | GETはN>0なら初回+N回。POSTは1回 | ローカル受信回数と例外を実測済み |
| 本文読み取り中の失敗 | retryしない | body timeoutと途中切断を実測済み |
| 503 | Retry-Afterなしではretryしない | GETで実測済み |
| 503 + Retry-After | GETは初回+N回。N回消費後の503を返す。POSTはretryしない | Retry-After: 0でPythonと比較し、1秒の待機もRustテストで検証。413・429も同じ分岐で処理 |
| backoff | HTTPAdapterに整数を渡した場合backoff_factor=0 | urllib3のRetry定義を確認。Retry-Afterによる待機は別扱い |

- reqwest 0.12.28の`ClientBuilder::retry`は、エラー・ステータス・メソッドによる分類と回数制限を提供する。
  - 実測: `for_host(...).no_budget().max_retries_per_request(2)`で切断時の3回送信を確認した。
  - 不足: classifierのReqRepは応答ヘッダを公開せず、retry Futureは即時完了する。Retry-Afterの有無・値・待機をそこで再現できない。
  - 実装: `retry::never()`を指定し、HTTPモジュール側で対象・待機・残り回数を管理する。本文読み取り失敗では再送しない。
- `Retry-After`は秒数とHTTP日時を受理する。空値では再試行せず返し、その他の不正値はInvalidHeaderとして停止する。
- TLS backendは`rustls-tls-webpki-roots`を使用する。通知も同じTLS・CA・proxy設定を使用する。
  - 理由: native-tlsがmacOSで追加する有効期間・EKUの制限を持ち込まない。EKUなしの10年・1年証明書で転送と通知の成功を検証する。
  - ルート: Mozilla系のwebpki-rootsを使う。certifiと更新時期まで同一とは限らない。明示CA bundleを指定した場合は既定ルートを無効にする。
  - 読込: CA bundleはHTTPS接続時だけ読む。HTTPのみの実行は不正なbundleの影響を受けない。
  - 読込エラー: 存在しないパスはOSErrorとしてchallengeの外へ返す。読取権限のないファイルはSSLErrorとしてfailureにする。いずれもPython実測と同じ分類を維持する。
  - 内容エラー: 空・不正なPEMはSSLErrorとしてchallengeでfailureにする。
- reqwestが返すTLS・proxyの接続失敗はConnectionError系へ写す。
  - 契約: Pythonの例外クラスの派生構造ではなく、failureになる範囲を維持する。ネイティブなエラー詳細はError.messageに保持する。

参照: [reqwest 0.12.28 retry](https://docs.rs/reqwest/0.12.28/reqwest/retry/index.html)。実装判断は取得済みcrateの`src/retry.rs`と基準Pythonのrequests・urllib3ソースを照合した。

### 本文と記録の実測

- rawはUTF-8 bytes、formはUTF-8のurlencode、jsonはensure_ascii付きのJSONになる。
  - JSON: `, `・`: `の空白も本文bytesの契約に含む。多倍長整数の桁も保持する。
  - form: リストは同名キーへ展開し、Noneは省略する。boolの文字列表現は`True`・`False`。
  - 多重リスト: requestsの展開後、urlencodeのdoseqでもう1段展開する。
- Python版のPOSTは`data=raw or form`と`json=json_`をrequestsへ渡す。
  - 優先順位: 非空dataがjsonより優先する。空dataとjsonを渡すとjsonになる。
- fixtureの記録はmethod・path/query・比較対象ヘッダ・body_hexを保持する。
  - 正規化: ホストはループバックの一時ポートなので省く。ヘッダ名は小文字。リストの順序は比較契約に含めない。
  - 検証: 62ケースすべてをRustと比較し、保存済みPython fixtureの再現も検査する。
- zstdの途中切れは展開済みbytesを返す。
  - 実測: ヘッダの途中なら空本文、本文の途中なら展開できた部分を保持する。不正なフレームはContentDecodingErrorとする。
- `http-review2.json`にCAの5ケースと通知の14ケースを保存する。
  - 採取: `compat/gen/http_review2.py`でPython実測をexpectedに保存する。ローカルのTLS受信先と通知受信先だけを使う。
  - 検証: 19件すべてをPython観測値と照合する。通知v1・v2の本文bytesとUser-Agent以外の自動ヘッダはPython記録と比較する。
- `http-request.json`の18ケースでpath置換の後方参照、queryの`/i`・削除・`$DATETIME`、CP932のURL、ヘッダ・本文を固定する。
  - URL: 既存のパーセントエンコードも`%HH`の大文字表記へそろえる。
  - query: overwriteとremoveの`/i`は元のqsから最初に一致したキーを選ぶ。追加したキーを再検索しない。
  - ヘッダ: Pythonと同じく通常dictでマージしてから大小無視へ変換する。大文字小文字だけ違うキーを併記した場合の挿入順もfixtureで維持する。
  - Content-Length: 非空POST本文では明示値も実際の本文bytes長へ更新する。
  - 空POST: 明示Content-Length・Transfer-EncodingがなければContent-Length: 0を送る。

### 通信失敗とTrialの実測

Pythonの`challenge`は`requests.exceptions.ConnectionError`だけを捕捉する。観測した実例外を`concurrent_request`の境界へ注入し、実際の`challenge`による結果もfixtureへ保存する。

Rustもこの捕捉範囲を互換契約として維持する。ReadTimeoutなどを一律failureにはしない。

| 障害 | GET N=0 | GET N=2 | POST N=0・2 | challengeの結果 |
| --- | --- | --- | --- | --- |
| DNS失敗・接続拒否 | ConnectionError | ConnectionError | ConnectionError | failure |
| connect timeout | ConnectTimeout | ConnectTimeout | ConnectTimeout | failure。ConnectionErrorの派生型 |
| 応答ヘッダ前の切断 | ConnectionError | ConnectionError | ConnectionError | failure |
| 応答ヘッダ前のread timeout | ReadTimeout | ConnectionError | ReadTimeout | ReadTimeoutは捕捉せず停止。ConnectionErrorだけfailure |
| 本文のread timeout | ConnectionError | ConnectionError | ConnectionError | failure |
| Content-Lengthに満たない本文で切断 | ChunkedEncodingError | ChunkedEncodingError | ChunkedEncodingError | 捕捉せず停止 |
| redirect上限超過 | TooManyRedirects | TooManyRedirects | 未実測 | 捕捉せず停止 |

- failureの両側summaryは`url`と`type: unknown`だけを保持する。
  - 省略: status_code、byte、response_sec、encoding。片側が成功しても失敗側だけのsummaryにはしない。
  - URL: 送信前に作ったURLを保存する。queryが空でも末尾の`?`が残る。
- TLSのSSLErrorとProxyErrorもConnectionErrorの派生型。
  - 検証: ローカルCAの未信頼とホスト名不一致を拒否し、challengeのfailureへ変換することを確認する。

## challengeの統合と保存

- 入口は`executor::Pipeline::build`と`Arc<Pipeline>::exec`。
  - 入力: `log2reqs::prepare`でreqs2reqsまで適用したRequest配列、共有HttpTransport、key、processes時のCookieスコープ。
  - 出力: `LoggedResult<Execution, Progress>`。Executionは`trials`・`logs`・`started_at`・`ended_at`・`concurrency`・`comparison`を持つ。Report集計・final・CLIは実行しない。
  - 単体入口: `challenge`は1trialのHTTPを実行する。`finish`はHTTPのResponsePairを注入し、同期アドオンと保存を実行する。
- logsはCLIの進捗行を組み立てる構造化データを観測順で保持する。
  - 形式: `Progress { seq, total, level, event }`。色・接頭辞・文字列の書式は含めない。
  - 開始: `Started`に元のRequestを保持する。nameなし・空文字はNoneを維持する。
  - HTTP: カスタマイズ後のURL、設定のproxy host、追加headersとraw・form・json、大文字小文字を保持したマージ後headersを送信前に記録する。URLの動的queryはログ用に再計算しない。
  - 応答: res2res前のstatus_code・response_sec・本文byte数・Content-Typeを両側とも記録する。encodingやtypeの変換で停止した場合もHTTP結果を保持する。
  - 計測: res2resとres2dictの両側、Diff diagnosis、Judgement、Store criterion、Did challengeの8段階をDurationで保持する。Store criterionはdumpと保存を含む。失敗した段階の完了イベントは出さない。
  - アドオン: レイヤーとone/otherに相当するstage、設定内のindex、name、選んだ分岐の素の値を保持する。条件式をログ用に再評価しない。
  - 保存: 保存可否と、書き込み済みの各ファイルのside・本文/prop種別・パス・byte数・encodingを保持する。
  - 結果: `Judged`はjudgement後・store_criterion前に記録する。`ConnectionFailed`は通信失敗時、`Failed`はtrialを停止するErrの発生時に記録する。
  - 表示材料: `Progress::summary()`で既定・-vの結果行に必要なTrialProgressを取り出せる。正常応答は`name.unwrap_or(path)`、通信失敗の名前なしは`None`と表示する。
  - レベル: Normalは既定、Verboseは-v、VeryVerboseは-vv、Debugは-vvvに対応する。Warning・Errorはverbosityによらず表示できる。
- `exec_with_progress`はイベントをコールバックへ実行中に通知する。
  - 順序: コールバックの呼び出しを直列化し、成功時のExecution.logsと失敗時のFailure.logsにも同じイベント列を保持する。
  - コールバック: worker上で呼ぶため、短時間で戻りpanicしないこと。channelへ送ってCLI側で処理してもよい。
  - 停止: 保存・後続アドオンが失敗しても、実行中trialの完了までイベントを通知し続ける。
  - 互換入口: `exec`は通知先を省略した入口。単体のchallenge・finishも直接出力しない。
  - 責務: 起動見出し・prepare・finalの出力は各呼出側に残す。urllib3の接続・応答ログは既存の比較除外に従う。
- 実行時刻はタイムゾーン付きの`started_at`と`ended_at`に保持する。
  - start: 保存ディレクトリ作成後・trial開始前。
  - end: 全trial完了後・latestの張り替え前。保存とdid_challenge/sleepの時間を含む。
- 成功時の順序はHTTPのone/other同時送信、res2resのone→other、res2dictのone→other、差分、judgement、store_criterion、dumpと保存、did_challenge。
  - 参照: judgement・store_criterion・did_challengeへres2res適用後のResponseを渡す。
  - 通信失敗: HTTPのChallengeOutcome::Failureをそのまま返す。後続アドオンと保存をスキップする。
  - エラー: initial_payloadを含むアドオン・HTTP・保存のErrは停止として返す。failureへ置換しない。Errを受け取った時点で新しいtrialのspawnを止め、実行中のtrialは保存とsleepも含めて完了を待つ。abortはしない。既に完了した保存は巻き戻さない。
- threadsとprocesses相当の並行数は`Config::concurrency()`から決める。
  - 制限: HTTP、アドオン、保存、sleepを含めて同時実行trial数を制限する。
  - 結果: 完了順に依存せず入力順へ並べる。エラーが複数ある場合も入力順の最初を返す。
  - runtime: HTTPはasync、同期アドオン・保存・sleepはspawn_blocking内で実行する。prepareの入力ファイル読み込みはtokio::fsを使う。
  - クライアント: HttpTransportのendpointをArcで共有する。`isolated_cookies()`でもreqwestのClientとHTTPS初期化状態を再利用する。
  - Cookie: threadsでは共有する。processesでは`ProcessCookieScope::PerTrial`を既定とし、trialごとに分離する。one/other内では共有する。公開APIでSharedを明示した場合のみtrial間でも共有する。
- store_criterion/freeはstored=trueなら短絡する。
  - 未指定: freeのwhen_anyがNoneなら保存する。空配列なら保存しない。レイヤー自体が空なら保存しない。
  - 条件: when_anyを先頭から評価し、最初の真で保存する。式には辞書を渡す。Responseのtext・ok等のプロパティは公開しない。
  - req: Pythonのchallengeが渡すname・path・qs・headersからRequestを再構築する。methodはGET、raw・form・jsonはNone、url_encodingはutf-8になる。元リクエストがPOSTの場合もこの形を維持する。
- dumpはpayload.bodyとpayload.encodingを順に変換する。Response本体とTrialのencoding・byteは変更しない。
  - json: キーを再帰ソートし、4スペースindent、ensure_ascii=False、Pythonの数値表記で保存する。
  - xml・html: 既存markupのprettyを使う。force・type・default_encodingの判定を各アドオン境界で行う。
  - encoding: 元encodingでstrict decodeし、指定encodingでstrict encodeする。元encodingがNoneならTypeError。
  - 通常整形: decode・encodeはreplace。スキップしてもpayload.encodingにdefault_encodingを補う。
- did_challenge/tagは条件に一致したタグを設定順で追加する。
  - 参照: 同じtagアドオン内ではすべて追加前のTrialを見る。後続tagアドオンは追加後のTrialを見る。
  - 重複: res2resのone・otherタグは最初の出現順で重複除外する。did_challengeによる追加タグは重複除外しない。
  - 辞書: Responseはto_dict相当、propsは生のdict/listまたはNone。Trialには保存済みfile・prop_fileが入る。
- did_challenge/sleepは`min + (max - min) * unit`秒待つ。
  - 範囲: unitは0以上1未満。max < minでもPython同様に計算する。負値・非有限値はエラー。
  - 検証: fixtureで乱数を固定した計算と、Rustテストで固定0.02秒以上の実待機を確認する。乱数列そのものは比較しない。
- 保存先は`responses/<key>/{one,other,one-props,other-props}`。
  - ファイル名: 本文は`(seq)name`、propは`(seq)name.json`。name未指定・空文字はseq。日本語・空白・括弧・拡張子をそのまま使う。
  - スラッシュ: 名前に含まれる場合もサニタイズや中間ディレクトリ作成をしない。Python同様に保存エラーとなる。
  - 順序: oneのdumpと書き込み、otherのdumpと書き込み、one-props、other-props。propは解析結果がある側だけ保存する。
  - prop: owlmixin同様に入れ子のnullも除外し、再帰キーソートと区切り`(',', ': ')`を適用する。`1E2`→`100.0`、`-0`→`0`等は保存時だけ正規化する。res2dictの値は書き換えない。
  - encoding: 本文はdumpの指定、propは固定UTF-8。propはoutputのwriterをUTF-8で使う。output.encodingは後続Report・summary・stdout等の設定として保持する。
  - scalar: propのルートがdict/list以外ならPythonと同じTypeError。本文2ファイルが既に保存されている場合も消さない。
  - ディレクトリ: 4サブディレクトリのmodeはUnixで0777。keyが既存でも保存用サブディレクトリが未作成なら使用できる。one・other・one-props・other-propsを順に作成し、既存サブディレクトリに達した時点でエラーとする。それ以前の作成分は巻き戻さない。
- keyは`sha256((str(now()) + args_str).encode('utf-8')).hexdigest()`。
  - 時刻: 空白区切り、タイムゾーン付き、マイクロ秒は0なら省略・非0なら6桁。`hash_from_args_at`で時刻固定のfixtureを比較する。
- latestは全trialが正常に返った後に相対リンクを張り替える。
  - failure: 通信失敗Trialも正常な戻り値なので更新する。アドオン例外では以前のlatestを維持する。
  - 既存リンク: リンク先が存在しなくても張り替える。過去のkeyディレクトリは削除しない。
- HTMLの最初の開始タグが途中で切れた場合、libxml2が作る親要素を空DOMへ補う。
  - 実測: `<bad`・`<p`・`<img`・属性途中はhtml/body、`<head`はhtmlのみ。`<html`・空文字・不完全コメント・DOCTYPE・終了タグはAttributeError。`<`単独は本文テキストとなる。
- 受入は`compat/fixtures/challenge.json`の104件。
  - 結果: Python完全一致102件。ゼロ除算の2件は既存の承認済み例外差異を適用し、TypeErrorで一致する。
  - 内容: store_criterion 24件、dump 41件、tag 9件、sleep 5件、prop 7件、key 8件、ファイル名4件、HTTP以降の統合6件。
  - ローカルHTTP: 既存22ケースとverbose_2・verbose_3を合わせた24ケースのTrial 47件と本文・prop 102ファイルを保存済みPython基準と比較する。request_time・response_secと動的portだけを正規化し、ヘッダ名の大小と承認済みheader_ignore_caseを既定の比較契約どおり扱う。
  - verbose: verbose_2の33行・verbose_3の45行に必要な値とレベルをイベントから復元して比較する。計測時間・動的port・User-Agentのversionを正規化する。実際のCLI書式はCLI統合時に比較する。
  - ストリーミング: コールバック内で開始とsleepを止め、HTTP前とexec完了前にイベントを受け取れることを検証する。通知列とlogsの完全一致を成功時・エラー時の両方で確認する。
  - 追加fixture: `compat/fixtures/challenge-review.json`の15件はPythonと一致する。保存先作成6件とログ9件を比較する。ログの実測時間だけを0.0秒へ正規化する。
  - 追加検証: 並行数と順序、Cookie共有・分離の既定、failureのスキップ、例外時の新規投入停止・実行中trial完了待ち・latest維持、空のkeyディレクトリ再使用と既存サブディレクトリ拒否、リンク張り替え、実行時刻、実sleepをRustテストで固定する。

## 未決

- `rustpython-parser`への依存。
  - 現実装: `ast.literal_eval`相当の構文検査に0.4.0を使う。tuple・set・bytes等のliteral fixtureで契約を固定する。
- `-vv` のconfig YAMLダンプ。
  - 申し送り: arbitrary_precisionを有効にしたserde_json::ValueをYAMLへ直列化すると、数値が `{$serde_json::private::Number: '1'}` と露出する。
  - 残件: CLIのconfigダンプ実装時に数値の変換境界を設ける。今回のYAML読み込みとマージ展開はデシリアライズ方向だけであり、この問題を起こさない。

### その他の未決

- `final/miroir` の `local_stack` と `endpoints` の同時指定。
  - Python版: ValueErrorで停止する。
  - 申し送り: Rustの型付きConfigは両方を保持できる。後続のfinal miroirタスクで排他検証を接続する。
- 外部コマンドアドオンの設定・入出力・エラー・タイムアウトの契約と、`cls_name`・`init addon` の移行方法。
- Python固有のtracebackをRust版のstderrでどこまで一致させるか。
  - 実測: Python版のencoding_hellは `Sorter.conditions` のUnknownPropertiesErrorで終了コード1。reportとresponsesは生成されない。この結果を基準に保存する。

## 受入の段階

- CLI統合前は5コンポーネントのJSON fixtureを使う。
- CLI統合後は16テンプレートのreport、保存ファイル、stdout、stderr、終了コードとAPI記録を比較する。
- 通知は追加ケースでSlack v1・v2をローカル受信先に送って比較する。
- 既存不具合は `python_mismatch: true`、理由、Python観測値、修正後のexpectedを持つfixtureで検証する。
- ヘッダ除外の大小無視は、Pythonの観測ケースと正しい大小表記の対照ケースを両方採取する。
  - 規則: Pythonラッパーの自己比較は観測値と一致させる。Rustバイナリは対照ケースの出力を期待値とし、大小を変えた入力configで成功することを検証する。
- Viewer目視、実OSでのHTTP環境依存挙動、Miroir実接続は後続の全体受入で行う。
