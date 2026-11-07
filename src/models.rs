//! 設定・レポート・アドオン間で共有するモデル。
//! OwlMixinのTOptionはNoneを省略する。読込時の空文字はNone、空配列・空辞書は残す。
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::Arc;

pub use crate::diff::DiffKeys;
pub type Object = Map<String, Value>;
pub type Headers = IndexMap<String, String>;
pub type Queries = IndexMap<String, Vec<String>>;
pub type CognitionDiffs = IndexMap<String, DiffKeys>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
}

pub(crate) fn optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.filter(|s| !s.is_empty()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(default)]
    pub method: HttpMethod,
    pub path: String,
    #[serde(default)]
    pub qs: Queries,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub form: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub json: Option<Value>,
    #[serde(default)]
    pub headers: Headers,
    #[serde(default = "url_encoding")]
    pub url_encoding: String,
}

fn url_encoding() -> String {
    "utf-8".into()
}
pub(crate) fn utf8() -> String {
    "utf8".into()
}
pub(crate) fn one() -> i64 {
    1
}
fn executor_class() -> String {
    "Executor".into()
}
fn notifier_username() -> String {
    "jumeaux".into()
}

impl Default for Request {
    fn default() -> Self {
        Self {
            name: None,
            method: HttpMethod::Get,
            path: String::new(),
            qs: Queries::new(),
            raw: None,
            form: None,
            json: None,
            headers: Headers::new(),
            url_encoding: url_encoding(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub body: Arc<[u8]>,
    pub encoding: Option<String>,
    pub headers: Headers,
    pub url: String,
    pub status_code: u16,
    /// timedeltaの秒。丸めたelapsed_secとは別に保持する。
    pub elapsed: f64,
    pub elapsed_sec: f64,
    #[serde(rename = "type")]
    pub response_type: String,
}

impl Response {
    pub fn text(&self) -> crate::compat::Result<String> {
        crate::compat::encoding::response_text(&self.body, self.encoding.as_deref())
    }
    pub fn ok(&self) -> bool {
        self.status_code == 200
    }
    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathReplace {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCustomization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<Queries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessPoint {
    pub name: String,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathReplace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryCustomization>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub proxy: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_response_encoding: Option<String>,
    #[serde(default)]
    pub headers: Headers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSummary {
    pub response_dir: String,
    #[serde(default = "utf8")]
    pub encoding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    pub threads: i64,
    pub processes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifierType {
    Slack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notifier {
    #[serde(rename = "type")]
    pub notifier_type: NotifierType,
    #[serde(default = "one")]
    pub version: i64,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub channel: Option<String>,
    #[serde(default = "notifier_username")]
    pub username: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub icon_emoji: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub use_blocks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Addon {
    pub name: String,
    #[serde(default = "executor_class")]
    pub cls_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub config: Option<Object>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub include: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Addons {
    pub log2reqs: Addon,
    #[serde(default)]
    pub reqs2reqs: Vec<Addon>,
    #[serde(default)]
    pub res2res: Vec<Addon>,
    #[serde(default)]
    pub res2dict: Vec<Addon>,
    #[serde(default)]
    pub judgement: Vec<Addon>,
    #[serde(default)]
    pub store_criterion: Vec<Addon>,
    #[serde(default)]
    pub dump: Vec<Addon>,
    #[serde(default)]
    pub did_challenge: Vec<Addon>,
    #[serde(default, rename = "final")]
    pub final_addons: Vec<Addon>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Same,
    Different,
    Failure,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusCounts {
    pub same: u64,
    pub different: u64,
    pub failure: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Time {
    pub start: String,
    pub end: String,
    pub elapsed_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub one: AccessPoint,
    pub other: AccessPoint,
    pub status: StatusCounts,
    pub tags: Vec<String>,
    pub time: Time,
    pub concurrency: Concurrency,
    pub output: OutputSummary,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_encoding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseSummary {
    pub url: String,
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_sec: Option<f64>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_type: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub mime_type: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub encoding: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub file: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub prop_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub response_header: Option<Object>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub seq: u64,
    pub name: String,
    pub tags: Vec<String>,
    pub headers: Headers,
    pub queries: Queries,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub form: Option<Object>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "owl_option")]
    pub json: Option<Object>,
    pub one: ResponseSummary,
    pub other: ResponseSummary,
    pub method: HttpMethod,
    pub path: String,
    pub request_time: String,
    pub status: Status,
    /// Noneは解析不能、省略する。Some(empty)は差分なし、{}を保存する。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs_by_cognition: Option<CognitionDiffs>,
}

impl Trial {
    /// Python版retryで失われるrawも復元する。URL encodingの保存は別契約。
    pub fn to_request(&self) -> Request {
        Request {
            name: Some(self.name.clone()),
            method: self.method,
            path: self.path.clone(),
            qs: self.queries.clone(),
            headers: self.headers.clone(),
            raw: self.raw.clone(),
            form: self.form.clone().map(Value::Object),
            json: self.json.clone().map(Value::Object),
            ..Request::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub version: String,
    pub key: String,
    pub title: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifiers: Option<IndexMap<String, Notifier>>,
    pub summary: Summary,
    pub trials: Vec<Trial>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addons: Option<Addons>,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_hash: Option<String>,
}

/// OwlMixinは任意のdict/list内でもNoneを除外する。入力オブジェクトは変更しない。
fn owl_option<T: Serialize, S: serde::Serializer>(
    value: &Option<T>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
    omit_nulls(&mut value);
    value.serialize(serializer)
}
fn omit_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for v in map.values_mut() {
                omit_nulls(v);
            }
        }
        Value::Array(list) => {
            list.retain(|v| !v.is_null());
            for v in list {
                omit_nulls(v);
            }
        }
        _ => (),
    }
}
