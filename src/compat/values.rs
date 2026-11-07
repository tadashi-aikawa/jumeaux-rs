//! 正式モデルをPython互換の式入力へ変換するアダプター。
use indexmap::IndexMap;
use std::sync::Arc;

use minijinja::{
    ErrorKind, State, Value,
    value::{Enumerator, Object},
};
use serde::Serialize;
use serde_json::Value as Json;

pub use crate::models::{Request, Response};

pub fn serialized(value: impl Serialize) -> Value {
    match serde_json::to_value(value) {
        Ok(value) => from_json(&value),
        Err(error) => Value::from(minijinja::Error::new(
            ErrorKind::InvalidOperation,
            error.to_string(),
        )),
    }
}

/// arbitrary_precisionのserde内部表現をテンプレートへ漏らさない。
pub fn from_json(value: &Json) -> Value {
    match value {
        Json::Null => Value::from(()),
        Json::Bool(value) => Value::from(*value),
        Json::String(value) => Value::from(value.clone()),
        Json::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::from(value)
            } else if let Some(value) = value.as_u64() {
                Value::from(value)
            } else if let Some(value) = value.as_i128() {
                Value::from(value)
            } else {
                Value::from(value.as_str().parse::<f64>().expect("JSON number"))
            }
        }
        Json::Array(values) => Value::from(values.iter().map(from_json).collect::<Vec<_>>()),
        Json::Object(values) => {
            Value::from_pairs(values.iter().map(|(k, v)| (k.clone(), from_json(v))))
        }
    }
}

/// JSONは数値を変換し、式用Objectを持つMiniJinja Valueはそのまま渡す。
/// 任意のserdeモデルにはserialized()を使う。
pub trait TemplateInput {
    fn into_value(self) -> Value;
}
impl TemplateInput for Json {
    fn into_value(self) -> Value {
        from_json(&self)
    }
}
impl TemplateInput for &Json {
    fn into_value(self) -> Value {
        from_json(self)
    }
}
impl TemplateInput for Value {
    fn into_value(self) -> Value {
        self
    }
}
impl TemplateInput for &Value {
    fn into_value(self) -> Value {
        self.clone()
    }
}

impl Request {
    /// filter/rename/json_sortのto_dict()経路。Noneのキーを省略する。
    pub fn to_dict_value(&self) -> Value {
        serialized(self)
    }

    /// res2res/jsonのreq経路。Optionの.get()などを公開する。
    pub fn to_template_value(&self) -> Value {
        let base = serialized(self);
        let mut fields: IndexMap<String, Value> = base
            .try_iter()
            .unwrap()
            .map(|k| (k.to_string(), base.get_item(&k).unwrap()))
            .collect();
        for (name, value) in [
            ("name", serialized(&self.name)),
            ("raw", serialized(&self.raw)),
            ("form", serialized(&self.form)),
            ("json", serialized(&self.json)),
        ] {
            fields.insert(name.into(), Value::from_object(PythonOption(Ok(value))));
        }
        Value::from_pairs(fields)
    }
}

impl Response {
    /// res2res/type はプロパティを持たない to_dict() を条件式に渡す。
    pub fn to_dict_value(&self) -> Value {
        let mut fields = vec![
            ("body", Value::from_bytes(self.body.to_vec())),
            ("headers", Value::from_object(Headers(self.headers.clone()))),
            ("url", Value::from(self.url.clone())),
            ("status_code", Value::from(self.status_code)),
            ("elapsed", Value::from_object(Timedelta(self.elapsed))),
            ("elapsed_sec", Value::from(self.elapsed_sec)),
            ("type", Value::from(self.response_type.clone())),
        ];
        if let Some(encoding) = &self.encoding {
            fields.push(("encoding", Value::from(encoding.clone())));
        }
        Value::from_pairs(fields)
    }

    pub fn to_template_value(&self) -> Value {
        Value::from_object(self.clone())
    }
}

impl Object for Response {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        Some(match key.as_str()? {
            "text" => match self.text() {
                Ok(text) => Value::from(text),
                Err(error) => error_value(error),
            },
            "body" => Value::from_bytes(self.body.to_vec()),
            "status_code" => Value::from(self.status_code),
            "url" => Value::from(self.url.clone()),
            "elapsed_sec" => Value::from(self.elapsed_sec),
            "type" => Value::from(self.response_type.clone()),
            "elapsed" => Value::from_object(Timedelta(self.elapsed)),
            "ok" => Value::from(self.ok()),
            "byte" => Value::from(self.body.len()),
            "headers" => Value::from_object(Headers(self.headers.clone())),
            "encoding" => Value::from_object(PythonOption(Ok(serialized(&self.encoding)))),
            "content_type" => {
                Value::from_object(PythonOption(Ok(Value::from(self.content_type()))))
            }
            "mime_type" => Value::from_object(PythonOption(Ok(Value::from(
                self.content_type().map(|s| s.split(';').next().unwrap()),
            )))),
            // Python版はlist > intでTypeError。承認なしに既存不具合を直さない。
            "charset" => {
                if self.content_type().is_some() {
                    error_value(super::Error::new(
                        "TypeError",
                        "Response.charset compares list and int",
                    ))
                } else {
                    Value::from_object(PythonOption(Ok(Value::from(()))))
                }
            }
            _ => return None,
        })
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(&[
            "body",
            "encoding",
            "headers",
            "url",
            "status_code",
            "elapsed",
            "elapsed_sec",
            "type",
            "text",
            "ok",
            "byte",
            "content_type",
            "mime_type",
            "charset",
        ])
    }
}

#[derive(Debug)]
struct Headers(IndexMap<String, String>);

fn error_value(error: super::Error) -> Value {
    Value::from(
        minijinja::Error::new(ErrorKind::InvalidOperation, error.message.clone())
            .with_source(error),
    )
}

#[derive(Debug)]
struct Timedelta(f64);

impl Object for Timedelta {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        Some(match key.as_str()? {
            "days" => Value::from((self.0 / 86400.0).floor() as i64),
            "seconds" => Value::from(self.0.floor().rem_euclid(86400.0) as i64),
            "microseconds" => Value::from((self.0.rem_euclid(1.0) * 1_000_000.0).round() as i64),
            _ => return None,
        })
    }
    fn call_method(
        self: &Arc<Self>,
        _state: &mut State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        match (name, args) {
            ("total_seconds", []) => Ok(Value::from(self.0)),
            _ => Err(minijinja::Error::from(ErrorKind::UnknownMethod)),
        }
    }
}

impl Object for Headers {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.0
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key.as_str().unwrap_or("")))
            .map(|(_, value)| Value::from(value.clone()))
    }
    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Values(self.0.keys().cloned().map(Value::from).collect())
    }
}

#[derive(Debug)]
struct PythonOption(super::Result<Value>);

/// judgement の dict_one/dict_other は TOption をそのまま公開する。
pub fn optional(value: Option<&Json>) -> Value {
    Value::from_object(PythonOption(Ok(value
        .map(from_json)
        .unwrap_or_else(|| Value::from(())))))
}

impl Object for PythonOption {
    fn call_method(
        self: &Arc<Self>,
        _state: &mut State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        let value = self.0.as_ref().map_err(|e| {
            minijinja::Error::new(ErrorKind::InvalidOperation, e.message.clone())
                .with_source(e.clone())
        })?;
        match (name, args) {
            ("get", []) => Ok(value.clone()),
            ("get_or", [default]) => Ok(if value.is_none() {
                default.clone()
            } else {
                value.clone()
            }),
            ("is_none", []) => Ok(Value::from(value.is_none())),
            ("is_some", []) => Ok(Value::from(!value.is_none())),
            _ => Err(minijinja::Error::from(ErrorKind::UnknownMethod)),
        }
    }
}
