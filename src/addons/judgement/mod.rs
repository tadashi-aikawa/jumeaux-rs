pub mod ignore;
pub mod same;

use crate::{
    addons::{
        AddonConfig, AddonExecutor, Judgement, parse_config,
        payload::{JudgementPayload, JudgementReference},
    },
    compat::{Result, template::TemplateEngine, values},
    diff::{self, DiffKeys},
    models::{Addon, CognitionDiffs, Headers},
};

pub struct Factory;
impl crate::addons::Factory<Judgement> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<Judgement>>> {
        Ok(
            match parse_config(crate::addons::Layer::Judgement, addon, "addons.judgement")? {
                AddonConfig::JudgementIgnore(config) => Box::new(ignore::Executor::new(config)?),
                AddonConfig::JudgementSame(config) => Box::new(same::Executor::new(config)?),
                _ => unreachable!("judgement registration"),
            },
        )
    }
}

fn is_empty(diffs: &DiffKeys) -> bool {
    diffs.added.is_empty() && diffs.changed.is_empty() && diffs.removed.is_empty()
}

/// executor.judgement の入力。変換結果なしと、差分なしの空辞書を区別する。
pub fn initial_payload(
    reference: &JudgementReference,
    judge_response_header: bool,
    ignore_response_header_keys: &[String],
) -> JudgementPayload {
    let same_bytes = reference.res_one.body == reference.res_other.body;
    let diffs = match (&reference.dict_one, &reference.dict_other) {
        (Some(one), Some(other)) => Some(if same_bytes {
            DiffKeys::default()
        } else {
            diff::diff(one, other)
        }),
        _ => None,
    };
    let regard_as_same_body = diffs.as_ref().map_or(same_bytes, is_empty);
    let diffs_by_cognition = diffs.map(|diffs| {
        let mut result = CognitionDiffs::new();
        if !is_empty(&diffs) {
            result.insert("unknown".into(), diffs);
        }
        result
    });
    JudgementPayload {
        diffs_by_cognition,
        regard_as_same_body,
        regard_as_same_header: !judge_response_header
            || headers_equal(
                &reference.res_one.headers,
                &reference.res_other.headers,
                ignore_response_header_keys,
            ),
    }
}

/// 名前・除外キーはASCII大小無視、値は完全一致。
pub fn headers_equal(one: &Headers, other: &Headers, ignore: &[String]) -> bool {
    let normalized = |headers: &Headers| -> std::collections::BTreeMap<String, String> {
        headers
            .iter()
            .filter(|(key, _)| {
                !ignore
                    .iter()
                    .any(|excluded| key.eq_ignore_ascii_case(excluded))
            })
            .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
            .collect()
    };
    normalized(one) == normalized(other)
}

fn context(reference: &JudgementReference) -> minijinja::Value {
    minijinja::Value::from_pairs([
        (
            "req",
            minijinja::Value::from_pairs([
                ("name", values::serialized(&reference.name)),
                ("path", values::serialized(&reference.path)),
                ("qs", values::serialized(&reference.qs)),
                ("headers", values::serialized(&reference.headers)),
            ]),
        ),
        ("res_one", reference.res_one.to_template_value()),
        ("res_other", reference.res_other.to_template_value()),
        ("dict_one", values::optional(reference.dict_one.as_ref())),
        (
            "dict_other",
            values::optional(reference.dict_other.as_ref()),
        ),
    ])
}

fn when(
    engine: &TemplateEngine,
    expression: Option<&str>,
    context: &minijinja::Value,
) -> Result<bool> {
    expression.map_or(Ok(true), |expression| {
        Ok(engine.when(expression, context)?.truthy())
    })
}

fn load_when(engine: &TemplateEngine, expression: Option<&str>) -> Result<()> {
    if let Some(expression) = expression {
        engine.load(&format!("{{{{{expression}}}}}"))?;
    }
    Ok(())
}
