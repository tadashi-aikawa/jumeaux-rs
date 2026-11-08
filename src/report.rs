//! challengeの実行結果を、保存・retry・finalに渡すReportへ集計する。
use crate::{config::Config, executor::Execution, models::*};

pub fn create_report(
    config: &Config,
    execution: Execution,
    key: String,
    retry_hash: Option<String>,
) -> Report {
    let mut status = StatusCounts::default();
    for trial in &execution.trials {
        match trial.status {
            Status::Same => status.same += 1,
            Status::Different => status.different += 1,
            Status::Failure => status.failure += 1,
        }
    }
    // Python timedelta.secondsは日数と小数秒を含まない。
    let elapsed_sec = (execution.ended_at - execution.started_at)
        .num_seconds()
        .rem_euclid(86_400) as u64;
    Report {
        version: env!("CARGO_PKG_VERSION").into(),
        key,
        title: config.title.clone().unwrap_or_else(|| "No title".into()),
        description: config.description.clone(),
        notifiers: config.notifiers.clone(),
        summary: Summary {
            one: config.one.clone(),
            other: config.other.clone(),
            status,
            tags: config.tags.clone().unwrap_or_default(),
            time: Time {
                start: timestamp(execution.started_at),
                end: timestamp(execution.ended_at),
                elapsed_sec,
            },
            concurrency: execution.concurrency,
            output: config.output.clone(),
            comparison: Some(execution.comparison),
            default_encoding: None,
        },
        trials: execution.trials,
        addons: Some(config.addons.clone()),
        retry_hash,
    }
}

fn timestamp(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    if at.timestamp_subsec_micros() == 0 {
        at.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        at.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
    }
}

/// final/notifyはPythonのto_dict(ignore_none=False)を条件とメッセージへ渡す。
pub fn template_value(report: &Report) -> crate::compat::Result<serde_json::Value> {
    use serde_json::{Value, json};
    fn fill(value: &mut Value, keys: &[&str]) {
        for key in keys {
            value
                .as_object_mut()
                .expect("model object")
                .entry(*key)
                .or_insert(Value::Null);
        }
    }
    let mut value = serde_json::to_value(report)
        .map_err(|e| crate::compat::Error::new("ValueError", e.to_string()))?;
    fill(
        &mut value,
        &["description", "notifiers", "addons", "retry_hash"],
    );
    fill(&mut value["summary"], &["default_encoding"]);
    fill(&mut value["summary"]["output"], &["logger"]);
    for side in ["one", "other"] {
        fill(
            &mut value["summary"][side],
            &["path", "query", "proxy", "default_response_encoding"],
        );
        if value["summary"][side]["query"].is_object() {
            fill(
                &mut value["summary"][side]["query"],
                &["overwrite", "remove"],
            );
        }
    }
    if let Some(notifiers) = value["notifiers"].as_object_mut() {
        for notifier in notifiers.values_mut() {
            fill(notifier, &["channel", "icon_emoji", "icon_url"]);
        }
    }
    if let Some(addons) = &report.addons {
        for layer in crate::addons::Layer::ALL {
            let items: Vec<_> = layer.configured(addons).iter().map(|a|
                json!({"name":a.name,"cls_name":a.cls_name,"config":a.config,"include":a.include,"tags":a.tags})
            ).collect();
            value["addons"][layer.name()] = if layer == crate::addons::Layer::Log2Reqs {
                items[0].clone()
            } else {
                json!(items)
            };
        }
    }
    for (trial, model) in value["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&report.trials)
    {
        fill(trial, &["raw", "form", "json", "diffs_by_cognition"]);
        trial["form"] = json!(model.form);
        trial["json"] = json!(model.json);
        for (side, response) in [("one", &model.one), ("other", &model.other)] {
            fill(
                &mut trial[side],
                &[
                    "status_code",
                    "byte",
                    "response_sec",
                    "content_type",
                    "mime_type",
                    "encoding",
                    "file",
                    "prop_file",
                    "response_header",
                ],
            );
            trial[side]["response_header"] = json!(response.response_header);
        }
    }
    Ok(value)
}
