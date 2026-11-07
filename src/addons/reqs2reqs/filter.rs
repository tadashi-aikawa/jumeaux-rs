//! Python addons/reqs2reqs/filter.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub when: String,
}
impl Config {
    pub fn apply(
        &self,
        requests: Vec<crate::models::Request>,
    ) -> crate::compat::Result<Vec<crate::models::Request>> {
        requests
            .into_iter()
            .filter_map(|req| match super::matches(Some(&self.when), &req) {
                Ok(true) => Some(Ok(req)),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
}
