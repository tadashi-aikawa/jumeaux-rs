//! 数値の字句を維持し、重複キーを追加で拒否する通信専用入口。
use crate::compat::{Error, Result};
use serde_json::Value;
use std::collections::HashSet;

pub fn parse(bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes).map_err(|_| protocol("$: expected UTF-8 JSON"))?;
    let value = crate::compat::json::parse(text)
        .map_err(|_| protocol("$: expected one finite JSON document without BOM"))?;
    // JSON文法は確認済み。文字列直後のコロンは必ずオブジェクトキー。
    let mut objects: Vec<HashSet<String>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                objects.push(HashSet::new());
                i += 1;
            }
            b'}' => {
                objects.pop();
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                let end = i;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if bytes.get(i) == Some(&b':') {
                    let key: String =
                        serde_json::from_str(&text[start..end]).expect("validated JSON string");
                    if !objects.last_mut().expect("validated object").insert(key) {
                        return Err(protocol("$: duplicate object key"));
                    }
                }
            }
            _ => i += 1,
        }
    }
    Ok(value)
}
pub fn protocol(message: impl Into<String>) -> Error {
    Error::new("ExecAddonProtocolError", message)
}
