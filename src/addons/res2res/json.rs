//! Python addons/res2res/json.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub transformer: Transformer,
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub default_encoding: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transformer {
    pub module: String,
    #[serde(default = "function", deserialize_with = "null_function")]
    pub function: String,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());
fn function() -> String {
    "transform".into()
}
crate::models::defaults::null_default_with!(null_function, String, function());
