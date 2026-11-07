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

impl Config {
    pub fn read(&self, body: &[u8]) -> crate::compat::Result<Vec<crate::models::Request>> {
        let text = crate::compat::encoding::decode_input(body, &self.encoding)?;
        let mut value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| crate::compat::Error::new("JSONDecodeError", e.to_string()))?;
        if let Some(items) = value.as_array_mut() {
            for item in items {
                if let Some(object) = item.as_object_mut() {
                    object.retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "name"
                                | "method"
                                | "path"
                                | "qs"
                                | "raw"
                                | "form"
                                | "json"
                                | "headers"
                                | "url_encoding"
                        )
                    });
                }
            }
        }
        super::requests(value)
    }
}
