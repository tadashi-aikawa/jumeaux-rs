//! Python addons/reqs2reqs/head.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub size: i64,
}
impl Config {
    pub fn apply(&self, mut requests: Vec<crate::models::Request>) -> Vec<crate::models::Request> {
        let len = requests.len();
        let end = if self.size < 0 {
            len.saturating_sub(self.size.unsigned_abs() as usize)
        } else {
            usize::try_from(self.size).unwrap_or(usize::MAX).min(len)
        };
        requests.truncate(end);
        requests
    }
}
