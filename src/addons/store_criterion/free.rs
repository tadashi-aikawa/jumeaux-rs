//! Python addons/store_criterion/free.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_any: Option<Vec<String>>,
}

use crate::{
    addons::{
        AddonExecutor, StoreCriterion,
        payload::{StoreCriterionPayload, StoreCriterionReference},
    },
    compat::{Result, template::TemplateEngine, values},
};

pub struct Executor {
    config: Config,
    engine: TemplateEngine,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        if let Some(expressions) = &config.when_any {
            for expression in expressions {
                engine.load(&format!("{{{{{expression}}}}}"))?;
            }
        }
        Ok(Self { config, engine })
    }
}
impl AddonExecutor<StoreCriterion> for Executor {
    fn exec(
        &self,
        payload: StoreCriterionPayload,
        reference: &StoreCriterionReference,
    ) -> Result<StoreCriterionPayload> {
        self.exec_with_log(payload, reference, &|_| {})
    }
    fn exec_with_log(
        &self,
        mut payload: StoreCriterionPayload,
        reference: &StoreCriterionReference,
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> Result<StoreCriterionPayload> {
        if payload.stored {
            return Ok(payload);
        }
        let Some(expressions) = &self.config.when_any else {
            payload.stored = true;
            return Ok(payload);
        };
        // challengeで渡されないRequestフィールドにはPythonモデルの既定値が入る。
        let req = &reference.req;
        let context = values::checked_context(minijinja::Value::from_pairs([
            ("status", values::serialized(reference.status)),
            (
                "req",
                minijinja::Value::from_pairs([
                    ("name", values::serialized(&req.name)),
                    ("path", values::serialized(&req.path)),
                    ("qs", values::serialized(&req.qs)),
                    ("headers", values::serialized(&req.headers)),
                    ("method", values::serialized(req.method)),
                    ("url_encoding", values::serialized(&req.url_encoding)),
                    ("raw", values::serialized(&req.raw)),
                    ("form", values::serialized(&req.form)),
                    ("json", values::serialized(&req.json)),
                ]),
            ),
            ("res_one", response_dict(&reference.res_one)),
            ("res_other", response_dict(&reference.res_other)),
        ]))?;
        for expression in expressions {
            if self.engine.when(expression, &context)?.truthy() {
                log(crate::addons::log::AddonLog::Stored {
                    expression: expression.clone(),
                });
                payload.stored = true;
                break;
            }
        }
        Ok(payload)
    }
}

fn response_dict(response: &crate::models::Response) -> minijinja::Value {
    let base = response.to_dict_value();
    let mut fields: indexmap::IndexMap<String, minijinja::Value> = base
        .try_iter()
        .unwrap()
        .map(|key| (key.to_string(), base.get_item(&key).unwrap()))
        .collect();
    fields.insert("encoding".into(), values::serialized(&response.encoding));
    minijinja::Value::from_pairs(fields)
}
