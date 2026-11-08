//! Python addons/res2dict/block.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub force: bool,
    pub header_regexp: String,
    pub record_regexp: String,
}

use crate::{
    addons::{AddonExecutor, Res2Dict, payload::Res2DictPayload},
    compat::{Error, Result, regex::Regex},
};
use serde_json::{Map, Value};

pub struct Executor {
    force: bool,
    header: Regex,
    record: Regex,
}

impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            force: config.force,
            header: Regex::new(&config.header_regexp)?,
            record: Regex::new(&config.record_regexp)?,
        })
    }

    pub fn to_dict(&self, text: &str) -> Result<Value> {
        let mut result = Map::new();
        let mut records = Map::new();
        let mut key = Value::Null;
        // str.splitlines() は CRLF を1つの改行として扱う。
        let normalized = text.replace("\r\n", "\n");
        for line in normalized
            .split([
                '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
                '\u{2029}',
            ])
            .chain(std::iter::once(""))
        {
            if line.is_empty() {
                if !records.is_empty() {
                    let key = match &key {
                        Value::Null => "null".into(),
                        Value::String(key) => key.clone(),
                        _ => {
                            return Err(Error::new(
                                "TypeError",
                                "block header tuple cannot be a JSON key",
                            ));
                        }
                    };
                    result.insert(key, Value::Object(std::mem::take(&mut records)));
                }
                continue;
            }
            let headers = self.header.findall(line)?;
            if headers.len() == 1 {
                key = headers[0].clone();
                continue;
            }
            let matches = self.record.findall(line)?;
            if matches.len() == 1 {
                let parts = match &matches[0] {
                    Value::Array(parts) => parts.clone(),
                    Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                    _ => unreachable!(),
                };
                let first = parts
                    .first()
                    .ok_or_else(|| Error::new("IndexError", "empty block record"))?;
                records.insert(
                    first.as_str().unwrap().into(),
                    parts.get(1).cloned().unwrap_or(Value::Null),
                );
            }
        }
        Ok(Value::Object(result))
    }
}

impl AddonExecutor<Res2Dict> for Executor {
    fn exec(&self, payload: Res2DictPayload, _: &()) -> Result<Res2DictPayload> {
        self.exec_with_log(payload, &(), &|_| {})
    }
    fn exec_with_log(
        &self,
        mut payload: Res2DictPayload,
        _: &(),
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> Result<Res2DictPayload> {
        log(crate::addons::log::conversion(
            "block",
            self.force,
            payload.result.is_some(),
            &payload.response.response_type,
        ));
        if payload.result.is_none() || self.force {
            payload.result = Some(self.to_dict(&payload.response.text()?)?);
        }
        Ok(payload)
    }
}
