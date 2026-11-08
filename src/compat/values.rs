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
    convert_json(value, false).expect("render conversion does not reject numbers")
}

/// when用の変換では、数値を作る時点で非有限値を拒否する。
pub fn from_json_for_when(value: &Json) -> super::Result<CheckedContext> {
    super::json::validate(value)?;
    Ok(CheckedContext(convert_json(value, true)?))
}

fn convert_json(value: &Json, finite: bool) -> super::Result<Value> {
    // 子から親の順に組み立てる。深いdictの変換でも呼び出し元のスタックを消費しない。
    enum Step<'a> {
        Value(&'a Json),
        Array(usize),
        Object(&'a serde_json::Map<String, Json>, usize),
    }
    let mut pending = vec![Step::Value(value)];
    let mut converted = Vec::new();
    while let Some(step) = pending.pop() {
        let value = match step {
            Step::Value(Json::Array(values)) => {
                pending.push(Step::Array(converted.len()));
                pending.extend(values.iter().rev().map(Step::Value));
                continue;
            }
            Step::Value(Json::Object(values)) => {
                pending.push(Step::Object(values, converted.len()));
                pending.extend(values.values().rev().map(Step::Value));
                continue;
            }
            Step::Array(start) => Value::from(converted.split_off(start)),
            Step::Object(map, start) => {
                Value::from_pairs(map.keys().cloned().zip(converted.split_off(start)))
            }
            Step::Value(Json::Null) => Value::from(()),
            Step::Value(Json::Bool(value)) => Value::from(*value),
            Step::Value(Json::String(value)) => Value::from(value.clone()),
            Step::Value(Json::Number(value)) => {
                if let Some(value) = value.as_i64() {
                    Value::from(value)
                } else if let Some(value) = value.as_u64() {
                    Value::from(value)
                } else if let Some(value) = value.as_i128() {
                    Value::from(value)
                } else {
                    let value = value.as_str().parse::<f64>().expect("JSON number");
                    if finite && !value.is_finite() {
                        return Err(super::json::nonfinite_error());
                    }
                    Value::from(value)
                }
            }
        };
        converted.push(value);
    }
    Ok(converted.pop().expect("JSON root"))
}

/// 検査済みの条件式入力。内部値を外から直接構築・変更できない。
///
/// 生JSONはwhenへ渡せない。
/// ```compile_fail
/// use jumeaux::compat::template::TemplateEngine;
/// TemplateEngine::new().when("True", serde_json::json!({})).unwrap();
/// ```
/// MiniJinja Valueも検査が必要。
/// ```compile_fail
/// use jumeaux::compat::template::TemplateEngine;
/// TemplateEngine::new().when("True", minijinja::context!(x => 1)).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct CheckedContext(Value);

impl CheckedContext {
    /// 個別に検査済みの値を結合する。大きいdictを再検査しない。
    pub fn from_pairs<K: Into<String>>(values: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self(Value::from_pairs(
            values.into_iter().map(|(key, value)| (key.into(), value.0)),
        ))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// when専用。CheckedContextとその参照以外には実装できない。
pub trait TemplateInput: sealed::Sealed {
    fn into_when_value(self) -> Value;
}
impl sealed::Sealed for CheckedContext {}
impl sealed::Sealed for &CheckedContext {}
impl TemplateInput for CheckedContext {
    fn into_when_value(self) -> Value {
        self.0
    }
}
impl TemplateInput for &CheckedContext {
    fn into_when_value(self) -> Value {
        self.0.clone()
    }
}

/// renderは既存の表示互換を保つ。whenの検査契約とは分離する。
pub trait RenderInput {
    fn into_value(self) -> Value;
}
impl RenderInput for Json {
    fn into_value(self) -> Value {
        from_json(&self)
    }
}
impl RenderInput for &Json {
    fn into_value(self) -> Value {
        from_json(self)
    }
}
impl RenderInput for Value {
    fn into_value(self) -> Value {
        self
    }
}
impl RenderInput for &Value {
    fn into_value(self) -> Value {
        self.clone()
    }
}

/// ネイティブのMiniJinja値を使う呼び出し元は、context構築時に一度だけ検査する。
pub fn checked_context(value: Value) -> super::Result<CheckedContext> {
    validate_template(&value)?;
    Ok(CheckedContext(value))
}

fn validate_template(value: &Value) -> super::Result<()> {
    if let Some(option) = value.downcast_object_ref::<PythonOption>() {
        if let Ok(value) = &option.0 {
            validate_template(value)?;
        }
    } else if let Some(response) = value.downcast_object_ref::<Response>() {
        if !response.elapsed.is_finite() || !response.elapsed_sec.is_finite() {
            return Err(super::json::nonfinite_error());
        }
    } else {
        match value.kind() {
            minijinja::value::ValueKind::Number => {
                if f64::try_from(value.clone()).is_ok_and(|n| !n.is_finite()) {
                    return Err(super::json::nonfinite_error());
                }
            }
            minijinja::value::ValueKind::Map => {
                if let Ok(keys) = value.try_iter() {
                    for key in keys {
                        if let Ok(value) = value.get_item(&key) {
                            validate_template(&value)?;
                        }
                    }
                }
            }
            minijinja::value::ValueKind::Seq => {
                if let Ok(values) = value.try_iter() {
                    for value in values {
                        validate_template(&value)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
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
pub(super) struct PythonOption(super::Result<Value>);

/// judgement の dict_one/dict_other は TOption をそのまま公開する。
pub fn optional(value: Option<&Json>) -> super::Result<CheckedContext> {
    Ok(CheckedContext(Value::from_object(PythonOption(Ok(value
        .map(from_json_for_when)
        .transpose()?
        .map(|value| value.0)
        .unwrap_or_else(|| Value::from(())))))))
}

impl Object for PythonOption {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let name = key.as_str()?;
        if name == "value" {
            return Some(match &self.0 {
                Ok(value) => value.clone(),
                Err(error) => error_value(error.clone()),
            });
        }
        if matches!(
            name,
            "get" | "get_or" | "is_none" | "is_some" | "any" | "map" | "flat_map"
        ) {
            let option = self.clone();
            let name = name.to_owned();
            return Some(Value::from_function(
                move |state: &mut State, args: minijinja::value::Rest<Value>| {
                    option.call_method(state, &name, &args)
                },
            ));
        }
        None
    }

    fn call_method(
        self: &Arc<Self>,
        state: &mut State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        let value = self.0.as_ref().map_err(|e| {
            minijinja::Error::new(ErrorKind::InvalidOperation, e.message.clone())
                .with_source(e.clone())
        })?;
        let names: &[&str] = match name {
            "get" | "is_none" | "is_some" | "any" => &[],
            "get_or" => &["default"],
            "map" | "flat_map" => &["func"],
            _ => return Err(minijinja::Error::from(ErrorKind::UnknownMethod)),
        };
        let (positional, kwargs): (&[Value], minijinja::value::Kwargs) =
            minijinja::value::from_args(args)?;
        if positional.len() > names.len() {
            return Err(minijinja::Error::new(
                ErrorKind::InvalidOperation,
                "too many Option arguments",
            ));
        }
        let mut arguments = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let arg = if let Some(arg) = positional.get(index) {
                if kwargs.has(name) {
                    return Err(minijinja::Error::new(
                        ErrorKind::InvalidOperation,
                        "duplicate Option argument",
                    ));
                }
                arg.clone()
            } else if kwargs.has(name) {
                kwargs.get::<Value>(name)?
            } else {
                return Err(minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    "missing Option argument",
                ));
            };
            arguments.push(arg);
        }
        kwargs.assert_all_used().map_err(|error| {
            minijinja::Error::new(ErrorKind::InvalidOperation, error.to_string())
        })?;
        match (name, arguments.as_slice()) {
            ("get", []) => Ok(value.clone()),
            ("get_or", [default]) => Ok(if value.is_none() {
                default.clone()
            } else {
                value.clone()
            }),
            ("is_none", []) => Ok(Value::from(value.is_none())),
            ("is_some" | "any", []) => Ok(Value::from(!value.is_none())),
            ("map" | "flat_map", [function]) => {
                let mapped = if value.is_none() {
                    Value::from(())
                } else {
                    if function.is_undefined() {
                        return Err(minijinja::Error::from(ErrorKind::UndefinedError));
                    }
                    let result = function.call(state, std::slice::from_ref(value))?;
                    if name == "flat_map" {
                        match result.call_method(state, "get", &[]) {
                            Err(error) if error.kind() == ErrorKind::UnknownMethod => {
                                return Err(minijinja::Error::new(
                                    ErrorKind::InvalidOperation,
                                    "flat_map result has no get method",
                                )
                                .with_source(super::Error::new(
                                    "AttributeError",
                                    "flat_map result has no get method",
                                )));
                            }
                            result => result?,
                        }
                    } else {
                        result
                    }
                };
                Ok(Value::from_object(PythonOption(Ok(mapped))))
            }
            _ => Err(minijinja::Error::from(ErrorKind::UnknownMethod)),
        }
    }
}
