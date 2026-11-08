//! Python addons/res2res/json_sort.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub items: Vec<Sorter>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub footprints_tag: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sorter {
    pub when: String,
    pub targets: Vec<Target>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_keys: Option<Vec<String>>,
}

use crate::{
    addons::{AddonExecutor, Res2Res, payload::Res2ResPayload},
    compat::{
        Error, Result,
        encoding::{self, Errors},
        regex::{ExactRegex, Regex},
        template::TemplateEngine,
    },
};
use num_bigint::BigInt;
use num_traits::FromPrimitive;
use serde_json::Value;
use std::cmp::Ordering;

struct CompiledSorter {
    when: String,
    targets: Vec<(ExactRegex, Option<Vec<String>>)>,
}
pub struct Executor {
    items: Vec<CompiledSorter>,
    footprints_tag: Option<String>,
    engine: TemplateEngine,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        let items = config
            .items
            .into_iter()
            .map(|sorter| {
                engine.load(&format!("{{{{{}}}}}", sorter.when))?;
                Ok(CompiledSorter {
                    when: sorter.when,
                    targets: sorter
                        .targets
                        .into_iter()
                        .map(|t| Ok((Regex::new_exact(&t.path)?, t.sort_keys)))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            items,
            footprints_tag: config.footprints_tag,
            engine,
        })
    }
}
impl AddonExecutor<Res2Res> for Executor {
    fn exec(&self, payload: Res2ResPayload, _: &()) -> Result<Res2ResPayload> {
        self.exec_with_log(payload, &(), &|_| {})
    }
    fn exec_with_log(
        &self,
        payload: Res2ResPayload,
        _: &(),
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> Result<Res2ResPayload> {
        if payload.response.response_type != "json" {
            log(crate::addons::log::AddonLog::JsonSortSkipped);
            return Ok(payload);
        }
        let text = payload.response.text()?;
        let depth = crate::compat::json::scan(&text)?;
        crate::compat::json::with_stack(depth, || self.sort(payload, &text))
    }
}
impl Executor {
    fn sort(&self, mut payload: Res2ResPayload, text: &str) -> Result<Res2ResPayload> {
        let original = crate::compat::json::parse(text)?;
        let mut sorted = original.clone();
        let context = crate::compat::values::checked_context(payload.req.to_dict_value())?;
        for sorter in &self.items {
            if self.engine.when(&sorter.when, &context)?.truthy() {
                if !sorted.is_object() && !sorted.is_array() {
                    // Python _list_sort iterates strings; other scalar roots are not iterable.
                    sorted = match sorted.as_str() {
                        Some(s) => {
                            Value::Array(s.chars().map(|c| Value::String(c.to_string())).collect())
                        }
                        None => {
                            return Err(Error::new("TypeError", "json_sort root is not iterable"));
                        }
                    };
                }
                traverse(&mut sorted, "root", &sorter.targets)?;
            }
        }
        let encoding = payload
            .response
            .encoding
            .as_deref()
            .ok_or_else(|| Error::new("TypeError", "json_sort requires an encoding"))?;
        payload.response.body =
            encoding::encode(&json_string(&sorted), encoding, Errors::Replace)?.into();
        if !equal(&original, &sorted)
            && let Some(tag) = &self.footprints_tag
        {
            payload.tags.push(tag.clone());
        }
        Ok(payload)
    }
}

fn traverse(
    value: &mut Value,
    location: &str,
    targets: &[(ExactRegex, Option<Vec<String>>)],
) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                // json_sort uses raw keys, unlike DeepDiff's quoted/escaped path format.
                traverse(value, &format!("{location}<'{key}'>"), targets)?;
            }
        }
        Value::Array(list) => {
            let mut target = None;
            for (path, keys) in targets {
                if path.exact_match(location)? {
                    target = Some(keys);
                    break;
                }
            }
            for (i, value) in list.iter_mut().enumerate() {
                traverse(value, &format!("{location}<{i}>"), targets)?;
            }
            if let Some(keys) = target {
                let keyed = list
                    .iter()
                    .map(|value| {
                        let key = match keys {
                            Some(keys) => Value::Array(
                                keys.iter()
                                    .map(|key| {
                                        value
                                            .as_object()
                                            .ok_or_else(|| {
                                                Error::new(
                                                    "TypeError",
                                                    "sort_keys requires an object",
                                                )
                                            })?
                                            .get(key)
                                            .cloned()
                                            .ok_or_else(|| Error::new("KeyError", key))
                                    })
                                    .collect::<Result<_>>()?,
                            ),
                            None if value.is_object() || value.is_array() => {
                                Value::String(json_string(value))
                            }
                            None => value.clone(),
                        };
                        Ok((key, value.clone()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                *list = stable_sort(keyed)?
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect();
            }
        }
        _ => {}
    }
    Ok(())
}

// 比較失敗をそのまま返す安定マージソート。sort_byへ不整合な比較結果を返さない。
fn stable_sort(mut values: Vec<(Value, Value)>) -> Result<Vec<(Value, Value)>> {
    if values.len() < 2 {
        return Ok(values);
    }
    let length = values.len();
    let right = values.split_off(length / 2);
    let mut left = stable_sort(values)?.into_iter().peekable();
    let mut right = stable_sort(right)?.into_iter().peekable();
    let mut result = Vec::with_capacity(length);
    while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
        if compare(&a.0, &b.0)? != Ordering::Greater {
            result.push(left.next().unwrap());
        } else {
            result.push(right.next().unwrap());
        }
    }
    result.extend(left);
    result.extend(right);
    Ok(result)
}

fn integer(value: &Value) -> Option<BigInt> {
    match value {
        Value::Bool(v) => Some(BigInt::from(u8::from(*v))),
        Value::Number(n) if !n.as_str().contains(['.', 'e', 'E']) => n.as_str().parse().ok(),
        _ => None,
    }
}
fn number_compare(a: &Value, b: &Value) -> Ordering {
    match (integer(a), integer(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(a), None) => {
            let b = float(b);
            if let Some(truncated) = BigInt::from_f64(b) {
                a.cmp(&truncated)
                    .then_with(|| 0.0_f64.partial_cmp(&b.fract()).unwrap())
            } else if b.is_sign_positive() {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (None, Some(_)) => number_compare(b, a).reverse(),
        (None, None) => float(a).partial_cmp(&float(b)).unwrap(),
    }
}

fn float(value: &Value) -> f64 {
    value.as_number().unwrap().as_str().parse().unwrap()
}
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            number_compare(a, b) == Ordering::Equal
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|b| equal(v, b)))
        }
        _ => a == b,
    }
}
fn compare(a: &Value, b: &Value) -> Result<Ordering> {
    match (a, b) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            Ok(number_compare(a, b))
        }
        (Value::String(a), Value::String(b)) => Ok(a.cmp(b)),
        (Value::Array(a), Value::Array(b)) => {
            for (a, b) in a.iter().zip(b) {
                if !equal(a, b) {
                    return compare(a, b);
                }
            }
            Ok(a.len().cmp(&b.len()))
        }
        _ => Err(Error::new("TypeError", "json_sort keys are not orderable")),
    }
}

/// json.dumps(ensure_ascii=False) の空白と挿入順を保つ。
fn json_string(value: &Value) -> String {
    match value {
        Value::Number(number) if number.as_str().contains(['.', 'e', 'E']) => {
            let value = float(value);
            crate::compat::template::python_float_repr(value)
        }
        Value::Number(_) => integer(value).unwrap().to_string(),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap(),
                    json_string(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(list) => format!(
            "[{}]",
            list.iter().map(json_string).collect::<Vec<_>>().join(", ")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}
