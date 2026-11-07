//! Python addons/log2reqs/json.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub encoding: String,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());
