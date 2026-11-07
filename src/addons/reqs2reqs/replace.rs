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
impl Config {
    pub fn apply(
        &self,
        requests: Vec<crate::models::Request>,
    ) -> crate::compat::Result<Vec<crate::models::Request>> {
        self.apply_with_datetime(requests, crate::compat::template::parse_datetime)
    }
    pub fn apply_with_datetime(
        &self,
        requests: Vec<crate::models::Request>,
        datetime: impl Fn(&str) -> crate::compat::Result<String>,
    ) -> crate::compat::Result<Vec<crate::models::Request>> {
        requests
            .into_iter()
            .map(|mut req| {
                for item in &self.items {
                    // Pythonは同一Requestを変更するため、後続条件は置換済みの値を見る。
                    if super::matches(item.when.as_deref(), &req)? {
                        for (key, values) in &item.queries {
                            req.qs.insert(
                                key.clone(),
                                values
                                    .iter()
                                    .map(|v| datetime(v))
                                    .collect::<crate::compat::Result<_>>()?,
                            );
                        }
                        req.headers.extend(item.headers.clone());
                    }
                }
                Ok(req)
            })
            .collect()
    }
}
