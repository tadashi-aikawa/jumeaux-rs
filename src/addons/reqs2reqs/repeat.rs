//! Python addons/reqs2reqs/repeat.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub times: i64,
}
impl Config {
    pub fn apply(
        &self,
        requests: Vec<crate::models::Request>,
    ) -> crate::compat::Result<Vec<crate::models::Request>> {
        if requests.is_empty() || self.times <= 0 {
            return Ok(Vec::new());
        }
        let times = usize::try_from(self.times)
            .map_err(|e| crate::compat::Error::new("OverflowError", e.to_string()))?;
        let len = requests
            .len()
            .checked_mul(times)
            .ok_or_else(|| crate::compat::Error::new("OverflowError", "repeat size overflow"))?;
        let mut result = Vec::new();
        result
            .try_reserve(len)
            .map_err(|e| crate::compat::Error::new("MemoryError", e.to_string()))?;
        for _ in 0..times {
            result.extend(requests.iter().cloned());
        }
        Ok(result)
    }
}
