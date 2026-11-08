//! Python addons/final/json.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub sysout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent: Option<i64>,
}

impl Config {
    pub fn render(&self, report: &crate::models::Report) -> crate::compat::Result<String> {
        let value = serde_json::to_value(report)
            .map_err(|e| crate::compat::Error::new("SerializeError", e.to_string()))?;
        crate::output::json_text(&value, self.indent)
    }
}
