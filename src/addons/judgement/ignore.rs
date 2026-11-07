//! Python addons/judgement/ignore.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub ignores: Vec<Ignore>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ignore {
    pub title: String,
    pub conditions: Vec<Condition>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub added: Vec<Case>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub removed: Vec<Case>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub changed: Vec<Case>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub path: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}
