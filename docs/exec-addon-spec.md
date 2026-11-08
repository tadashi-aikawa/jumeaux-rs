# 外部コマンド方式のアドオン仕様

本書は2026-11-08にタダシが承認した仕様。末尾の「決定」に6論点の決定を記録する。

- 9レイヤー共通の `name: exec` を追加し、1呼び出しにつき1プロセスを起動する。
- `res2res/json` は名前を維持し、`transformer` の中身を外部コマンド設定へ置き換える。
- 配布対象: macOS・Linux。
  - 対象外: 外部コマンドアドオンのWindowsでの動作。
- 対象外: Pythonモジュールの動的ロード、常駐プロセス、実装コードの追加。

## 基準と現状

Python版の基準コミットは `dc276b8e3a0486bd8d4de2c8e3900aa36cbb7a66` 。Rust版の調査基準は `3245d6bf1a9add1037c8a3ddd8d7482319997ec8` 。

| 一次ソース | 確認した契約 |
| --- | --- |
| Python `jumeaux/addons/__init__.py` | 組み込み名、Pythonモジュール名の順に解決。`cls_name` の既定値は `Executor`。レイヤー内は戻り値を順に渡す |
| Python `jumeaux/addons/models.py` | AddonとAddonsの設定モデル |
| Python `jumeaux/models.py` | 9レイヤーのPayload・Reference、Request・Response・Reportの実体 |
| Python `jumeaux/domain/config/vo.py` | reqs2reqsへ渡すConfig、OutputSummary、Notifier |
| Python `jumeaux/addons/res2res/json.py` | `when`、transformerの引数、出力encodingと置換規則 |
| Python `jumeaux/sample/addon/` | add_path・ignore_values・tableの意図と旧設定例 |
| Python `mkdocs/ja/addons/index.md`、`addons/res2res.md` | カスタムアドオンの設定、transformerの公開IF |
| Python `mkdocs/ja/releases/v0.md` | `init addon` の導入説明 |
| Rust [payload.rs](../src/addons/payload.rs)、[mod.rs](../src/addons/mod.rs)、[registry.rs](../src/addons/registry.rs) | 型付きStage、静的登録、`Chain::build` |
| Rust [executor.rs](../src/executor.rs)、[commands.rs](../src/commands.rs)、[init.rs](../src/init.rs) | 呼び出し順、CLIの実経路、initの入口 |
| Rust [compat-spec.md](compat-spec.md) | Python依存機能は廃止せず外部コマンドで代替する決定 |

背景資料からの補正は次のとおり。

- Payload・Referenceの定義はPythonの `addons/models.py` ではなく `models.py` にある。
- RustのCLI入口は `src/commands/` ではなく `src/commands.rs` と `src/init.rs` にある。
- Pythonのlog2reqsはPayloadではなくRequest配列を返す。
- Pythonのfinalは副作用だけで完結せず、FinalPayloadを返す。
- 旧 `ignore_values.py` は存在しない `remaining_diff_keys` を参照する。
  - 方針: 現行の `diffs_by_cognition` に合わせ、サンプルの意図を再現する。
- 旧 `final/table.py` のexecはreference引数を受け取らない。
  - 根拠: 本体の `apply_final` はpayloadとreferenceの2引数で呼び出す。
  - 方針: この呼び出し不整合も新雛形へ持ち込まない。

## 設定

```yaml
addons:
  reqs2reqs:
    - name: exec
      tags: [custom]
      config:
        command: [python3, addon_sample/reqs2reqs/add_path.py]
        timeout_sec: 60
        cwd: .
        inherit_env: true
        env:
          LANG: C.UTF-8
          UNUSED_TOKEN: null
        max_stdout_bytes: 67108864
        max_stderr_bytes: 65536
        options:
          path: /hogehoge
```

`exec` は各レイヤーの予約名。log2reqsだけは既存どおり単一のAddonを指定する。他の8レイヤーは配列の要素として指定する。

| configキー | 型・既定値 | 契約 |
| --- | --- | --- |
| `command` | 必須の空でない文字列配列 | 先頭が実行ファイル、残りが引数。先頭の空文字と全要素のNULを拒否。空の引数は許可 |
| `timeout_sec` | 正の有限数、60 | 起動開始からstdin送信、stdout/stderrの回収、終了待ちまでの制限。0・負数・bool・無限大を拒否 |
| `cwd` | 文字列、未指定 | 子プロセスの作業ディレクトリ。未指定はJumeaux起動時のcwd |
| `inherit_env` | bool、true | 起動時の親環境を子へ継承するか |
| `env` | 文字列→文字列またはnullの辞書、`{}` | 継承後に上書き。nullは削除。変数名の空文字・`=`・NULと値のNULを拒否 |
| `max_stdout_bytes` | 正の整数、67108864 | stdoutの上限64 MiB。超過で停止。JSON解析前のUTF-8 bytesで数える |
| `max_stderr_bytes` | 0以上の整数、65536 | エラー診断用に保持するstderr末尾の最大bytes。0は保持なし。表示量は制限しない |
| `options` | JSONオブジェクト、`{}` | アドオン固有の設定。stdinの `options` に渡す |

- 既定値を持つキーへの明示的nullは既定値として扱う。
  - 例外: `env` 内のnullは環境変数の削除を表す。
- `config` の未知キーと型違いは設定エラーにする。
  - 例外: `options` のキーと値の意味は外部コマンドが検証する。
- `include`、タグ除外、設定マージ、配列順は既存の規則に従う。
  - 条件: 最終的に有効な設定だけを検証する。log2reqsは既存どおりタグ除外しない。
- retryでは、後述の `--allow-exec` 判定を子プロセスの起動より前に完了する。
  - 対象: `mode: "validate"` による検証もコマンド実行に含む。
- ホスト側の事前検証を、入力読み込み・HTTP送信・成果物作成より前に完了する。
  - 対象: 設定型、cwd、実行ファイルの解決、実行可能性、JSON変換のwhen構文。
  - 制限: この段階は子プロセスを起動せず、scriptやoptionsの意味までは検証しない。
- ホスト側の検証後、各外部コマンドをvalidateモードで1回起動する。

### Python指定との対応

| 旧指定 | 新指定・処置 |
| --- | --- |
| `name: addon_sample.reqs2reqs.add_path` | `name: exec` と `config.command` |
| 旧 `config: {path: /x}` | `config.options: {path: /x}` |
| `cls_name: MyExecutor` | キーを削除。起動する処理はcommandと引数で選ぶ |
| 組み込みアドオンの `cls_name: Executor`、未指定、null | 互換のため受理。外部クラスをロードする意味は持たない |
| `exec` の `cls_name: Executor`、未指定、null | 同じく受理。別名はエラー |
| `json.config.transformer.module/function` | 後述の `transformer.command/options` |
| Pythonクラスのインスタンス変数 | 呼び出しをまたいで保持されない。必要な状態は外部で管理する |

## プロセス間の共通プロトコル

### 入力と出力

stdinにはUTF-8のJSON文書を1つ送り、直後にEOFを送る。BOMは付けない。

```json
{
  "protocol_version": 1,
  "layer": "log2reqs",
  "mode": "payload",
  "options": {},
  "context": {
    "invocation_cwd": "/work/project",
    "verbosity": 0,
    "trial_seq": null,
    "side": null
  },
  "payload": {"file": "requests"},
  "reference": null
}
```

上はlog2reqsの完全な入力例。対応するstdoutは、たとえば `[{"path":"/hello"}]` となる。

- `protocol_version` は整数1。外部コマンドは未対応版を非0終了で拒否する。
- `mode` は汎用execで `payload`、JSON変換で `json_transformer`、起動前検証で `validate`。
- `options` は設定の `config.options` または `transformer.options` の内容。実行制御用のcommand・envなどは混ぜない。
- `context.invocation_cwd` はJumeaux起動時の絶対パス。
  - 用途: payload内の相対ファイルパスを子のcwdと独立に解決する。
- `context.verbosity` はCLIで指定された `-v` の段数を表す0以上の整数。
  - 用途: コマンド自身が診断ログの詳しさを選ぶ。親はstderrをverbosityによって省略しない。
- `context.trial_seq` は1始まりのtrial番号。log2reqs・reqs2reqs・final・validateではnull。
- `context.side` はres2res・res2dict・dumpの実処理で `"one"` または `"other"`。両側を扱うレイヤーとvalidateではnull。
- stdoutは戻り値そのものをJSON文書1つで返す。
  - 形式: 通常はpayloadオブジェクト。log2reqs・JSON変換・validateは後述の専用形式。
  - 許可: 前後のJSON空白と末尾改行。
  - 拒否: 空出力、BOM、不正UTF-8、複数文書、JSON前後のログ、NaN・Infinity、重複キー。
- stdoutのJSONと終了コード0の両方が揃った場合だけ、結果をチェーンへ適用する。
- stderrは診断・表出力用。Jumeauxのstdoutには転送しない。
  - 表示: 行ごとに親のstderrへ流す。全レイヤーで表示量に上限を設けず、不正UTF-8だけを置換する。
  - 識別: レイヤー・設定インデックス・trial番号・one/otherを、存在する範囲で各行へ付ける。
  - 保持: エラー診断用に末尾 `max_stderr_bytes` だけをリングバッファへ残す。既定は64 KiB。上限超過はエラーにしない。
  - 単位: 保持上限は子から受け取った生bytesで数える。診断時にUTF-8として復元し、先頭が文字の途中なら置換する。
  - 長い行: 改行前のバッファも8 KiBごとに流す。継続部分へ識別情報を重複挿入せず、本文を切り捨てない。EOF時に未改行の末尾も流し、表示用の改行を1つ補う。
    - 保持: 表示用の改行は診断用の生bytesへ加えない。
  - エラー: 保持した末尾をエラー詳細へ添える。全量表示済みの部分と重複し得ることを明示する。
- 入出力と終了待ちは同時に進める。
  - 理由: 子がstdinを読み終える前に大量出力してもデッドロックさせない。

### validateによる起動前検証

実効設定の各execと各json transformerを、設定エントリーごとに1回呼ぶ。

- 順序: retryの許可判定、全設定のホスト側検証、validate、入力読み込み、HTTP実行の順。
- 対象: タグ除外・マージ後の設定。commandが同じでもoptionsが異なり得るため重複排除しない。
  - 条件: whenがfalseになるtransformerも検証する。retryで実処理を呼ばないlog2reqsも実効設定に残る場合は検証する。
- 入力: 共通のenvelopeで `mode: "validate"`、`payload: null`、`reference: null`。layer・options・contextは通常どおり。
  - context: trial_seq・sideはnull。verbosityとinvocation_cwdは実処理と同じ。
- 成功: stdoutへ `{"valid":true}` だけを返し、終了コード0で終了する。
  - 適用: 検証結果だけを受理し、payloadチェーンやReportを更新しない。
- 失敗: optionsの必須キー・型・値の誤りをstderrへ出し、非0で終了する。
  - プロトコル違反: 空stdout、valid=false、未知キー、型違い、未対応modeも全体停止。
- 制限: 同じcwd・env・timeout・出力制限を使う。検証プロセスの状態は実処理へ持ち越さない。
- 副作用: コマンド作者は検証でHTTP送信や成果物作成を行わない。
  - 限界: ホストは検証コードの副作用を隔離できない。validateも任意コードの実行であり、信頼確認の代用にはならない。
- 本処理でもoptionsを検証する。
  - 理由: validate後にファイルや環境の状態が変わる可能性がある。

### モデルのJSON表現

PythonのPayload・Referenceのフィールド名と入れ子構造を維持する。Pythonオブジェクト固有の表現は次の規則でJSONへ写す。

| 元の型 | JSON表現 |
| --- | --- |
| `TOption[T]` | Tまたはnull。入力では未設定のモデルフィールドもnullとして列挙 |
| `TList[T]`、`TDict[T]` | 配列、オブジェクト。配列順を保持 |
| `HttpMethod`、`Status` | 既存モデルのenum値。例: `GET`、`same`、`different`、`failure` |
| `bytes` | 標準Base64の文字列。`+`・`/`のalphabet、必要な`=`padding、改行なし。空bytesは `""` |
| `datetime.timedelta` | 総秒数の有限JSON number。フィールド名は `elapsed` を維持 |
| `CaseInsensitiveDict` | ヘッダ名→文字列のオブジェクト。大文字小文字は保持し、復元後の検索は大小を区別しない |
| 計算プロパティ・メソッド | 送らない。`text`・`byte`・`regard_as_same`・`result_path`などは外部側で計算 |

- 任意JSONの内部にあるnullは保持する。
  - 理由: report保存用のnull除外を通信プロトコルへ適用すると本文が変わる。
- 出力は差分パッチではなく、完全な戻り値として検証する。
  - 必須: 各Payloadの非Optionalフィールド。既定値があるRequest等のモデルフィールドは既定値補完を許可する。
  - Optional: 欠落はnullと同じ。未知のモデルフィールドは拒否する。
  - 例外: 任意JSONと辞書のキーは未知フィールド検査の対象外。
- `reference` は参照用スナップショット。stdoutにreferenceを返してもホストの状態は更新できない。
- JSON numberの整数を浮動小数へ丸めて通信しない。
  - 条件: 値は既存のRust JSONモデルが受理する範囲に限る。
- Responseの `elapsed` と `elapsed_sec` は別々の値として保持する。
  - 理由: Pythonの `elapsed_sec` は丸めを含み、elapsedと同一ではない。

### 基本モデル

表の `?` はnullを許すことを表す。

| モデル | フィールド |
| --- | --- |
| Request | `name: string?`, `method: HttpMethod`, `path: string`, `qs: map<string,string[]>`, `raw: string?`, `form: object?`, `json: object?`, `headers: map<string,string>`, `url_encoding: string` |
| Response | `body: Base64`, `encoding: string?`, `headers: map<string,string>`, `url: string`, `status_code: integer`, `elapsed: number`, `elapsed_sec: number`, `type: string` |
| DiffKeys | `added: string[]`, `changed: string[]`, `removed: string[]` |
| OutputSummary | `response_dir: string`, `encoding: string`, `logger: null` |
| Trial | `seq`, `name`, `tags`, `headers`, `queries`, `raw`, `form`, `json`, `one`, `other`, `method`, `path`, `request_time`, `status`, `diffs_by_cognition` |
| ResponseSummary | `url`, `type`, `status_code`, `byte`, `response_sec`, `content_type`, `mime_type`, `encoding`, `file`, `prop_file`, `response_header` |
| Report | `version`, `key`, `title`, `description`, `notifiers`, `summary`, `trials`, `addons`, `retry_hash` |
| Summary | `one`, `other`, `status`, `tags`, `time`, `concurrency`, `output`, `default_encoding`。Rustの既存拡張 `comparison` も維持 |

Trial以下の型とenum値は [models.rs](../src/models.rs) および [report_fields.rs](../src/models/report_fields.rs) の既存外部IFに従う。送信時のnull保持だけは前述のプロトコル規則を優先する。

Request・Trialのformとjsonはルートがobjectまたはnullに限る。object内部の値には配列・scalar・nullを許可する。

Base64を選ぶ。

- 理由: 一時ファイルの所有権・削除時点・パス共有を新設せず、同じstdin/stdoutで完結する。
- コスト: 本文n bytesにつき `4 × ceil(n / 3)` bytesへ増える。dumpではresponse.bodyとbodyが重複する。
- 比較: ファイルパス方式は大きな本文のコピーを減らせるが、一時ファイルI/Oとライフサイクルが必要。
- 初版: ファイルパス方式、Base64との自動切り替えは提供しない。

## 9レイヤーの入出力

表のpayloadとreferenceがstdinの同名キーへ入る。stdoutには「戻り値」欄だけを返す。

| レイヤー | payload | reference | 戻り値 |
| --- | --- | --- | --- |
| log2reqs | `{file: string}` | null | `Request[]` |
| reqs2reqs | `{requests: Request[]}` | 実効Config全体 | `{requests: Request[]}` |
| res2res | `{response: Response, req: Request, tags: string[]}` | null | 同じ構造のpayload |
| res2dict | `{response: Response, result: object/array/null}` | null | 同じ構造のpayload |
| judgement | `{diffs_by_cognition: map<string,DiffKeys>?, regard_as_same_body: bool, regard_as_same_header: bool}` | `{name, path, qs, headers, res_one: Response, res_other: Response, dict_one: object/array/null, dict_other: object/array/null}` | 同じ構造のpayload |
| store_criterion | `{stored: bool}` | `{status: Status, req: Request, res_one: Response, res_other: Response}` | `{stored: bool}` |
| dump | `{response: Response, body: Base64, encoding: string?}` | null | 同じ構造のpayload |
| did_challenge | `{trial: Trial}` | `{res_one: Response, res_other: Response, res_one_props: object/array/null, res_other_props: object/array/null}` | `{trial: Trial}` |
| final | `{report: Report, output_summary: OutputSummary}` | `{notifiers: map<string,Notifier>?}` | 同じ構造のpayload |

### log2reqs・reqs2reqs

- log2reqsは入力ファイルごとに1回呼ぶ。戻ったRequest配列を指定ファイル順に連結する。
  - 入力: `file` はCLIが解決に使う元のパス文字列。相対パスは `context.invocation_cwd` 基準で開く。
  - 特例: Rust内部の `Log2ReqsPayload.requests` は送らず、戻り値を内部のrequestsへ格納する。
  - 理由: PythonのPayloadはfileしか持たない。
  - 空入力: 戻り値 `[]` は成功。stdoutの空文字やnullとは区別する。
- reqs2reqsは全ファイルの連結後、各設定を1回ずつ実行する。
  - reference: `one`、`other`、`output`、`threads`、`processes`、`max_retries`、`title`、`description`、`tags`、`input_files`、`notifiers`、`addons`、`judge_response_header`、`ignore_response_header_keys` を持つ実効Config。
  - 意味: Pythonの第2引数configを `reference` として公開する。外部コマンド固有の設定はトップレベル `options`。
- retryは既存どおりreportからRequestを復元する。
  - 実処理の回数: log2reqsは0回、reqs2reqsは各設定1回。validateの事前検証は別に数える。

### res2res・res2dict・dump

- oneとotherは別々のpayloadで呼ぶ。各チェーン内では直前の戻り値を渡す。
- res2resの戻り値のresponseとtagsを後続の比較へ反映する。
  - req: 同じres2resチェーンの後続には渡す。HTTP送信済みのRequestやTrialの元Requestを遡って変更しない。
- res2dictの未変換は `result: null` で返す。
  - 禁止: stdout全体のnull、`{}`・`[]`への置換による未変換表現。
  - 継続: nullでも後続res2dictを呼ぶ。既にresultがある場合も短絡終了しない。
  - 最終値: object/array/nullに限定する。scalarはプロトコルエラー。
  - response: 後続res2dictには渡す。比較元responseをres2resより前へ巻き戻して更新しない。
- dumpはstore_criterionの最終storedがtrueのときだけ呼ぶ。
  - 保存: 戻ったbodyをそのままbytesとして保存する。encodingから自動再エンコードしない。
  - 順序: oneのdumpと保存を終えてからotherへ進む。
  - response: dumpチェーン内だけで更新する。判定結果や既存のResponseSummaryは再計算しない。

### judgement・store_criterion・did_challenge

- judgementはboolean両方を必ず返す。
  - 判定: `regard_as_same_body && regard_as_same_header` がtrueならsame、それ以外はdifferent。
  - `diffs_by_cognition`: 分類名→DiffKeys。未分類は `unknown`。null、空辞書、空DiffKeysを区別する。
  - 方針: bodyだけを書き換えるアドオンは入力のheader判定を保持する。
- store_criterionはstoredを返す。初期値はfalse。
  - reference.req: Python互換の再構築Requestを渡す。name・path・qs・headersを引き継ぎ、methodはGET、raw・form・jsonはnull、url_encodingはutf-8。
- did_challengeは保存後に実行する。戻ったtrialをReportへ集約する。
  - 制限: statusやファイルパスを変更しても、比較・保存を再実行しない。
- HTTPのConnectionErrorによるfailureでは、これらを含むレスポンス取得後の6レイヤーを呼ばない。
  - 根拠: 現在のexecutorはFailureのTrialをそのまま返す。

### final

- finalはReport作成後、設定順に1回ずつ実行する。
- 副作用だけのコマンドも入力のpayloadをstdoutへ返す。
  - 禁止: 空stdoutやnullを成功扱いにする省略形。
  - 理由: Pythonのreduce契約と後続finalへの受け渡しを維持する。
- reportを変更した戻り値は後続finalへ渡す。Summaryの自動再集計はしない。
- 表や診断はstderr、成果物はファイルへ出す。
  - パス: `output_summary.response_dir` と `report.key` を結合する。相対パスの基準は `context.invocation_cwd`。
  - 既存の組み込みfinalのstdout仕様は変えない。

## res2res/jsonの外部transformer

### 比較と採用案

| 案 | 利点 | 負担 |
| --- | --- | --- |
| 汎用 `exec` に統一 | 外部プロトコルがpayloadだけになる | when、encoding選択、type更新を各スクリプトに重複実装する |
| **`json` に専用transformerを残す** | 既存設定のwhenとdefault_encodingを維持できる | 戻り値が文字列となる別modeを持つ |
| 新名 `json_exec` を追加 | 旧Python版との名前上の区別が明確 | 同じ機能に新しい名前と移行規則が増える |

専用transformerを採用する。プロセス起動・タイムアウト・ログは汎用execと共用する。

```yaml
addons:
  res2res:
    - name: json
      config:
        when: '"sample" in req.path'
        default_encoding: utf8
        transformer:
          command: [python3, transforms/wrap.py]
          timeout_sec: 60
          options: {key: wrap}
```

transformerは汎用execと同じ実行設定キーを持つ。`module` と `function` は受理しない。

### 処理順

1. ホストが既存のwhen評価器で `{"req": Request, "res": Response}` を評価する。
   - 規則: whenが未指定またはnullならtrue。式がfalseなら元のpayloadを返し、プロセスを起動しない。
   - 互換: テンプレート式の型・truthiness・Response公開プロパティは既存のPython互換規則を使う。
2. 子へ次の入力を渡す。
   - `layer: "res2res"`、`mode: "json_transformer"`。
   - `payload: {"body": Base64, "encoding": stringまたはnull}`。
   - `reference: null`。共通のprotocol_version・options・contextも渡す。
3. stdoutからJSON文字列を受け取る。
   - 対応: Pythonの `(bytes, encoding) -> str` のstrをJSON文字列としてエスケープしたもの。
   - 例: 変換後本文が `{"wrap":1}` ならstdoutは `"{\"wrap\":1}"`。
   - 禁止: stdoutに `{"wrap":1}` を直接返す。これはobjectであり、str契約と異なる。
4. ホストが出力encodingを決め、返った文字列をbytesへ変換する。
   - 規則: response.encodingが非nullならそれを使用。nullのときだけdefault_encodingを使用。
   - 既定値: default_encodingは `utf8`。明示的nullも同じ。
   - 注意: transformerへ渡すencodingは元のnullのまま。default_encodingで引数を補完しない。
   - エンコード: Pythonの `errors="replace"` 相当。対応codecと既知の差異はcompat-specに従う。
5. responseのbody・encoding・typeを更新する。
   - type: `json`。
   - 保持: headers・url・status_code・elapsed・elapsed_sec、およびpayloadのreq・tags。
   - 注意: Content-TypeとContent-Lengthを自動修正しない。

文字列の内容がJSONとして正しいかは、この段階では検証しない。

- 根拠: Python版もtransformerのstrをそのままencodeし、JSON parseしない。
- 区別: stdout自体のJSONが不正なら即停止。stdoutが有効なJSON文字列でも、その中身が不正JSONなら後続の既存res2dict等の規則で扱う。
- 理由: 本文の空白・キー順・数値表現を勝手に再シリアライズしない。

## エラーと停止

外部コマンドのエラーはJumeaux全体を終了コード1で停止する。trialのfailureへ変換しない。自動リトライ・スキップ・元payloadへのフォールバックはしない。

| 条件 | 検出と扱い |
| --- | --- |
| 実行ファイルがない、実行権限がない、cwdがない | 起動前検証の設定エラー。実行時に消えた場合もエラー |
| 終了コード非0、シグナル終了 | `ExecAddonError`。コードまたはシグナル番号を表示。stdoutは適用しない |
| stdoutが不正JSON・不正UTF-8・空、戻り値の型違い | `ExecAddonProtocolError`。JSONパスと期待型を表示 |
| Base64不正、モデル値の不正、stdout上限超過 | `ExecAddonProtocolError`。該当フィールドまたは上限を表示 |
| timeout_sec超過 | `ExecAddonTimeout`。制限秒数を表示し、所有する子プロセス群を終了 |
| パイプの読み書き失敗、起動のOSエラー | `ExecAddonError`。OSの原因を添える |
| when・encodeのエラー | 既存のエラー種別に設定パスを添えて停止 |

- エラー表示には設定ファイルの由来、設定パス、レイヤー、設定インデックス、存在する場合はtrial番号とone/otherを含める。
  - 表記: `trial=1 side=one`。値がない項目は省き、設定パスは1回だけ示す。
  - 機密: options・env値・入出力本文・引数全体をエラーへ自動転載しない。実行ファイル名とstderrは表示する。
- パイプのEOFだけでは成功としない。子の終了も待つ。
- タイムアウトと出力上限超過では、その起動専用のプロセスグループへTERMを送り、1秒後に残存プロセスへKILLを送る。直接の子を必ず回収する。
  - 対象: 自分が起動・管理したPIDとプロセスグループだけ。
  - 制約: アドオンはdaemon化やプロセスグループ離脱をしない。離脱した子孫の回収は保証しない。
  - 正常終了: 子は子孫の終了も待ち、stdout/stderrを閉じてから戻る。
  - 時間: 終了処理の猶予はtimeout_secとは別枠の最大1秒。timeout時点で結果の適用を禁止する。
- stdin/stdout/stderrを閉じない子孫がいても、タイムアウトの時計は止めない。
- 非0終了ではstderrの回収と子の終了確認が済み次第、待機を終える。
  - 回収: 残る所有プロセスは即時KILLする。毎回1秒の終了猶予を追加しない。
- trialのエラー検出後は新しいtrialの開始を止める。
  - 既存契約: 既に実行中の別trialは終了まで待つ。返すエラーは入力順で最初のもの。
  - 成果物: 作成済みファイルや外部コマンドの副作用はロールバックしない。正常完了のReport作成とfinalへは進まない。
- finalで失敗した場合、後続finalを実行しない。
  - 成果物: 先行finalが作ったreportや通知は残る。latestの更新済み状態も巻き戻さない。
- 親がSIGINTまたはSIGTERMを受けたら、新しいtrialと外部コマンドの起動を止める。
  - 終了: validate・finalを含む全ての所有プロセスグループへTERMを送り、1秒後に残っていればKILLを送る。直接の子を回収する。
  - 区別: 通常のtrialエラーで実行中trialを待つ規則より、ユーザーによる中断を優先する。
  - 結果: 子の結果を適用せず、後続finalも実行しない。SIGINTは終了コード130、SIGTERMは143で終了する。
  - 制限: 親へのSIGKILLやOS停止、プロセスグループを離脱した子孫の回収は保証しない。

### retryの外部コマンド実行許可

retryの実効設定に外部コマンドが残る場合、既定で拒否する。

- 対象: 全レイヤーのexecと `res2res/json` のtransformer.command。
  - 条件: タグ除外・設定マージ後に判定する。when=falseやretryで未使用のレイヤーを理由に許可判定を省略しない。
- 許可: 利用者がそのretry起動時に `--allow-exec` を指定した場合だけ実行する。
  - 保存: 許可状態をReportへ保存せず、次回retryへ引き継がない。
  - 範囲: runで利用者が直接指定するconfigには、このretry専用フラグを要求しない。
- 拒否時: 終了コード1。HTTP送信・成果物作成・validateを含む子プロセス起動を行わない。
- 確認対象: Report内のcommand・引数・cwd・env・optionsと実行先ファイル。
  - 理由: 共有Reportを読む操作と、その中の任意コードを実行する操作を分ける。

```text
ConfigError: report.addons.final[0]: 外部コマンドを含むReportのretryを拒否しました。
Report内のcommandと実行先を確認し、信頼できる場合だけ --allow-exec を指定してください。
validateも外部コマンド実行に含まれます。
移行ガイド: https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/exec/
```

## コマンド解決・環境・移植性

- シェルは暗黙に起動しない。
  - 規則: 配列の各要素を1引数として渡す。空白、`$VAR`、`~`、glob、パイプ記号を展開しない。
  - 明示実行: `[sh, -c, "..."]` は指定したシェルの意味で動く。
- cwdの相対指定はJumeaux起動時のcwd基準で解決する。
  - include: includeファイルのディレクトリ基準には変えない。
  - 理由: 既存の入力・出力パスおよびretryの起動ディレクトリ基準に揃える。
- `command[0]` に `/` があれば、絶対パスか子のcwd基準の相対パスとして解決する。
- `command[0]` に `/` がなければ、env適用後の子のPATHから解決する。
  - 規則: PATHの空要素・相対要素も子のcwd基準。PATH未設定なら検索せず、絶対パスか `/` を含む指定を要求する。
  - 固定: 事前検証で選んだ実行ファイルの絶対パスを、そのrun中の起動に使う。
- 子へ渡すargv[0]には、設定のcommand[0]をそのまま使う。
  - 例: `sh` を指定した場合は `sh`。解決した絶対パスや解決処理で付く `./` をargv[0]へ追加しない。
- commandの第2要素以降は書き換えない。
  - 例: `python3 addon_sample/final/table.py` のスクリプトは子のcwd基準で開かれる。
- 環境は起動時のスナップショットを使う。レイヤーごとのenv変更を親や他の子へ反映しない。
- アドオンは利用者と同じ権限で動く。サンドボックスやネットワーク隔離は提供しない。
  - 判断材料: 設定ファイル・include・retry元のReportはコマンド実行権限を持つ入力として扱う。
  - 機密: 継承した環境、Requestのヘッダ、Response、Config、Notifierは外部プロセスに見える。
  - 限界: `inherit_env: false` は環境の継承を止めるだけ。payloadやファイルへのアクセスを制限しない。
- reportにはcommand・cwd・options・明示envを既存のaddons設定として保存する。
  - null: 外部コマンド設定内ではenvの削除指定とoptions内のnullを保持する。retryでも同じ指定を復元する。
  - 保存しないもの: 継承した環境の全内容、解決済み実行ファイルのホスト固有パス、context。
  - 運用: 秘密は設定のenvやoptionsへ直書きせず、実行環境で渡す。
  - retry: `--allow-exec` 指定時だけ保存したコマンドを再実行する。相対パスはretry時の起動ディレクトリ基準になる。
- 初版の配布対象はmacOS・Linux。
  - 対象外: 外部コマンドアドオンのWindows動作。PATH・パス区切り・プロセス終了制御の保証と受入をmacOS・Linuxへ限定する。

## jumeaux init addonの雛形

Python 3の標準ライブラリだけで動く3つのスクリプトを生成する。JumeauxのPython版、owlmixin、pip installは不要。

- 前提: サンプル実行には `python3` が必要。Rust本体の実行やinitでのファイル生成には不要。
- 言語: シェル版を併設しない。
  - 理由: Base64と入れ子JSONを追加依存なしで扱える。
- 実装時の同梱元: `assets/addon/`。
- 生成先: 起動ディレクトリの `addon_sample/`。
- 既に `addon_sample` が存在するときは終了コード1。既存ファイルを上書きしない。
- 既存のconfig.ymlは自動編集しない。

```text
addon_sample/
├── README.md
├── config.example.yml
├── reqs2reqs/add_path.py
├── judgement/ignore_values.py
└── final/table.py
```

`config.example.yml` は次のaddons断片を含む。

```yaml
addons:
  reqs2reqs:
    - name: exec
      config:
        command: [python3, addon_sample/reqs2reqs/add_path.py]
        options:
          path: /hogehoge
  judgement:
    - name: exec
      config:
        command: [python3, addon_sample/judgement/ignore_values.py]
        options:
          values: [apple, orange]
  final:
    - name: exec
      config:
        command: [python3, addon_sample/final/table.py]
        options:
          columns: [seq, name, path, status]
```

READMEには既存配列への追記方法を示す。

- 注意: 既存のjudgementやfinalを置き換えず、必要な位置へ要素を追加する。
- 注意: 複数configのマージではレイヤー配列が連結されない。断片を別configとして渡すだけでは追記にならない。

### 3つのサンプルの契約

| スクリプト | options | 動作・受入例 |
| --- | --- | --- |
| add_path | 必須 `path: string` | 入力requestsの末尾へ既定値付きRequestを1件追加。`[]` と `/hogehoge` ならGETの1件になる |
| ignore_values | 必須 `values: string[]` | unknownの差分のうち、oneまたはotherの値のPython文字列表現に指定文字列を含むものを無視する。空配列なら無変更 |
| table | 必須 `columns: string[]` | seq・name・path・statusから指定順の表をstderrへ出す。payloadはそのままstdoutへ返す |

ignore_valuesのモデル移行は次のとおり。

1. valuesが空、body判定がtrue、またはdiffs_by_cognitionがnullならそのまま返す。
2. `diffs_by_cognition.unknown` のadded・changed・removedを順序を保って走査する。
   - 空辞書またはunknownなし: そのまま返す。既存の分類済み差分を再判定しない。
3. 差分パスからone/otherの値を取得する。
   - 規則: `root`、`<'key'>`、`<index>` とエスケープを既存の差分パス規則どおりに扱う。evalは使わない。
   - 欠落: PythonのNone相当。nullとの一致判定上の区別は設けない。
   - 不正なパス: 非0終了。文字列を単純にドットで分割しない。
4. 値の文字列表現に指定文字列が含まれる差分を、unknownから `ignore_values` 分類へ移す。
   - 保持: 既存の分類と各配列の順序。既存のignore_values分類には末尾追記。
5. unknownが空になったら `regard_as_same_body: true` とする。
   - 保持: `regard_as_same_header` は変更しない。
   - 整理: 空になった分類は除去する。

例として、oneが `{"fruit":"apple"}`、otherが `{"fruit":"orange"}`、unknown.changedが `["root<'fruit'>"]` の場合を使う。

- valuesが `["apple"]` ならignore_values.changedへ移し、bodyをtrueにする。
- 入力のheaderがfalseなら最終判定はdifferentのまま。

tableは列名のヘッダと各trialの行をタブ区切りで出す。

- 文字列中の改行・タブは可視の `\n`・`\t` に置換する。
- 空trialsでもヘッダを出す。未知列名は非0終了。
- 旧owlmixinの装飾罫線の完全再現は要求しない。
  - 理由: サンプルの目的は列指定による一覧表示の再現であり、追加依存を避ける。

3つともprotocol_version・layer・mode・optionsを検証し、エラーはstderrへ、結果JSONだけをstdoutへ出す。

- validate: 本文処理や表出力をせずoptionsの検証だけを行う。
- JSON出力: 全雛形で `allow_nan=False` を指定する。NaN・Infinityの流出は非0終了にする。
- stderr: tableは64 KiBを超えても全行を表示する。エラー診断の保持上限とは独立。

### 最小のstdin/stdout例

次はres2resのpayloadをそのまま返す通信例。validateにも対応する。

```python
import json
import sys

try:
    message = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    if message["protocol_version"] != 1 or message["layer"] != "res2res":
        raise ValueError("unsupported protocol or layer")
    if message["options"] != {}:
        raise ValueError("options must be empty")
    if message["mode"] == "validate":
        result = {"valid": True}
    elif message["mode"] == "payload":
        result = message["payload"]
    else:
        raise ValueError("unsupported mode")
    output = json.dumps(result, allow_nan=False)
    sys.stdout.buffer.write((output + "\n").encode("utf-8"))
except Exception as error:
    print(str(error), file=sys.stderr)
    sys.exit(1)
```

この例のoptionsは空オブジェクトに限定する。戻ったpayloadのモデル検証はホストが行う。

### encoding=nullに対応するtransformer例

次は `transforms/wrap.py` 用の雛形。JSON本文を指定キーで包み、変換後の本文文字列をJSON文字列として返す。

```python
import base64
import json
import sys

try:
    message = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    if message["protocol_version"] != 1 or message["layer"] != "res2res":
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
    output = json.dumps(result, allow_nan=False)
    sys.stdout.buffer.write((output + "\n").encode("utf-8"))
except Exception as error:
    print(str(error), file=sys.stderr)
    sys.exit(1)
```

- null対応: この雛形自身が入力のdecode用に `options.input_encoding` を選ぶ。既定はutf8。
  - 区別: ホストのdefault_encodingは出力のencode用。transformerへ渡すnullをホストが補完する規則には変えない。
- 例: bodyが `eyJhIjoxfQ==`、encodingがnull、optionsが `{}` なら、stdoutは `"{\"wrap\": {\"a\": 1}}"`。
- 生成対象: initの3雛形に4つ目のアドオンは追加せず、READMEにこの変換例を掲載する。

## 旧指定のエラーと移行案内

新旧指定の混在は拒否する。Pythonモジュールを推測して `python -m` で実行しない。

未知の名前には、対象レイヤーの登録済み名一覧または近い候補を先に示す。

- 理由: 組み込み名の打ち間違いを、Pythonモジュールの移行問題と断定しない。
- 候補: 編集距離2以内の登録名があれば、最も近い名前を先に示す。この場合は移行案内を付けない。
- 候補なし: 登録名一覧を示し、Pythonカスタムアドオンの場合の移行先を添える。
  - cls_name: 非Executorの旧指定も含む場合だけ、名前のエラーに削除案内を付記する。
- 案内URL: `https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/exec/` を公開予定ページとする。公開済みとは扱わない。

```text
ConfigError: config.yml: addons.reqs2reqs[0].name:
アドオン "shufle" は登録されていません。候補: shuffle
```

```text
ConfigError: config.yml: addons.reqs2reqs[0].name:
アドオン "addon_sample.reqs2reqs.add_path" は登録されていません。
登録済みの名前: add, empty_guard, exec, filter, head, rename, repeat, replace, shuffle
Pythonモジュール名を直接ロードする方式は使用できません。
外部コマンド方式へ移行してください: name: exec と config.command を指定し、
従来のconfigを config.options へ移してください。
旧.pyをcommandに指定するだけでは動きません。stdin/stdoutのJSON契約へ書き換えてください。
雛形: jumeaux init addon
移行ガイド: https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/exec/
```

```text
ConfigError: config.yml: addons.judgement[0].cls_name:
cls_name: "MyExecutor" は使用できません。cls_nameを削除し、
実行する処理を name: exec の config.command と引数で指定してください。
Pythonクラスの自動ラップは行いません。stdin/stdoutのJSON契約へ移行してください。
旧.pyをcommandに指定するだけでは動きません。
移行ガイド: https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/exec/
```

```text
ConfigError: config.yml: addons.res2res[0].config.transformer.module:
Python transformerのmodule/function指定は使用できません。
transformer.command: [python3, transforms/wrap.py] へ置き換えてください。
whenとdefault_encodingはそのまま使えます。
変換結果の文字列をJSON文字列としてstdoutへ返す契約が必要です。
旧.pyをcommandに指定するだけでは動きません。
移行ガイド: https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/exec/
```

- `function` だけ残っている場合も、そのキーを指して同じ移行先を案内する。
- `command` と `module/function` の混在は旧キーのエラーを優先する。
- 名前とcls_nameの両方が旧指定なら、名前のエラーにcls_name削除も付記する。
- include由来の設定はinclude先のファイル名も示す。
- Python版の実装ファイルをcommandへ指定するだけでは移行できない。
  - 理由: 旧ファイルにはstdin/stdoutプロトコルのエントリーポイントがない。

## 性能とプロセス寿命

### 起動回数

変数は各レイヤーに設定された外部コマンドの数を表す。

| 変数 | 意味 |
| --- | --- |
| L | log2reqsのexec数。0または1 |
| Q | reqs2reqsのexec数 |
| R | res2resのexec数 |
| T | res2res/jsonのtransformer数 |
| D | res2dictのexec数 |
| J | judgementのexec数 |
| S | store_criterionのexec数 |
| U | dumpのexec数 |
| C | did_challengeのexec数 |
| F | finalのexec数 |
| V | validateの起動数。`L + Q + R + T + D + J + S + U + C + F` |

レスポンスを取得できた1trialあたりの起動数は次のとおり。

```text
保存なし: 2R + M + 2D + J + S + C
保存あり: 2R + M + 2D + J + S + 2U + C
M = one/otherでwhenがtrueとなったtransformerの合計数 (0〜2T)
```

- 各レイヤーにexecを1つずつ置き、transformerなしの場合は保存なし7回、保存あり9回。
- 上記へ常にtrueのtransformerを1つ加えると9回、11回。
- 入力ファイルK個、trial数N、全件保存・全transformer実行時の総数は次になる。
  - 通常run: `V + K × L + Q + N × (2R + 2T + 2D + J + S + 2U + C) + F`。
  - retry: 許可された場合は通常runから `K × L` を除く。validateのVは残る。許可されない場合はvalidateを含め0回。
  - 条件: エラーによる打ち切りなし。HTTP failureのtrialではレスポンス後の起動分が0。
- HTTPの内部リトライ回数は、レスポンス後のアドオン起動数を増やさない。
- アドオン自身の処理時間とは別に、起動・JSON・Base64・コピーの費用が毎回かかる。

### 本文のコピー数とstdout上限

次表は各レイヤーにexecを1つ置いた1trialの、通信に載る本文のコピー数。oneとotherは同じサイズBで、アドオンが本文を変更しない場合を数える。

| レイヤー | 起動数/trial | stdinの本文コピー数 | stdoutの本文コピー数 | 合計 |
| --- | --- | --- | --- | --- |
| res2res | 2 | 2 | 2 | 4 |
| res2dict | 2 | 2 | 2 | 4 |
| judgement | 1 | 2。referenceにoneとother | 0 | 2 |
| store_criterion | 1 | 2。referenceにoneとother | 0 | 2 |
| dump | 保存時2 | 4。各側でresponse.bodyとbody | 4。各側でresponse.bodyとbody | 8 |
| did_challenge | 1 | 2。referenceにoneとother | 0 | 2 |
| 合計・保存なし | 7 | 10 | 4 | 14 |
| 合計・保存あり | 9 | 14 | 8 | 22 |

- 対象: Response.bodyとDumpPayload.bodyのBase64表現。メモリアロケーションの実測回数ではない。
  - 除外: res2dictのresult、Requestの本文、その他のJSON、ホスト内部の追加コピー。
- 通信量: 保存ありでは本文だけでおおむね `22 × 4B / 3` bytes。
  - 例: oneとotherが各1 MiBなら約29.3 MiB/trial。各10 MiBなら約293 MiB/trial。
- json transformer: 両側を変換するとstdinへ本文2コピーを追加する。
  - stdout: 変換後のJSON文字列2つ。大きさは変換結果とJSONエスケープ量に依存し、Base64のコピー数へは加算しない。
- log2reqs・reqs2reqs・final・validateは、この表で数えるResponse.bodyを持たない。

stdoutの既定上限64 MiBで返せる本文サイズの目安は次のとおり。

| レイヤー | 1応答に含む本文 | 元bytesの上限目安 |
| --- | --- | --- |
| res2res | response.bodyの1コピー | 約48 MiB未満 |
| res2dict | response.bodyの1コピーとresult | 約48 MiB未満。大きいresultがあればさらに小さい |
| dump | response.bodyとbodyの2コピー | 両者が同サイズなら各約24 MiB未満 |

- 計算: `4 × ceil(B / 3)` のBase64膨張を適用する。JSON構文と他フィールドもstdout上限に含むため、48 MiB・24 MiBちょうどは通らない。
- dumpでサイズが異なる場合は、両方のBase64サイズと他フィールドの合計で判定する。
- 対応: 大きい本文を扱う設定ではmax_stdout_bytesを増やす。stderrの保持上限を変えてもstdoutの制限は変わらない。

### レビュアーの実測と見積もり

実装レビューで報告された、このマシンでの概数を次に示す。Rust本体の性能保証値ではない。[^review-bench]

| 測定対象 | 報告値 |
| --- | --- |
| 外部コマンド1起動あたりの費用 | 約25 ms |
| Pythonの起動のみの別測定 | 約41 ms |
| 100 trial、`--threads 8` | 約2.29秒 |

- 逐次見積もり: 1 trialに9回起動する場合、`9 × 25 ms = 225 ms/trial`。
  - 100 trial: `100 × 9 × 25 ms = 22.5秒`。
  - 1,000 trial: `1,000 × 9 × 25 ms = 225秒`。
  - 更新: 旧仕様の108秒と発注文の150秒は、この225秒へ統一する。
- validate: 実効設定数V回の起動を別途加える。同じ25 msを仮定すると `25 × V` ms/run。
- 測定条件: 25 msの費用と41 msのPython単体起動は別測定として扱う。
  - 制限: 同一条件の内訳として加算・減算しない。2.29秒も並列時の報告値であり、逐次見積もりの単純な8分の1ではない。
- 本文量: referenceとdumpで運ぶ本文量は異なる。大きな本文のJSON・Base64変換費用を起動回数だけから推定しない。

[^review-bench]: Opus実装レビューを受けた進行役の修正依頼から受領した実測概数。レビュー資料は `/private/tmp/claude-501/-Users-tadashi-aikawa-git-github-com-tadashi-aikawa-jumeaux-rs-wt-exec-addon/0e2a6639-bce4-4437-a846-cbba317fc92a/scratchpad/`。この改訂では再測定していない。

1起動の追加費用をpミリ秒とすると、9回起動するtrialはおおむね `9p` ミリ秒を加算する。

- 注意: 並列trialで壁時計時間を短縮できても、総CPU費用とメモリ使用量は減らない。

### 常駐方式との比較

| 方式 | 費用・利点 | 契約上の負担 |
| --- | --- | --- |
| **1呼び出し1プロセス** | 毎回の起動費用。EOFで入力と終了が確定。状態が呼び出しごとに独立 | 大量trialとPython起動が多い構成では遅い |
| 常駐1プロセスへJSON Lines | 起動費用をrun中1回へ削減 | 全trialを直列化するか、request IDによる多重化が必要。flush・状態漏れ・再起動・途中終了の規則が増える |
| 設定ごとの常駐プロセスプール | 起動費用をworker数へ抑え、並列性を確保 | workerへの割当てと状態の意味が変わる。タイムアウト時の未処理要求と副作用の再実行管理が必要 |

初版は1呼び出し1プロセスとする。

- 理由: 初版で固定する外部IFを小さくでき、失敗した呼び出しを特定しやすい。
- 理由: 任意言語で1文書を読み1文書を書くだけで動く。常駐serverの実装を利用者へ要求しない。
- 理由: 現在のtrial並列上限をそのまま使える。1trial内の外部コマンドは直列で動く。
- トレードオフ: Pythonクラスの状態保持と起動費用は互換にならない。
- 将来: 実測で起動費用が支配的な場合、別modeまたは別プロトコル版として常駐を提案する。
  - 禁止: 初版の1文書・EOF契約を、設定なしにJSON Linesへ変更しない。

## 受入条件

| 受入項目 | 合格条件 |
| --- | --- |
| 9レイヤー | passthroughと変更の双方でフィールド・順序・referenceが表どおり。組み込みとexecの混在も同じ順 |
| Option | モデルのOptionalフィールドの出力欠落とnullは同じ。入力にはnullを明示する。空object/arrayはnullと区別し、任意JSON内部のキー欠落とnull値は保持 |
| Request・Trial | form/jsonのルートはobject/nullを受理し、配列・scalarは拒否。object内部の任意JSONは保持 |
| binary | NUL・不正UTF-8を含む全byte値、空bytes、大きな本文がBase64往復で一致 |
| judgement | body=trueでもheader=falseならdifferent。分類済み差分を保持 |
| 呼び出し回数 | 保存あり9回・なし7回、HTTP failure、when=false、複数入力、許可済みretryで計算式と一致。validateの設定数V回を別途加算 |
| JSON変換 | when、encoding=null、default_encoding、replace、Content-Type保持、文字列内不正JSONがPython規則と一致 |
| エラー | 非0・不正JSON・空stdout・コマンド欠落・timeout・stdout上限で停止。後続アドオンを起動しない |
| stderr | 64 KiBを超えるtableの全行が表示される。長い未改行行も欠落・無制限バッファなし。診断保持は指定した末尾bytes以内、0なら保持なし。大量stderrだけでは停止しない |
| 並列と終了 | エラー後の新trial停止、実行中trialの完了待ち、子孫がpipeを保持した場合もtimeout、所有PIDの回収 |
| 親シグナル | validate・trial・final実行中のSIGINT/SIGTERMで新規起動を止め、全所有グループへTERM、1秒後に残存へKILL。所有外PIDに触れず、終了コード130/143 |
| retry許可 | フラグなしで全コマンド起動0回。validate・when=false・未使用log2reqsも許可判定を迂回しない。--allow-exec指定時だけ実行し、許可は次のretryへ保存しない |
| validate | 実効設定ごとに1回。options不正・mode未対応・返却形式違反で入力読み込み・HTTP・成果物作成前に停止 |
| cwd・env | PATH差し替え、空白入り引数、include、retry、env削除、inherit_env=false、明示相対cwdを確認 |
| サンプル | 追加依存なしで3つが起動。ignore_valuesの特殊パスとheader保持、tableのstderr、既存ディレクトリ保護。通信例とtransformerのnull対応、allow_nan=Falseによる非有限数拒否 |
| 移行 | 打ち間違いには候補または登録一覧を先に表示。モジュール名・非Executor・module/function混在を起動前に拒否し、旧.pyの直接指定では移行できない旨と公開URLを表示 |
| Windows | 動作保証の対象外と明記。既存serverのWindows分岐を理由に外部コマンドの保証を暗黙追加しない |
| 既存動作 | 組み込みアドオンのCLI・Report・保存ファイルの既存互換契約を維持 |

旧サンプルの不具合はPython完全一致の期待値にせず、意図と修正後の期待値を固定する。

## 決めたこと

以下は承認済み仕様の詳細。

| 選択 | 理由 |
| --- | --- |
| 全レイヤーの予約名をexecへ統一 | 組み込みと同じ配列へ置ける。名前解決に例外を増やさない |
| commandは配列、シェル展開なし | 引数の空白やメタ文字を暗黙に再解釈しない |
| 設定・stdinともキー名をoptionsへ統一 | 実行制御のconfigとアドオン固有のoptionsを区別し、受け渡し時の改名をなくす |
| bytesはBase64、elapsedは総秒数 | JSON非対応の型だけを明示的に変換する |
| Optionalのnullを明示、本文JSONのnullを保持 | 未変換・空・欠落と本文値を混同しない |
| log2reqsだけRequest配列を返す | Pythonの実際の戻り値契約に揃える |
| finalもpayloadを必須で返す | 後続finalとReport変更の機能を維持する |
| 全エラーでrunを停止、自動再試行なし | Pythonの例外停止に合わせ、副作用を二重実行しない |
| timeout既定60秒、無制限値なし | 既存の実行中trial待ちが永久停止しない。長い処理は明示的に延長できる |
| stdout既定64 MiB、stderrは全量表示・末尾64 KiB保持 | tableを切らずに診断保持メモリを制限する。両方の上限設定を分離する |
| SIGINT/SIGTERMで全所有プロセスグループを終了 | 親だけが止まって子が処理を続けることを防ぐ |
| contextへverbosity・trial_seq・sideを追加 | コマンド側のログ調整と呼び出し単位の識別に使う |
| cwdは起動ディレクトリ基準 | include位置に依存せず、既存パスとretryの運用に揃える |
| Python標準ライブラリの3雛形 | JSON処理を追加依存なしで示せる |
| 旧雛形の不整合は修正して再現 | 現行モデルで動くサンプルを提供する |
| 明示Executorは受理、その他cls_nameは拒否 | 組み込み設定と既存reportの互換性を保つ |

## 決定

6論点とも2026-11-08に案Aで確定した。以下の比較は決定時の検討記録。

### 1. 初版のプロセス寿命

決定: 1呼び出し1プロセス。決定日: 2026-11-08。

場面: 1,000trialで各レイヤーに外部処理を置くと、1呼び出し1プロセスでは数千回起動する。

```text
案A: trial → 起動 → 1要求 → 1応答 → 終了
案B: 起動 → [trial1 ⇄ 応答] → [trial2 ⇄ 応答] → 終了
案C: 起動したworker群 → trialを分配 → 終了
```

| 案 | 評価 |
| --- | --- |
| **A. 1呼び出し1プロセス** | **推奨**。失敗の分離と実装負担を優先 |
| B. 常駐1プロセスのJSON Lines | 起動費用を削減。直列化または多重化の仕様が必要 |
| C. 常駐プールのJSON Lines | 起動費用と並列性を両立。状態・再起動の仕様が最も大きい |

- 推奨理由: 初版では外部IFを小さく確定し、性能測定後に常駐の必要性を判断できる。
- 承認対象: 起動費用とPythonクラスのインスタンス状態を引き継がない差異を許容するか。

### 2. res2res/jsonの専用契約

決定: jsonを残し、transformer.commandへ置換する。決定日: 2026-11-08。

場面: 既存transformerの「bodyとencodingを受け、strを返す」処理だけを移植する。

```text
案A: json → ホストでwhen → 外部でbody変換 → ホストでencode/type更新
案B: exec → 外部でwhen・body変換・encode/type更新 → payloadを返す
```

| 案 | 評価 |
| --- | --- |
| **A. jsonを残しtransformer.commandへ置換** | **推奨**。whenとencodingの互換処理をホストに集約 |
| B. 汎用execへ統一 | 実処理のmodeはpayloadに統一。既存のjson固有処理をスクリプトへ移す必要がある |

- 推奨理由: 本文だけを変換する用途を短く書け、エンコードやheaders保持の差異を生みにくい。
- 承認対象: stdoutにJSON文字列を返す専用modeを持つか。

### 3. 親環境の既定の受け渡し

決定: inherit_env=trueを既定とし、falseへの切り替えを提供する。決定日: 2026-11-08。

場面: JumeauxのSlack通知用に設定した `SLACK_INCOMING_WEBHOOKS_URL` が、通知しないものも含め全execの子へ渡る。

```text
案A: 親環境 → 継承 → envで上書き・削除 → コマンド
案B: 空の環境 → envで明示した変数だけ → コマンド
案C: 親環境 → Jumeauxが使う秘密の変数を除去 → envで明示上書き → コマンド
```

| 案 | 評価 |
| --- | --- |
| **A. inherit_env=trueを既定** | **推奨**。従来のPythonカスタム処理と実行環境を合わせやすい |
| B. inherit_env=falseを既定 | 渡す環境を絞れる。PATHや必要変数の明示が必要 |
| C. 継承するがJumeaux自身が使う秘密の変数は除く | PATHを保ち、Slack用Webhookの不要な受け渡しを減らす。除外リストの保守が必要 |

- 推奨理由: 利用者が選んだ外部コマンドを信頼する実行モデルでは、既存運用との互換性が高い。
- 承認対象: 親の秘密を含む環境が既定で外部コマンドへ渡ることを許容するか。
- 案Cの具体化: 初版の既定除外対象は `SLACK_INCOMING_WEBHOOKS_URL`。除外設定が有効な間は、envで同名を明示指定した場合だけ渡す。
  - 限界: その他の資格情報を自動検出する方式ではない。外部コマンドのファイルアクセスも制限しない。
- 共通: 全継承・継承なしの切り替えは提供する。案Cを採用する場合は `exclude_env: [SLACK_INCOMING_WEBHOOKS_URL]` を既定とする設定を追加し、空配列で全継承へ戻せるようにする。

### 4. retryで保存された外部コマンドを実行する許可

決定: 既定で拒否し、--allow-exec指定時だけvalidateを含む外部コマンド実行を許可する。決定日: 2026-11-08。

場面: 他の人から共有されたReportをretryしたところ、Report内のcommandが利用者の権限で動く。

```text
案A: 共有Report → retry → 既定は拒否 → --allow-exec指定時だけvalidate・実処理
案B: 共有Report → 保存されたexecを無視 → 利用者がconfigを指定 → 実行
案C: 共有Report → retry → 保存されたcommandをそのまま実行
```

| 案 | 評価 |
| --- | --- |
| **A. 既定で拒否し--allow-execで許可** | **推奨**。Reportの閲覧・再試行と任意コードの実行許可を分ける |
| B. retryではexecを無視してconfigの指定を求める | 実行設定を手元で選び直せる。再現に必要な設定と新たなCLI経路の指定が必要 |
| C. 現状どおり実行 | 操作を増やさず再現できる。共有Reportからの任意コマンド実行を防げない |

- 推奨理由: 共有Reportをretryするだけで任意コマンドが動くことを防ぎ、保存設定での再現も明示許可により維持できる。
- 案Bの範囲: execだけでなくjson transformerのコマンドも除外する。新しいconfigの指定がなければ停止し、アドオンを黙って飛ばして比較を続けない。
- 承認対象: retryに `--allow-exec` を新設し、validateを含む全外部コマンド起動の前に許可を要求するか。

### 5. validateモードによる起動前検証

決定: 全exec・transformerをmode: "validate"で検証する。決定日: 2026-11-08。

場面: 1,000trialを始める前に、final/tableのoptions.columnsの誤りを見つける。

```text
案A: ホスト検証 → 各コマンドをvalidateで1回起動 → 全て成功 → HTTP・実処理
案B: ホスト検証 → HTTP・実処理 → 該当アドオンの初回実行でoptionsを検証
```

| 案 | 評価 |
| --- | --- |
| **A. validateを入れる** | **推奨**。各コマンドがoptionsを事前検証。設定数だけ起動が増える |
| B. validateを入れない | プロトコルと起動回数を抑える。後半のレイヤーの設定誤りはrunの終盤まで分からない |

- 推奨理由: Python版はアドオン生成時に設定を検証する。HTTP送信や成果物作成前に設定誤りを検出する機会を維持する。
  - 範囲: 動的な入力に依存するエラーまで事前検出できるわけではない。
- 承認対象: `mode: "validate"` と `{"valid":true}` の応答を全exec・transformerへ要求するか。
- 前提: retryの許可判定より前にはvalidateも起動しない。

### 6. Windowsでの動作を仕様上の対象外とするか

決定: 外部コマンドアドオンのWindows動作は仕様上の対象外とする。決定日: 2026-11-08。

場面: macOS・Linux向けに配布するバイナリとは別に、Windowsでローカルビルドした利用者がexecを使う。

```text
案A: 配布macOS・Linux → execの保証もmacOS・Linux → Windowsの動作は対象外
案B: 配布macOS・Linux → ローカルビルドのWindowsも保証 → 終了制御・PATHの受入を追加
```

| 案 | 評価 |
| --- | --- |
| **A. Windowsでのexec動作を仕様上の対象外と明記** | **推奨**。配布・保証・受入の範囲を揃える |
| B. Windowsでのexec動作も保証する | ローカルビルド利用者も対象になる。Windows固有のプロセス終了・PATH・引数の契約と受入が必要 |

- 推奨理由: 本案のシグナルとプロセスグループの契約はmacOS・Linux向け。Windows対応を暗黙に約束すると、同じ終了保証を提供できない。
- 既存コード: server.rsにWindows用のブラウザ起動分岐がある。
  - 区別: この分岐は外部コマンドアドオンのWindows保証を意味しない。本案の承認で既存分岐の削除は求めない。
- 承認対象: 配布対象の既決事項とは別に、外部コマンドアドオンのWindows動作を保証対象外と明記するか。
