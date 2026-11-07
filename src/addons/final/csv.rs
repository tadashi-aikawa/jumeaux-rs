//! Python addons/final/csv.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub column_names: Vec<String>,
    pub output_path: String,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub with_header: bool,
}
