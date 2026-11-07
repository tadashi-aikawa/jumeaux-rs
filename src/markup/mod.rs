//! Python版のres2dict/dumpに対応する、デコード済みHTML・XMLの変換。
//!
//! encoding、force、レスポンス種別の判定は呼び出し側の責務。
//! HTML5とlxmlの回復処理の既知の差異はdocs/compat-spec.mdを参照。

pub mod html;
pub mod xml;

use serde_json::{Map, Value};

/// 比較fixtureのPython例外に対応するエラー分類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    MissingHtml,
    InvalidXml,
    MalformedHtml,
    LimitExceeded,
}

impl Error {
    pub fn python_exception(self) -> &'static str {
        match self {
            Self::MissingHtml => "AttributeError",
            Self::InvalidXml => "ExpatError",
            Self::MalformedHtml => "Exception",
            Self::LimitExceeded => "LimitExceeded",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingHtml => "HTML要素がありません",
            Self::InvalidXml => "XMLが不正です",
            Self::MalformedHtml => "整形後のHTMLタグが対応していません",
            Self::LimitExceeded => "マークアップの深さまたはエンティティ展開の上限を超えました",
        })
    }
}

impl std::error::Error for Error {}

const MAX_DEPTH: usize = 1000;

fn insert_repeated(object: &mut Map<String, Value>, key: String, value: Value) {
    match object.get_mut(&key) {
        Some(Value::Array(values)) => values.push(value),
        Some(previous) => *previous = Value::Array(vec![previous.take(), value]),
        None => {
            object.insert(key, value);
        }
    }
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
