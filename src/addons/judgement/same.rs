//! Python addons/judgement/same.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub when_any: Vec<String>,
}

use crate::{
    addons::{
        AddonExecutor, Judgement,
        payload::{JudgementPayload, JudgementReference},
    },
    compat::{Result, template::TemplateEngine},
};
pub struct Executor {
    config: Config,
    engine: TemplateEngine,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        for expression in &config.when_any {
            super::load_when(&engine, Some(expression))?;
        }
        Ok(Self { config, engine })
    }
}
impl AddonExecutor<Judgement> for Executor {
    fn exec(
        &self,
        payload: JudgementPayload,
        reference: &JudgementReference,
    ) -> Result<JudgementPayload> {
        self.exec_with_log(payload, reference, &|_| {})
    }
    fn exec_with_log(
        &self,
        mut payload: JudgementPayload,
        reference: &JudgementReference,
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> Result<JudgementPayload> {
        if payload.regard_as_same_body {
            return Ok(payload);
        }
        let context = super::context(reference)?;
        for expression in &self.config.when_any {
            if self.engine.when(expression, &context)?.truthy() {
                log(crate::addons::log::AddonLog::RegardAsSame {
                    expression: expression.clone(),
                });
                payload.regard_as_same_body = true;
                break;
            }
        }
        Ok(payload)
    }
}
