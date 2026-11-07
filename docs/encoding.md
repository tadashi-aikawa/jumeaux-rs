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
  - 規則: CRLF・CRをLFへ変換する。`utf-16`・`utf-32`ストリームでBOMがなければUnicodeError。
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
| ASCII | ascii、us-ascii、646、ansi_x3.4-1968 | 0x80以上をstrictエラーまたはU+FFFD | 非ASCIIをstrictエラーまたは`?` |
| Latin-1 | latin1、latin-1、iso-8859-1、iso8859-1、l1、cp819、iso-ir-100、csisolatin1、ibm819 | 0x00〜FFを同値のUnicodeへ | U+00FFまで。Windows-1252へは寄せない |
| Latin-5 | latin5、l5、iso-8859-9、iso8859_9 | Latin-1との6文字の差を補正。0x80はU+0080 | 同じ対応表を逆引き。Windows-1254へは寄せない |
| Shift_JIS | shift_jis、shiftjis、sjis、s_jis | JISの対応。0x8160はU+301C。CP932拡張を受理しない | JISの対応。CP932だけの文字をstrictエラーまたは`?` |
| CP932 | cp932、ms932、mskanji、ms_kanji、Windows-31J、cswindows31j | 0x8160はU+FF5E。A0・FD〜FFはU+F8F0〜F8F3 | ￢は81CA、ⅰ・髙はEEEF・EEE0。U+E000〜E757はF040〜F9FC。¥・‾はstrictエラーか`?` |
| EUC-JP | euc-jp、euc_jp、eucjp、ujis | A1C1はU+301CなどJIS写像へ補正。NEC拡張を拒否 | JIS写像とJIS X 0212の3バイト列を生成 |
| EUC-KR | euc_kr、euckr | KS X 1001とfiller付きjamoから音節を復元 | KS X 1001にないHangul音節はfillerと3つのjamoで表現 |
| CP949 | cp949、ms949、uhc、ks_c_5601-1987、ksx1001 | encoding_rsのEUC_KR拡張込み。82A0は`궇` | 同左 |
| Latin-9 | iso8859_15、iso-8859-15、latin9、l9 | Latin-1との8文字の差を補正 | 同じ対応表を逆引き |
| Mac Roman | mac-roman、macroman、macintosh | encoding_rsのMacintosh | 同左 |
| CP437 | cp437、437、ibm437 | 専用256文字表 | 同じ対応表を逆引き |
| UTF-32 | utf-32、utf32、utf_32 | BOMでendianを選択。bytes単体のBOMなしはnative endian | native endianのBOMを付ける |
| UTF-32 LE/BE | utf-32-le、utf-32-be、utf32le、utf32be | 指定endian。BOMも文字として保持 | BOMなし |
| Windows-1252/1254 | windows-1252、windows-1254、cp1252、cp1254 | Pythonの未定義バイトはstrictエラーかU+FFFD | 未定義C1制御文字はstrictエラーか`?` |
| ISO-2022-JP | iso2022_jp、iso-2022-jp、iso2022jp | JIS写像へ補正。13区・75〜7E行・ESC ( Iをstrictで拒否 | JIS写像へ補正して状態付きencode。№・℡・Ⅰ・半角カナはstrictエラーまたは`?` |
| 非対応 | 上記の明示対応にないcodec。例: koi8-r、gbk、euc_jis_2004 | LookupError。WHATWGのlabelへfallbackしない | LookupError |
| 明示拒否 | hz-gb-2312、iso-2022-kr、replacement | LookupError | LookupError |

- 主な別名は大小を区別しない。
- Windows-31J・cswindows31jはPythonでもCP932の別名として解決される。
  - 検証: 別名をそのままPythonのencode/decodeへ渡してfixtureを採取する。`python_codec: cp932`は解決先を示す。
- decodeの指定codecは別種のBOMで上書きしない。
- strictのエラー分類はUnicodeDecodeError・UnicodeEncodeError。
- 表の外にあるPython固有codecの互換性は保証しない。
  - 例: EUC-JIS-2004は専用アダプター未実装。
- CP932などの全Unicode対応表をPythonと網羅比較したものではない。
  - 検証: 日本語・波ダッシュ・NEC拡張・不正bytesをfixtureで固定する。

## decodeの未決の差異

| codec・入力bytes | 条件 | Python | Rust |
| --- | --- | --- | --- |
| EUC-JP `8FA2B7` | strict・replace | `~` | `〜` |
| EUC-JP `8FA2` | replace | U+FFFD 1文字 | U+FFFD 2文字 |
| CP949 `8180`・`A180`・`C680` | replace | U+FFFD 2文字 | U+FFFD 1文字 |
| ISO-2022-JP `ESC $ B 2D21 ESC ( B`などのNEC拡張 | replace | U+FFFD 1文字 | U+FFFD 2文字 |
| ISO-2022-JP `ESC ( I 31 ESC ( B` | replace | U+FFFD＋`1` | U+FFFD 3文字＋`1` |

これらはJIS X 0212の写像と、不正列を消費した後の読み直し位置の差異。Python期待値を保持し、`rust_expected`でRustの現出力を固定する。

- ISO-2022-JPの生の制御文字にはstrictでも差異がある。
  - encode: PythonはSO・SI・ESCをそのままbytesへ変換する。RustはUnicodeEncodeError。
  - decode: Pythonは単独のSO・SIをそのまま文字へ変換する。RustはUnicodeDecodeError。単独のESCのdecodeは両者ともUnicodeDecodeError。

## 推定の差異

`detect-euc`だけPython版の推定名と一致しない。

| fixture | Python charset-normalizer | Rust chardetng |
| --- | --- | --- |
| detect-euc | euc_jis_2004 | EUC-JP |

- テストはPythonの期待値を保持し、Rustの結果も明示的にassertする。
- 進行役の決定案Aにより、推定codec名のPython一致は保証しない。
  - 条件: EUC-JPでdecodeした本文の写像はPythonに合わせる。
- ASCII・UTF-8・CP932・ISO-2022-JP・UTF-8 BOMの採取例は一致する。
  - 制限: 推定器が異なるため、任意の入力で一致するという保証ではない。

## 依存crate

| crate | lockの版 | 用途 |
| --- | --- | --- |
| encoding_rs | 0.8.44 | 文字コードの変換 |
| chardetng | 1.0.0 | HTTP本文の推定 |
| regex | 1.13.1 | bytes上のmeta・XML宣言の抽出 |
| indexmap | 2.14.2 | query・headersの挿入順の保持 |

- encoding_rsの標準encodeは、表現できない文字を数値文字参照にする。
  - 補正: Pythonの`?`へ事前置換し、その文字列をまとめてencodeする。
- encoding_rsはUTF-16をencodeしない。
  - 補正: RustのUTF-16コードユニットからendian・BOMを明示してbytesを作る。
- これらのライブラリ差は、差異を許容した決定ではない。
  - 詳細: [互換仕様の未決](compat-spec.md#未決)

参照: [encoding_rs 0.8.44](https://docs.rs/encoding_rs/0.8.44/encoding_rs/)、[chardetng 1.0.0](https://docs.rs/chardetng/1.0.0/chardetng/)。
