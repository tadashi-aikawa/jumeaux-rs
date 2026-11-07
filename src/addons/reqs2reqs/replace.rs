//! Python addons/reqs2reqs/replace.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub items: Vec<Replacer>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacer {
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub queries: crate::models::Queries,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub headers: crate::models::Headers,
}
