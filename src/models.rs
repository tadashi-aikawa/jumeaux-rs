//! 設定・レポート・アドオン間で共有するモデル。
//! OwlMixinのTOptionはNoneを省略する。読込時の空文字はNone、空配列・空辞書は残す。
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::Arc;

pub(crate) mod defaults;
mod report_fields;
pub use crate::diff::DiffKeys;
use defaults::{null_default, null_default_with};
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
    #[serde(default, deserialize_with = "null_default")]
    pub method: HttpMethod,
    pub path: String,
    #[serde(default, deserialize_with = "null_default")]
    pub qs: Queries,
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
    #[serde(default, deserialize_with = "null_default")]
    pub headers: Headers,
    #[serde(default = "url_encoding", deserialize_with = "null_url_encoding")]
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

null_default_with!(null_url_encoding, String, url_encoding());
null_default_with!(null_utf8, String, utf8());
null_default_with!(null_one, i64, one());
null_default_with!(null_executor_class, String, executor_class());
null_default_with!(null_notifier_username, String, notifier_username());

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
    #[serde(default, deserialize_with = "null_default")]
    pub headers: Headers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSummary {
    pub response_dir: String,
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
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
    #[serde(default = "one", deserialize_with = "null_one")]
    pub version: i64,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub channel: Option<String>,
    #[serde(
        default = "notifier_username",
        deserialize_with = "null_notifier_username"
    )]
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
    #[serde(default, deserialize_with = "null_default")]
    pub use_blocks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Addon {
    pub name: String,
    #[serde(default = "executor_class", deserialize_with = "null_executor_class")]
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
    #[serde(default, deserialize_with = "null_default")]
    pub reqs2reqs: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub res2res: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub res2dict: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub judgement: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub store_criterion: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub dump: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default")]
    pub did_challenge: Vec<Addon>,
    #[serde(default, deserialize_with = "null_default", rename = "final")]
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
    #[serde(deserialize_with = "null_default")]
    pub same: u64,
    #[serde(deserialize_with = "null_default")]
    pub different: u64,
    #[serde(deserialize_with = "null_default")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<crate::config::HeaderJudgementSettings>,
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
            form: self.form.clone(),
            json: self.json.clone(),
            ..Request::default()
        }
    }
}

// 同じフィールド定義から公開モデルと厳格な読込用モデルを作る。
macro_rules! report_model {
    ($($(#[$attr:meta])* pub $field:ident: $ty:ty,)+) => {
        #[derive(Debug, Clone, Serialize)]
        pub struct Report { $($(#[$attr])* pub $field: $ty,)+ }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ReportFields { $($(#[$attr])* $field: $ty,)+ }
        impl From<ReportFields> for Report {
            fn from(fields: ReportFields) -> Self { Self { $($field: fields.$field,)+ } }
        }
    };
}
report_model! {
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

impl Report {
    /// report由来の未知キーは捨て、呼出元が検証できるパス付き警告を返す。
    pub fn from_value_with_warnings(
        mut value: Value,
    ) -> crate::compat::Result<(Self, Vec<String>)> {
        let warnings = report_fields::prune(&mut value);
        let fields: ReportFields = crate::config::deserialize(&value, "report")?;
        Ok((fields.into(), warnings))
    }
}

impl<'de> Deserialize<'de> for Report {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // serdeの入口には副作用を持たせない。警告が必要なCLIは明示APIを使う。
        let value = Value::deserialize(deserializer)?;
        let (report, _) =
            Self::from_value_with_warnings(value).map_err(serde::de::Error::custom)?;
        Ok(report)
    }
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
