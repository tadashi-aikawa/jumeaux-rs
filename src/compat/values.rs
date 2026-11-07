//! 式入力用の暫定モデル。設定・モデル実装時に置換する。
use std::{collections::BTreeMap, sync::Arc};

use minijinja::{
    ErrorKind, State, Value,
    value::{Enumerator, Object},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::encoding;

pub fn serialized(value: impl Serialize) -> Value {
    Value::from(minijinja::value::Serde(value))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub method: String,
    pub path: String,
    pub qs: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<Json>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<Json>,
    pub headers: BTreeMap<String, String>,
    pub url_encoding: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            name: None,
            method: "GET".into(),
            path: String::new(),
            qs: BTreeMap::new(),
            raw: None,
            form: None,
            json: None,
            headers: BTreeMap::new(),
            url_encoding: "utf-8".into(),
        }
    }
}

impl Request {
    pub fn to_template_value(&self) -> Value {
        let base = serialized(self);
        let mut fields: BTreeMap<String, Value> = base
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
        Value::from(fields)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub body: Vec<u8>,
    pub encoding: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub url: String,
    pub status_code: u16,
    /// timedeltaの秒。elapsed_secとは別に保持する。
    pub elapsed: f64,
    pub elapsed_sec: f64,
    #[serde(rename = "type")]
    pub response_type: String,
}

impl Response {
    pub fn text(&self) -> super::Result<String> {
        encoding::response_text(&self.body, self.encoding.as_deref())
    }
    pub fn ok(&self) -> bool {
        self.status_code == 200
    }
    pub fn to_template_value(&self) -> Value {
        Value::from_object(self.clone())
    }
    fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }
}

impl Object for Response {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        Some(match key.as_str()? {
            "text" => match self.text() {
                Ok(text) => Value::from(text),
                Err(error) => error_value(error),
            },
            "body" => Value::from_bytes(self.body.clone()),
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
            key => {
                return serialized(self.as_ref())
                    .get_attr(key)
                    .ok()
                    .filter(|v| !v.is_undefined());
            }
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
struct Headers(BTreeMap<String, String>);

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
