//! Python addons/judgement/ignore.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub ignores: Vec<Ignore>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ignore {
    pub title: String,
    pub conditions: Vec<Condition>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub added: Vec<Case>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub removed: Vec<Case>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub changed: Vec<Case>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub path: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}

use crate::{
    addons::{
        AddonExecutor, Judgement,
        payload::{JudgementPayload, JudgementReference},
    },
    compat::{
        Error, Result,
        regex::{ExactRegex, Regex},
        template::TemplateEngine,
        values,
    },
    diff::{DiffKeys, get_by_diff_key},
};

struct CompiledCase {
    path: ExactRegex,
    when: Option<String>,
}
struct CompiledCondition {
    when: Option<String>,
    added: Vec<CompiledCase>,
    changed: Vec<CompiledCase>,
    removed: Vec<CompiledCase>,
}
struct CompiledIgnore {
    title: String,
    conditions: Vec<CompiledCondition>,
}
pub struct Executor {
    ignores: Vec<CompiledIgnore>,
    engine: TemplateEngine,
}

impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let engine = TemplateEngine::new();
        let compile_cases = |cases: Vec<Case>| -> Result<Vec<CompiledCase>> {
            cases
                .into_iter()
                .map(|case| {
                    super::load_when(&engine, case.when.as_deref())?;
                    Ok(CompiledCase {
                        path: Regex::new_exact(&case.path)?,
                        when: case.when,
                    })
                })
                .collect()
        };
        let ignores = config
            .ignores
            .into_iter()
            .map(|ignore| {
                let conditions = ignore
                    .conditions
                    .into_iter()
                    .map(|condition| {
                        super::load_when(&engine, condition.when.as_deref())?;
                        Ok(CompiledCondition {
                            when: condition.when,
                            added: compile_cases(condition.added)?,
                            changed: compile_cases(condition.changed)?,
                            removed: compile_cases(condition.removed)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(CompiledIgnore {
                    title: ignore.title,
                    conditions,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { ignores, engine })
    }

    fn matched(
        &self,
        paths: &[String],
        cases: &[CompiledCase],
        reference: &JudgementReference,
    ) -> Result<Vec<String>> {
        let mut matched = Vec::new();
        for path in paths {
            for case in cases {
                if !case.path.exact_match(path)? {
                    continue;
                }
                let value = |dict: &Option<serde_json::Value>| {
                    values::from_json_for_when(
                        dict.as_ref()
                            .and_then(|dict| get_by_diff_key(dict, path))
                            .unwrap_or(&serde_json::Value::Null),
                    )
                };
                let context = values::CheckedContext::from_pairs([
                    ("one", value(&reference.dict_one)?),
                    ("other", value(&reference.dict_other)?),
                ]);
                if super::when(&self.engine, case.when.as_deref(), &context)? {
                    matched.push(path.clone());
                    break;
                }
            }
        }
        Ok(matched)
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
        if payload.regard_as_same_body || payload.diffs_by_cognition.is_none() {
            return Ok(payload);
        }
        let diffs = payload.diffs_by_cognition.as_mut().unwrap();
        let context = super::context(reference)?;
        for ignore in &self.ignores {
            // 各conditionは同じunknownのスナップショットを見る。重複もPython同様残す。
            let mut matches = Vec::new();
            for condition in &ignore.conditions {
                if !super::when(&self.engine, condition.when.as_deref(), &context)? {
                    continue;
                }
                let unknown = diffs
                    .get("unknown")
                    .ok_or_else(|| Error::new("KeyError", "unknown"))?;
                matches.push(DiffKeys {
                    added: self.matched(&unknown.added, &condition.added, reference)?,
                    changed: self.matched(&unknown.changed, &condition.changed, reference)?,
                    removed: self.matched(&unknown.removed, &condition.removed, reference)?,
                });
            }
            for matched in matches {
                let mut merged = diffs.get(&ignore.title).cloned().unwrap_or_default();
                merged.added.extend(matched.added.iter().cloned());
                merged.changed.extend(matched.changed.iter().cloned());
                merged.removed.extend(matched.removed.iter().cloned());
                let unknown = diffs.get_mut("unknown").unwrap();
                unknown.added.retain(|path| !matched.added.contains(path));
                unknown
                    .changed
                    .retain(|path| !matched.changed.contains(path));
                unknown
                    .removed
                    .retain(|path| !matched.removed.contains(path));
                diffs.insert(ignore.title.clone(), merged);
            }
        }
        payload.regard_as_same_body = super::is_empty(
            diffs
                .get("unknown")
                .ok_or_else(|| Error::new("KeyError", "unknown"))?,
        );
        log(crate::addons::log::AddonLog::CognitionDiffs(diffs.clone()));
        diffs.retain(|_, diff| !super::is_empty(diff));
        Ok(payload)
    }
}
