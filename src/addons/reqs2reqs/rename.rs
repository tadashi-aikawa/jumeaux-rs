//! Python addons/reqs2reqs/rename.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub conditions: Vec<Condition>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}
impl Config {
    pub fn apply(
        &self,
        requests: Vec<crate::models::Request>,
    ) -> crate::compat::Result<Vec<crate::models::Request>> {
        requests
            .into_iter()
            .map(|mut request| {
                for condition in &self.conditions {
                    if super::matches(condition.when.as_deref(), &request)? {
                        let name =
                            super::engine().render(&condition.name, request.to_dict_value())?;
                        request.name = (!name.is_empty()).then_some(name);
                        break;
                    }
                }
                Ok(request)
            })
            .collect()
    }
}
