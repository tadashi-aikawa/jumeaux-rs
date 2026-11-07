//! Python addons/log2reqs/csv.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub encoding: String,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub keep_blank: bool,
    #[serde(default = "dialect", deserialize_with = "null_dialect")]
    pub dialect: String,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());
fn dialect() -> String {
    "excel".into()
}
crate::models::defaults::null_default_with!(null_dialect, String, dialect());

use crate::{
    compat::{
        Error, Result,
        encoding::{self, Errors},
    },
    models::{Headers, Request},
};
impl Config {
    pub fn read(&self, body: &[u8]) -> Result<Vec<Request>> {
        let delimiter = match self.dialect.as_str() {
            "excel" | "unix" => ',',
            "excel-tab" => '\t',
            _ => {
                return Err(Error::new(
                    "Error",
                    format!("unknown dialect: {}", self.dialect),
                ));
            }
        };
        let text = encoding::decode_input(body, &self.encoding)?;
        rows(&text, delimiter)?.into_iter().map(|row| {
            if row.len() > 5 { return Err(Error::new("ValueError", "CSV has more than five columns")); }
            let qs = encoding::parse_query(row.get(3).map(String::as_str).unwrap_or(""), "utf-8", self.keep_blank, Errors::Replace)?;
            let headers: Headers = encoding::parse_query(row.get(4).map(String::as_str).unwrap_or(""), "utf-8", self.keep_blank, Errors::Replace)?
                .into_iter().map(|(k, v)| (k, v[0].clone())).collect();
            let mut item = serde_json::json!({"name": row.first(), "method": row.get(1), "path": row.get(2), "qs": qs, "headers": headers});
            super::requests(serde_json::Value::Array(vec![item.take()])).map(|mut rs| rs.remove(0))
        }).collect()
    }
}

/// csv.readerの既定strict=False。引用符の後の文字、改行を含むfield、EOF中の引用も受理する。
fn rows(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut at_start = true;
    let mut started = false;
    let mut field_len = 0;
    while let Some(c) = chars.next() {
        started = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                    field_len += 1;
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
                field_len += 1;
            }
        } else if c == '"' && at_start {
            quoted = true;
            at_start = false;
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
            field_len = 0;
            at_start = true;
        } else if c == '\n' {
            if !row.is_empty() || !field.is_empty() || !at_start {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            field_len = 0;
            at_start = true;
            started = false;
        } else {
            field.push(c);
            field_len += 1;
            at_start = false;
        }
        if field_len > 131_072 {
            return Err(Error::new(
                "Error",
                "field larger than field limit (131072)",
            ));
        }
    }
    if started {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
