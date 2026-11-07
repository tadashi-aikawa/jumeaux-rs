//! Python addons/did_challenge/sleep.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub max: f64,
    pub min: f64,
}
