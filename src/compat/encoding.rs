//! 経路ごとのstrict/replaceと、Python codec名をencoding_rsへ写すアダプター。
use std::{collections::HashMap, sync::LazyLock};

use indexmap::IndexMap;

use super::{Error, Result, values::Request};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errors {
    Strict,
    Replace,
}

enum Codec {
    Ascii,
    Latin1,
    Utf8Sig,
    Utf16 { big_endian: Option<bool> },
    ShiftJis,
    Cp932,
    EucJp,
    EucKr,
    Latin15,
    Cp437,
    Utf32 { big_endian: Option<bool> },
    Other(&'static encoding_rs::Encoding),
}

fn codec(label: &str) -> Result<Codec> {
    let name = label.to_ascii_lowercase().replace(['-', ' ', '.'], "_");
    Ok(match name.as_str() {
        "ascii" | "us_ascii" | "646" => Codec::Ascii,
        "latin1" | "latin_1" | "iso8859_1" | "iso_8859_1" | "l1" => Codec::Latin1,
        "utf_8_sig" => Codec::Utf8Sig,
        "utf16" | "utf_16" => Codec::Utf16 { big_endian: None },
        "utf_16_le" | "utf16le" | "utf_16le" => Codec::Utf16 {
            big_endian: Some(false),
        },
        "utf_16_be" | "utf16be" | "utf_16be" => Codec::Utf16 {
            big_endian: Some(true),
        },
        "shift_jis" | "shiftjis" | "sjis" | "s_jis" => Codec::ShiftJis,
        "cp932" | "ms932" | "mskanji" | "ms_kanji" => Codec::Cp932,
        "utf8" | "utf_8" | "u8" => Codec::Other(encoding_rs::UTF_8),
        "euc_jp" | "eucjp" | "ujis" => Codec::EucJp,
        "euc_kr" | "euckr" | "ks_c_5601_1987" | "ksx1001" => Codec::EucKr,
        "cp949" | "ms949" | "uhc" => Codec::Other(encoding_rs::EUC_KR),
        "iso8859_15" | "iso_8859_15" | "latin9" | "latin_9" | "l9" => Codec::Latin15,
        "mac_roman" | "macroman" | "macintosh" => Codec::Other(encoding_rs::MACINTOSH),
        "cp437" | "437" | "ibm437" => Codec::Cp437,
        "utf32" | "utf_32" => Codec::Utf32 { big_endian: None },
        "utf32le" | "utf_32_le" | "utf_32le" => Codec::Utf32 {
            big_endian: Some(false),
        },
        "utf32be" | "utf_32_be" | "utf_32be" => Codec::Utf32 {
            big_endian: Some(true),
        },
        "iso2022_jp" | "iso_2022_jp" | "iso2022jp" => Codec::Other(encoding_rs::ISO_2022_JP),
        _ => Codec::Other(
            encoding_rs::Encoding::for_label(label.as_bytes())
                .or_else(|| encoding_rs::Encoding::for_label(name.replace('_', "-").as_bytes()))
                .filter(|encoding| *encoding != encoding_rs::REPLACEMENT)
                .ok_or_else(|| Error::new("LookupError", format!("unknown encoding: {label}")))?,
        ),
    })
}

pub fn decode(body: &[u8], label: &str, errors: Errors) -> Result<String> {
    let failure = || Error::new("UnicodeDecodeError", format!("invalid {label} bytes"));
    match codec(label)? {
        Codec::Ascii | Codec::Latin1 => {
            let ascii = matches!(codec(label)?, Codec::Ascii);
            body.iter()
                .map(|&b| {
                    if ascii && b > 127 {
                        if errors == Errors::Strict {
                            Err(failure())
                        } else {
                            Ok('\u{fffd}')
                        }
                    } else {
                        Ok(char::from(b))
                    }
                })
                .collect()
        }
        Codec::Utf8Sig => decode(
            body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body),
            "utf-8",
            errors,
        ),
        Codec::ShiftJis => decode_shift_jis(body, errors),
        Codec::Cp932 => decode_cp932(body, errors),
        Codec::EucJp => decode_euc_jp(body, errors),
        Codec::EucKr => decode_euc_kr(body, errors),
        Codec::Latin15 => Ok(body.iter().map(|&b| latin15(b)).collect()),
        Codec::Cp437 => Ok(body.iter().map(|&b| cp437(b)).collect()),
        Codec::Utf32 { big_endian } => {
            let (big, bytes) = match (big_endian, body) {
                (None, [0xff, 0xfe, 0, 0, rest @ ..]) => (false, rest),
                (None, [0, 0, 0xfe, 0xff, rest @ ..]) => (true, rest),
                (endian, bytes) => (endian.unwrap_or(cfg!(target_endian = "big")), bytes),
            };
            bytes
                .chunks(4)
                .map(|chunk| {
                    let c = <[u8; 4]>::try_from(chunk).ok().and_then(|unit| {
                        char::from_u32(if big {
                            u32::from_be_bytes(unit)
                        } else {
                            u32::from_le_bytes(unit)
                        })
                    });
                    c.or((errors == Errors::Replace).then_some('\u{fffd}'))
                        .ok_or_else(failure)
                })
                .collect()
        }
        Codec::Utf16 { big_endian } => {
            let (big, bytes) = match (big_endian, body) {
                (None, [0xff, 0xfe, rest @ ..]) => (false, rest),
                (None, [0xfe, 0xff, rest @ ..]) => (true, rest),
                (endian, bytes) => (endian.unwrap_or(cfg!(target_endian = "big")), bytes),
            };
            let mut output = String::new();
            let unit = |i| {
                if big {
                    u16::from_be_bytes([bytes[i], bytes[i + 1]])
                } else {
                    u16::from_le_bytes([bytes[i], bytes[i + 1]])
                }
            };
            let mut i = 0;
            while i < bytes.len() {
                let mut consumed = 2;
                let c = if i + 1 >= bytes.len() {
                    consumed = 1;
                    None
                } else {
                    let first = unit(i);
                    if (0xd800..=0xdbff).contains(&first) {
                        if i + 3 >= bytes.len() {
                            consumed = bytes.len() - i;
                            None
                        } else if (0xdc00..=0xdfff).contains(&unit(i + 2)) {
                            consumed = 4;
                            char::from_u32(
                                0x10000
                                    + ((u32::from(first) - 0xd800) << 10)
                                    + u32::from(unit(i + 2))
                                    - 0xdc00,
                            )
                        } else {
                            None
                        }
                    } else {
                        char::from_u32(u32::from(first))
                    }
                };
                match c {
                    Some(c) => output.push(c),
                    None if errors == Errors::Replace => output.push('\u{fffd}'),
                    None => return Err(failure()),
                }
                i += consumed;
            }
            Ok(output)
        }
        selected => {
            let encoding = match selected {
                Codec::Other(encoding) => encoding,
                _ => unreachable!(),
            };
            // decode()はBOMで指定codecを上書きするため使わない。
            let (text, failed) = encoding.decode_without_bom_handling(body);
            if failed && errors == Errors::Strict {
                return Err(failure());
            }
            // WHATWGは未定義バイトをC1制御文字へ写すが、Pythonは変換失敗にする。
            text.chars()
                .map(|c| {
                    if undefined_windows(encoding, c) {
                        if errors == Errors::Strict {
                            Err(failure())
                        } else {
                            Ok('\u{fffd}')
                        }
                    } else {
                        Ok(c)
                    }
                })
                .collect()
        }
    }
}

fn undefined_windows(encoding: &encoding_rs::Encoding, c: char) -> bool {
    (encoding == encoding_rs::WINDOWS_1252
        && matches!(c, '\u{81}' | '\u{8d}' | '\u{8f}' | '\u{90}' | '\u{9d}'))
        || (encoding == encoding_rs::WINDOWS_1254
            && matches!(
                c,
                '\u{81}' | '\u{8d}' | '\u{8e}' | '\u{8f}' | '\u{90}' | '\u{9d}' | '\u{9e}'
            ))
}

fn latin15(b: u8) -> char {
    match b {
        0xa4 => '€',
        0xa6 => 'Š',
        0xa8 => 'š',
        0xb4 => 'Ž',
        0xb8 => 'ž',
        0xbc => 'Œ',
        0xbd => 'œ',
        0xbe => 'Ÿ',
        b => char::from(b),
    }
}

fn cp437(b: u8) -> char {
    const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
    if b < 128 {
        char::from(b)
    } else {
        HIGH.chars().nth(usize::from(b - 128)).unwrap()
    }
}

fn decode_cp932(body: &[u8], errors: Errors) -> Result<String> {
    let mut output = String::new();
    let mut i = 0;
    while i < body.len() {
        let first = body[i];
        if matches!(first, 0xa0 | 0xfd..=0xff) {
            output.push(
                char::from_u32(if first == 0xa0 {
                    0xf8f0
                } else {
                    0xf8f1 + u32::from(first - 0xfd)
                })
                .unwrap(),
            );
            i += 1;
            continue;
        }
        let width = if ((0x81..=0x9f).contains(&first) || (0xe0..=0xfc).contains(&first))
            && i + 1 < body.len()
        {
            2
        } else {
            1
        };
        let (text, failed) =
            encoding_rs::SHIFT_JIS.decode_without_bom_handling(&body[i..i + width]);
        if failed {
            if errors == Errors::Strict {
                return Err(Error::new("UnicodeDecodeError", "invalid cp932 bytes"));
            }
            output.push('\u{fffd}');
            i += 1;
        } else {
            output.push_str(&text);
            i += width;
        }
    }
    Ok(output)
}

fn decode_euc_jp(body: &[u8], errors: Errors) -> Result<String> {
    let mut output = String::new();
    let mut i = 0;
    while i < body.len() {
        let first = body[i];
        let width = match first {
            0..=0x7f => 1,
            0x8f => 3,
            _ => 2,
        };
        let invalid = first == 0xad || first >= 0xf5 || i + width > body.len();
        let (text, failed) =
            encoding_rs::EUC_JP.decode_without_bom_handling(&body[i..(i + width).min(body.len())]);
        if invalid || failed {
            if errors == Errors::Strict {
                return Err(Error::new("UnicodeDecodeError", "invalid euc_jp bytes"));
            }
            output.push('\u{fffd}');
            i += 1;
        } else {
            output.extend(text.chars().map(sjis_from_windows));
            i += width;
        }
    }
    Ok(output)
}

// encoding_rsのencoderが選ばないPythonのNEC選択IBM拡張/JIS X 0212を逆引きする。
static CP932_EXTENSION: LazyLock<HashMap<char, Vec<u8>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for first in 0xed..=0xee {
        for second in 0x40..=0xfc {
            let bytes = vec![first, second];
            if let Ok(text) = decode_cp932(&bytes, Errors::Strict) {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    map.entry(c).or_insert(bytes);
                }
            }
        }
    }
    map
});

static JIS0212: LazyLock<HashMap<char, Vec<u8>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for first in 0xa1..=0xfe {
        for second in 0xa1..=0xfe {
            let bytes = vec![0x8f, first, second];
            if let Ok(text) = decode_euc_jp(&bytes, Errors::Strict) {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    map.entry(c).or_insert(bytes);
                }
            }
        }
    }
    map
});

fn encode_japanese(text: &str, errors: Errors, euc: bool) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for c in text.chars() {
        if !euc {
            if let Some(bytes) = CP932_EXTENSION.get(&c) {
                output.extend(bytes);
                continue;
            }
            if ('\u{f8f0}'..='\u{f8f3}').contains(&c) {
                output.push(if c == '\u{f8f0}' {
                    0xa0
                } else {
                    0xfd + (u32::from(c) - 0xf8f1) as u8
                });
                continue;
            }
        }
        let mapped = sjis_to_windows(c);
        let encoding = if euc {
            encoding_rs::EUC_JP
        } else {
            encoding_rs::SHIFT_JIS
        };
        let mut buf = [0; 4];
        let (bytes, _, failed) = encoding.encode(mapped.encode_utf8(&mut buf));
        let invalid = failed
            || (!euc && matches!(c, '¥' | '‾'))
            || (euc
                && ((mapped == c && sjis_from_windows(c) != c)
                    || decode_euc_jp(&bytes, Errors::Strict).is_err()));
        if invalid {
            if euc && let Some(bytes) = JIS0212.get(&c) {
                output.extend(bytes);
                continue;
            }
            if errors == Errors::Strict {
                return Err(Error::new(
                    "UnicodeEncodeError",
                    if euc {
                        "text cannot be encoded as euc_jp"
                    } else {
                        "text cannot be encoded as cp932"
                    },
                ));
            }
            output.push(b'?');
        } else {
            output.extend_from_slice(&bytes);
        }
    }
    Ok(output)
}

const HANGUL_INITIAL: &str = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";
const HANGUL_FINAL: &str = "ㅤㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ";

fn decode_euc_kr(body: &[u8], errors: Errors) -> Result<String> {
    let mut output = String::new();
    let mut i = 0;
    while i < body.len() {
        // Pythonのeuc_krはKS X 1001にない音節をfiller+3 jamoで表す。
        if body[i..].starts_with(b"\xa4\xd4") {
            let end = (i + 8).min(body.len());
            let (jamo, failed) = encoding_rs::EUC_KR.decode_without_bom_handling(&body[i + 2..end]);
            let jamo: Vec<_> = jamo.chars().collect();
            let syllable = if !failed && jamo.len() == 3 {
                HANGUL_INITIAL
                    .chars()
                    .position(|c| c == jamo[0])
                    .zip(HANGUL_FINAL.chars().position(|c| c == jamo[2]))
                    .filter(|_| ('ㅏ'..='ㅣ').contains(&jamo[1]))
                    .and_then(|(initial, final_)| {
                        char::from_u32(
                            0xac00
                                + initial as u32 * 588
                                + (u32::from(jamo[1]) - 0x314f) * 28
                                + final_ as u32,
                        )
                    })
            } else {
                None
            };
            if let Some(c) = syllable {
                output.push(c);
            } else if errors == Errors::Strict {
                return Err(Error::new("UnicodeDecodeError", "invalid euc_kr syllable"));
            } else {
                output.push('\u{fffd}');
            }
            i = end;
            continue;
        }
        let width = if body[i] < 128 { 1 } else { 2 };
        let bytes = &body[i..(i + width).min(body.len())];
        let invalid =
            width == 2 && (bytes.len() != 2 || bytes.iter().any(|b| !(0xa1..=0xfe).contains(b)));
        let (text, failed) = encoding_rs::EUC_KR.decode_without_bom_handling(bytes);
        if invalid || failed {
            if errors == Errors::Strict {
                return Err(Error::new("UnicodeDecodeError", "invalid euc_kr bytes"));
            }
            output.push('\u{fffd}');
            i += 1;
        } else {
            output.push_str(&text);
            i += width;
        }
    }
    Ok(output)
}

fn encode_euc_kr(text: &str, errors: Errors) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for c in text.chars() {
        let mut buf = [0; 4];
        let (bytes, _, failed) = encoding_rs::EUC_KR.encode(c.encode_utf8(&mut buf));
        if !failed
            && bytes.len() == 2
            && bytes.iter().any(|b| !(0xa1..=0xfe).contains(b))
            && ('가'..='힣').contains(&c)
        {
            let index = u32::from(c) - 0xac00;
            let jamo = [
                HANGUL_INITIAL.chars().nth((index / 588) as usize).unwrap(),
                char::from_u32(0x314f + index % 588 / 28).unwrap(),
                HANGUL_FINAL.chars().nth((index % 28) as usize).unwrap(),
            ];
            output.extend_from_slice(b"\xa4\xd4");
            for c in jamo {
                output.extend_from_slice(&encoding_rs::EUC_KR.encode(c.encode_utf8(&mut buf)).0);
            }
        } else if failed || (bytes.len() == 2 && bytes.iter().any(|b| !(0xa1..=0xfe).contains(b))) {
            if errors == Errors::Strict {
                return Err(Error::new(
                    "UnicodeEncodeError",
                    "text cannot be encoded as euc_kr",
                ));
            }
            output.push(b'?');
        } else {
            output.extend_from_slice(&bytes);
        }
    }
    Ok(output)
}

fn sjis_to_windows(c: char) -> char {
    match c {
        '〜' => '～',
        '‖' => '∥',
        '−' => '－',
        '¢' => '￠',
        '£' => '￡',
        '¬' => '￢',
        c => c,
    }
}

fn sjis_from_windows(c: char) -> char {
    match c {
        '～' => '〜',
        '∥' => '‖',
        '－' => '−',
        '￠' => '¢',
        '￡' => '£',
        '￢' => '¬',
        c => c,
    }
}

fn decode_shift_jis(body: &[u8], errors: Errors) -> Result<String> {
    let mut output = String::new();
    let mut i = 0;
    while i < body.len() {
        let first = body[i];
        let width = if first <= 0x7f || (0xa1..=0xdf).contains(&first) {
            1
        } else if ((0x81..=0x9f).contains(&first) || (0xe0..=0xea).contains(&first))
            && first != 0x87
            && i + 1 < body.len()
        {
            2
        } else {
            0
        };
        let (text, failed) =
            encoding_rs::SHIFT_JIS.decode_without_bom_handling(&body[i..i + width]);
        if width == 0 || failed {
            if errors == Errors::Strict {
                return Err(Error::new("UnicodeDecodeError", "invalid shift_jis bytes"));
            }
            output.push('\u{fffd}');
            i += 1;
        } else {
            output.extend(text.chars().map(sjis_from_windows));
            i += width;
        }
    }
    Ok(output)
}

fn encode_shift_jis(text: &str, errors: Errors) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for c in text.chars() {
        let mapped = match c {
            '〜' => '～',
            '‖' => '∥',
            '−' => '－',
            '¢' => '￠',
            '£' => '￡',
            '¬' => '￢',
            c => c,
        };
        let mut buf = [0; 4];
        let (bytes, _, failed) = encoding_rs::SHIFT_JIS.encode(mapped.encode_utf8(&mut buf));
        let invalid = failed
            || (mapped == c && sjis_from_windows(c) != c)
            || decode_shift_jis(&bytes, Errors::Strict).is_err();
        if invalid {
            if errors == Errors::Strict {
                return Err(Error::new(
                    "UnicodeEncodeError",
                    "text cannot be encoded as shift_jis",
                ));
            }
            output.push(b'?');
        } else {
            output.extend_from_slice(&bytes);
        }
    }
    Ok(output)
}

pub fn encode(text: &str, label: &str, errors: Errors) -> Result<Vec<u8>> {
    let failure = || {
        Error::new(
            "UnicodeEncodeError",
            format!("text cannot be encoded as {label}"),
        )
    };
    match codec(label)? {
        Codec::Ascii | Codec::Latin1 => {
            let max = if matches!(codec(label)?, Codec::Ascii) {
                127
            } else {
                255
            };
            text.chars()
                .map(|c| {
                    if u32::from(c) <= max {
                        Ok(c as u8)
                    } else if errors == Errors::Replace {
                        Ok(b'?')
                    } else {
                        Err(failure())
                    }
                })
                .collect()
        }
        Codec::Utf8Sig => Ok([b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat()),
        Codec::ShiftJis => encode_shift_jis(text, errors),
        Codec::Cp932 => encode_japanese(text, errors, false),
        Codec::EucJp => encode_japanese(text, errors, true),
        Codec::EucKr => encode_euc_kr(text, errors),
        selected @ (Codec::Latin15 | Codec::Cp437) => text
            .chars()
            .map(|c| {
                (0..=255u8)
                    .find(|&b| {
                        if matches!(selected, Codec::Latin15) {
                            latin15(b) == c
                        } else {
                            cp437(b) == c
                        }
                    })
                    .or((errors == Errors::Replace).then_some(b'?'))
                    .ok_or_else(failure)
            })
            .collect(),
        Codec::Utf32 { big_endian } => {
            let big = big_endian.unwrap_or(cfg!(target_endian = "big"));
            let mut output = Vec::new();
            if big_endian.is_none() {
                output.extend_from_slice(if big {
                    b"\0\0\xfe\xff"
                } else {
                    b"\xff\xfe\0\0"
                });
            }
            for c in text.chars() {
                output.extend_from_slice(&if big {
                    u32::from(c).to_be_bytes()
                } else {
                    u32::from(c).to_le_bytes()
                });
            }
            Ok(output)
        }
        Codec::Utf16 { big_endian } => {
            let big = big_endian.unwrap_or(cfg!(target_endian = "big"));
            let mut output = Vec::new();
            if big_endian.is_none() {
                output.extend_from_slice(if big { b"\xfe\xff" } else { b"\xff\xfe" });
            }
            for unit in text.encode_utf16() {
                output.extend_from_slice(&if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            Ok(output)
        }
        selected => {
            let encoding = match selected {
                Codec::Other(e) => e,
                _ => unreachable!(),
            };
            let (bytes, _, failed) = encoding.encode(text);
            if !failed && !text.chars().any(|c| undefined_windows(encoding, c)) {
                return Ok(bytes.into_owned());
            }
            if errors == Errors::Strict {
                return Err(failure());
            }
            // encoding_rsの数値文字参照をPythonの?へ置き換える。
            // 文字ごとのbytes連結ではISO-2022-JPの状態が壊れるので、先に文字列を補正する。
            let safe: String = text
                .chars()
                .map(|c| {
                    let mut buf = [0; 4];
                    if encoding.encode(c.encode_utf8(&mut buf)).2 || undefined_windows(encoding, c)
                    {
                        '?'
                    } else {
                        c
                    }
                })
                .collect();
            Ok(encoding.encode(&safe).0.into_owned())
        }
    }
}

pub fn response_text(body: &[u8], encoding: Option<&str>) -> Result<String> {
    decode(body, encoding.unwrap_or("utf-8"), Errors::Replace)
}

pub fn transcode(body: &[u8], source: &str, destination: &str) -> Result<Vec<u8>> {
    encode(
        &decode(body, source, Errors::Strict)?,
        destination,
        Errors::Strict,
    )
}

/// open(..., encoding=...)と同じ改行変換。ファイル種別のparserより前に呼ぶ。
pub fn decode_input(body: &[u8], encoding: &str) -> Result<String> {
    if matches!(codec(encoding)?, Codec::Utf16 { big_endian: None })
        && !body.is_empty()
        && !body.starts_with(b"\xff\xfe")
        && !body.starts_with(b"\xfe\xff")
    {
        return Err(Error::new(
            "UnicodeError",
            "UTF-16 stream does not start with BOM",
        ));
    }
    if matches!(codec(encoding)?, Codec::Utf32 { big_endian: None })
        && !body.is_empty()
        && !body.starts_with(b"\xff\xfe\0\0")
        && !body.starts_with(b"\0\0\xfe\xff")
    {
        return Err(Error::new(
            "UnicodeError",
            "UTF-32 stream does not start with BOM",
        ));
    }
    Ok(decode(body, encoding, Errors::Strict)?
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}

pub fn content_encodings(body: &[u8]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    static PATTERNS: LazyLock<[::regex::bytes::Regex; 3]> = LazyLock::new(|| {
        [
            r#"(?i-u)<meta.*?charset=["']*(.+?)["'>]"#,
            r#"(?i-u)<meta.*?content=["']*;?charset=(.+?)["'>]"#,
            r#"(?-u)^<\?xml.*?encoding=["']*(.+?)["'>]"#,
        ]
        .map(|pattern| ::regex::bytes::Regex::new(pattern).expect("constant regex"))
    });
    for re in PATTERNS.iter() {
        for captures in re.captures_iter(body) {
            result.push(decode(&captures[1], "utf-8", Errors::Strict)?);
        }
    }
    Ok(result)
}

pub fn detect(body: &[u8]) -> String {
    if body.starts_with(b"\xef\xbb\xbf") {
        return "UTF-8-SIG".into();
    }
    if body.starts_with(b"\xff\xfe") || body.starts_with(b"\xfe\xff") {
        return "UTF-16".into();
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    detector.feed(body, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
    if encoding == encoding_rs::ISO_2022_JP {
        return "ISO-2022-JP".into();
    }
    if body.is_ascii() {
        return "ascii".into();
    }
    match encoding.name() {
        "UTF-8" => "utf-8".into(),
        "Shift_JIS" => "CP932".into(),
        name => name.into(),
    }
}

pub fn decide_encoding(
    content_type: Option<&str>,
    body: &[u8],
    selected: Option<&str>,
    default: Option<&str>,
    apparent: Option<&str>,
) -> Result<Option<String>> {
    let ct = content_type.unwrap_or("");
    if ct.contains("octet-stream") {
        return Ok(None);
    }
    if let Some(selected) = selected.filter(|s| !s.is_empty())
        && !(ct.contains("text") && selected == "ISO-8859-1")
    {
        return Ok(Some(selected.into()));
    }
    Ok(Some(
        content_encodings(body)?
            .into_iter()
            .next()
            .or_else(|| default.filter(|s| !s.is_empty()).map(String::from))
            .or_else(|| apparent.map(String::from))
            .unwrap_or_else(|| detect(body)),
    ))
}

/// requests.utils.get_encoding_from_headersの優先順を含むHTTP入口。
pub fn http_encoding(
    content_type: Option<&str>,
    body: &[u8],
    default: Option<&str>,
) -> Result<Option<String>> {
    let ct = content_type.unwrap_or("");
    let mut selected = None;
    for item in ct.split(';').skip(1) {
        if let Some((key, value)) = item.split_once('=')
            && key.trim().eq_ignore_ascii_case("charset")
        {
            selected = Some(value.trim().trim_matches(['\'', '"']));
        }
    }
    let selected = selected.or_else(|| {
        if ct.contains("text") {
            Some("ISO-8859-1")
        } else if ct.contains("application/json") {
            Some("utf-8")
        } else {
            None
        }
    });
    decide_encoding(content_type, body, selected, default, None)
}

fn unquote(value: &str, encoding: &str, errors: Errors) -> Result<String> {
    // urllib.unquoteはASCIIの連続部分をbytesへ戻してdecodeする。
    // Shift_JISの第2byteは生のASCIIでもよく、生のUnicodeは変換対象外。
    if !value.contains('%') {
        return Ok(value.replace('+', " "));
    }
    let mut result = String::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii() {
            let mut decoded = Vec::new();
            while i < bytes.len() && bytes[i].is_ascii() {
                if i + 2 < bytes.len()
                    && bytes[i] == b'%'
                    && let (Some(a), Some(b)) = (
                        (bytes[i + 1] as char).to_digit(16),
                        (bytes[i + 2] as char).to_digit(16),
                    )
                {
                    decoded.push((a * 16 + b) as u8);
                    i += 3;
                    continue;
                }
                decoded.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
                i += 1;
            }
            result.push_str(&decode(&decoded, encoding, errors)?);
            continue;
        }
        let c = value[i..].chars().next().unwrap();
        result.push(if c == '+' { ' ' } else { c });
        i += c.len_utf8();
    }
    Ok(result)
}

pub fn parse_query(
    query: &str,
    encoding: &str,
    keep_blank: bool,
    errors: Errors,
) -> Result<IndexMap<String, Vec<String>>> {
    let mut output: IndexMap<String, Vec<String>> = IndexMap::new();
    for item in query.split('&').filter(|s| !s.is_empty()) {
        let (key, value) = item.split_once('=').unwrap_or((item, ""));
        if !keep_blank && value.is_empty() {
            continue;
        }
        output
            .entry(unquote(key, encoding, errors)?)
            .or_default()
            .push(unquote(value, encoding, errors)?);
    }
    Ok(output)
}

pub fn guess_url_encoding(query: &str, candidates: &[String]) -> Result<Option<String>> {
    for candidate in candidates {
        match parse_query(query, candidate, false, Errors::Strict) {
            Ok(_) => return Ok(Some(candidate.clone())),
            Err(error) if error.kind == "UnicodeDecodeError" => (),
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub fn quote_plus(text: &str, encoding: &str) -> Result<String> {
    let mut result = String::new();
    for b in encode(text, encoding, Errors::Strict)? {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                result.push(b as char)
            }
            b' ' => result.push('+'),
            b => {
                use std::fmt::Write;
                write!(result, "%{b:02X}").unwrap();
            }
        }
    }
    Ok(result)
}

pub fn plain_requests(
    body: &[u8],
    encoding: &str,
    candidates: &[String],
    keep_blank: bool,
) -> Result<Vec<Request>> {
    let text = decode_input(body, encoding)?;
    text.split_terminator('\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let line = line.trim_end();
            let mut pieces = line.split('?');
            let path = pieces.next().unwrap_or("").to_string();
            let query = pieces.next().unwrap_or("");
            let url_encoding =
                guess_url_encoding(query, candidates)?.unwrap_or_else(|| "utf-8".into());
            let qs = parse_query(query, &url_encoding, keep_blank, Errors::Replace)?;
            Ok(Request {
                path,
                qs,
                url_encoding,
                ..Request::default()
            })
        })
        .collect()
}
