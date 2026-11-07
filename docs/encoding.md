# 文字コードの互換境界

`src/compat/encoding.rs` にcodecの選択・変換を集約する。入力形式やアドオンのparserは、この境界で得た文字列・bytesを受け取る。

## 経路

| 経路 | API | 決定・推定 | BOM | 変換失敗 |
| --- | --- | --- | --- | --- |
| plain入力 | `plain_requests` | `encoding`、既定UTF-8。URL候補は別処理 | 指定codecの規則 | 読み込みはstrict。改行はLFへ統一 |
| CSV・JSON・YAML入力 | `decode_input` | 各アドオンの`encoding`、既定UTF-8。ファイルの推定はしない | 指定codecの規則 | strict。形式のparseは後続の入力系アドオンで実装 |
| HTTP応答 | `http_encoding` | 下表の優先順 | 推定時にUTF-8-SIG・UTF-16を識別 | codec名の選択とdecodeは別操作 |
| `Response.text` | `response_text` | 応答encoding、なしならUTF-8 | 指定codecの規則 | replaceでU+FFFD。不明codecはLookupError |
| URL候補 | `guess_url_encoding` | 候補順にqueryをstrictでparseし、最初に成功した元の候補名 | パーセントbytesに指定codecを適用 | UnicodeDecodeErrorだけ次候補へ。不明codecはLookupError。候補全滅はNone |
| URL読解 | `parse_query` | 選択されたURL codec、既定UTF-8 | 同上 | 通常はreplace。空値を保持するかは`keep_blank` |
| URL生成 | `quote_plus` | Requestの`url_encoding` | 指定codecの規則 | strict。空白は`+`、非ASCII bytesは大文字の`%HH` |
| `dump/encoding` | `transcode` | payloadのsource → 設定のdestination | 両codecの規則 | decode・encodeともstrict |
| 本文整形後の保存 | `encode(..., Errors::Replace)` | payloadのencoding | 指定codecの規則 | Pythonと同じ`?` |
| `output.encoding` | `encode(..., Errors::Strict)` | output設定、既定UTF-8 | 指定codecの規則 | PythonのTextIOWrapperと同じstrict。実ストリームへの接続はCLI統合で行う |

- URL候補判定は、`keep_blank_values=False` の`urllib.parse.parse_qs`に対応する。
  - 規則: 空値のpairはdecode前に除外する。生のUnicodeを候補codecで再エンコードしない。
- `decode_input`はPythonのテキストファイル読み込みに対応する。
  - 規則: CRLF・CRをLFへ変換する。`utf-16`ストリームでBOMがなければUnicodeError。
- `dump_html` fixtureはHTML整形の前後にある文字コード境界を検証する。
  - 入力: Pythonの実関数から採取した`decoded_text`と`formatted_text`。
  - 残件: Rust側のHTML整形・保存アドオンとの接続は、markupとchallengeの統合時に検証する。

## HTTP応答の決定順

| 順位 | 条件 | 結果 |
| --- | --- | --- |
| 1 | Content-Typeに`octet-stream`を含む | encodingなし |
| 2 | requests相当のヘッダ処理でcodecを選択 | charset指定を優先。textの既定はISO-8859-1、application/jsonの既定はutf-8 |
| 2の例外 | Content-Typeに`text`を含み、選択値が厳密に`ISO-8859-1` | 明示charsetでも読み飛ばす |
| 3 | 本文にHTML meta・XML宣言 | `content_encodings`の先頭。charset meta、pragma meta、先頭XML宣言の順に収集 |
| 4 | 空でないdefault_response_encoding | 指定名を保持 |
| 5 | 上記なし | chardetngの推定。BOMを先に判定 |

`decide_encoding`はHTTPライブラリが選んだencodingとapparent_encodingを受け取れる。`http_encoding`はそのヘッダ選択と推定も行う入口とする。

## codecと別名

| codec | 主な受理名 | decode | encode |
| --- | --- | --- | --- |
| UTF-8 | utf-8、utf8、utf_8、u8 | BOMをU+FEFFとして保持 | BOMなし |
| UTF-8-SIG | utf-8-sig、utf_8_sig | 先頭BOMだけ除く | BOMを付ける |
| UTF-16 | utf-16、utf16、utf_16 | BOMでendianを選択。bytes単体でBOMなしならnative endian | native endianのBOMを付ける |
| UTF-16 LE/BE | utf-16-le、utf-16-be、utf16le、utf16be | 指定endian。BOMも文字として保持 | BOMなし |
| ASCII | ascii、us-ascii、646 | 0x80以上をstrictエラーまたはU+FFFD | 非ASCIIをstrictエラーまたは`?` |
| Latin-1 | latin1、latin-1、iso-8859-1、iso8859-1、l1 | 0x00〜FFを同値のUnicodeへ | U+00FFまで。Windows-1252へは寄せない |
| Shift_JIS | shift_jis、shiftjis、sjis、s_jis | JISの対応。0x8160はU+301C。CP932拡張を受理しない | JISの対応。CP932だけの文字をstrictエラーまたは`?` |
| CP932 | cp932、ms932、mskanji、ms_kanji | encoding_rsのShift_JIS。0x8160はU+FF5E | 同左 |
| EUC-JP | euc-jp、euc_jp、eucjp、ujis | encoding_rsのEUC-JP | 同左 |
| ISO-2022-JP | iso2022_jp、iso-2022-jp、iso2022jp | 状態付きdecode | 状態を保ってencode。非対応文字はstrictエラーまたは`?` |
| その他 | encoding_rsが受理するlabel | Encoding Standardの対応表 | 同左 |
| 非対応 | encoding_rsにも専用アダプターにもないcodec | LookupError | LookupError |

- 主な別名は大小を区別しない。
- decodeの指定codecは別種のBOMで上書きしない。
- strictのエラー分類はUnicodeDecodeError・UnicodeEncodeError。
- 表の外にあるPython固有codecの互換性は保証しない。
  - 例: UTF-32、EUC-JIS-2004は専用アダプター未実装。
- CP932などの全Unicode対応表をPythonと網羅比較したものではない。
  - 検証: 日本語・波ダッシュ・NEC拡張・不正bytesをfixtureで固定する。

## 推定の差異

`detect-euc`だけPython版の推定名と一致しない。

| fixture | Python charset-normalizer | Rust chardetng |
| --- | --- | --- |
| detect-euc | euc_jis_2004 | EUC-JP |

- テストはPythonの期待値を保持し、Rustの結果も明示的にassertする。
- この差異の許容は未決。
  - 詳細: [互換仕様の未決](compat-spec.md#未決)
- ASCII・UTF-8・CP932・ISO-2022-JP・UTF-8 BOMの採取例は一致する。
  - 制限: 推定器が異なるため、任意の入力で一致するという保証ではない。

## 依存crate

| crate | lockの版 | 用途 |
| --- | --- | --- |
| encoding_rs | 0.8.44 | 文字コードの変換 |
| chardetng | 1.0.0 | HTTP本文の推定 |
| regex | 1.13.1 | bytes上のmeta・XML宣言の抽出 |

- encoding_rsの標準encodeは、表現できない文字を数値文字参照にする。
  - 補正: Pythonの`?`へ事前置換し、その文字列をまとめてencodeする。
- encoding_rsはUTF-16をencodeしない。
  - 補正: RustのUTF-16コードユニットからendian・BOMを明示してbytesを作る。
- これらのライブラリ差は、差異を許容した決定ではない。
  - 詳細: [互換仕様の未決](compat-spec.md#未決)

参照: [encoding_rs 0.8.44](https://docs.rs/encoding_rs/0.8.44/encoding_rs/)、[chardetng 1.0.0](https://docs.rs/chardetng/1.0.0/chardetng/)。
