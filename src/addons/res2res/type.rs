//! Python addons/res2res/type.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub conditions: Vec<Condition>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub r#type: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}

use crate::{
    addons::{AddonExecutor, Res2Res, payload::Res2ResPayload},
    compat::{Result, template::TemplateEngine},
};

pub struct Executor {
    config: Config,
    engine: TemplateEngine,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        for condition in &config.conditions {
            if let Some(when) = &condition.when {
                engine.load(&format!("{{{{{when}}}}}"))?;
            }
        }
        Ok(Self { config, engine })
    }
}
impl AddonExecutor<Res2Res> for Executor {
    fn exec(&self, mut payload: Res2ResPayload, _: &()) -> Result<Res2ResPayload> {
        let context = minijinja::Value::from_pairs([
            ("req", payload.req.to_dict_value()),
            ("res", payload.response.to_dict_value()),
        ]);
        for condition in &self.config.conditions {
            if match &condition.when {
                None => true,
                Some(when) => self.engine.when(when, &context)?.truthy(),
            } {
                payload.response.response_type.clone_from(&condition.r#type);
                break;
            }
        }
        Ok(payload)
    }
}
