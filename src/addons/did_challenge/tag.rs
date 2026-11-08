//! Python addons/did_challenge/tag.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub conditions: Vec<Condition>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub tag: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}

use crate::{
    addons::{
        AddonExecutor, DidChallenge,
        payload::{DidChallengePayload, DidChallengeReference},
    },
    compat::{Error, Result, template::TemplateEngine, values},
};

pub struct Executor {
    config: Config,
    engine: TemplateEngine,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        for condition in &config.conditions {
            if let Some(expression) = &condition.when {
                engine.load(&format!("{{{{{expression}}}}}"))?;
            }
            engine.load(&condition.tag)?;
        }
        Ok(Self { config, engine })
    }
}
impl AddonExecutor<DidChallenge> for Executor {
    fn exec(
        &self,
        payload: DidChallengePayload,
        reference: &DidChallengeReference,
    ) -> Result<DidChallengePayload> {
        self.exec_with_log(payload, reference, &|_| {})
    }
    fn exec_with_log(
        &self,
        mut payload: DidChallengePayload,
        reference: &DidChallengeReference,
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> Result<DidChallengePayload> {
        let trial = serde_json::to_value(&payload.trial)
            .map_err(|e| Error::new("SerializeError", e.to_string()))?;
        let context = minijinja::Value::from_pairs([
            ("trial", values::from_json(&trial)),
            ("res_one", reference.res_one.to_dict_value()),
            ("res_other", reference.res_other.to_dict_value()),
            (
                "res_one_props",
                values::from_json(
                    reference
                        .res_one_props
                        .as_ref()
                        .unwrap_or(&serde_json::Value::Null),
                ),
            ),
            (
                "res_other_props",
                values::from_json(
                    reference
                        .res_other_props
                        .as_ref()
                        .unwrap_or(&serde_json::Value::Null),
                ),
            ),
        ]);
        let checked = values::checked_context(context.clone())?;
        // 同じtag内の全条件・テンプレートは追加前のtrialを参照する。
        let tags_before = payload.trial.tags.len();
        for condition in &self.config.conditions {
            let matched = match condition.when.as_deref() {
                None | Some("") => true,
                Some(expression) => self.engine.when(expression, &checked)?.truthy(),
            };
            if matched {
                payload
                    .trial
                    .tags
                    .push(self.engine.render(&condition.tag, &context)?);
            }
        }
        if payload.trial.tags.len() == tags_before {
            log(crate::addons::log::AddonLog::NoMatchingTags);
        }
        Ok(payload)
    }
}
