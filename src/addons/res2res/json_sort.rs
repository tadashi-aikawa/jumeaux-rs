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
