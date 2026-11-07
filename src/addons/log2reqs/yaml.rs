//! Python addons/log2reqs/yaml.py の設定契約。
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

impl Config {
    pub fn read(&self, body: &[u8]) -> crate::compat::Result<Vec<crate::models::Request>> {
        let text = crate::compat::encoding::decode_input(body, &self.encoding)?;
        let mut value = serde_yaml_ng::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
            .map_err(|e| crate::compat::Error::new("ParserError", e.to_string()))?;
        crate::config::expand_yaml_merges(&mut value, "requests")?;
        super::requests(value)
    }
}
