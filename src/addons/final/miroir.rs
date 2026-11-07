//! Python addons/final/miroir.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub table: String,
    pub bucket: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix: Option<String>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub cache_max_age: i64,
    #[serde(default = "with_zip", deserialize_with = "null_with_zip")]
    pub with_zip: bool,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub assumed_role_arn: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub checklist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_stack: Option<LocalStack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Endpoints>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub when: Vec<When>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalStack {
    pub r#use: bool,
    #[serde(default = "endpoint", deserialize_with = "null_endpoint")]
    pub endpoint: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamodb: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    NotEmpty,
    HasDifferent,
}
fn with_zip() -> bool {
    true
}
crate::models::defaults::null_default_with!(null_with_zip, bool, with_zip());
fn endpoint() -> String {
    "http://localhost".into()
}
crate::models::defaults::null_default_with!(null_endpoint, String, endpoint());
