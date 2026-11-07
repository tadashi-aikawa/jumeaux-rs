//! Python addons/reqs2reqs/empty_guard.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub notifies: Vec<Notify>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    pub notifier: String,
    pub message: String,
}
impl Config {
    pub async fn notify(
        &self,
        config: &crate::config::Config,
        context: &crate::notification::Context,
        logs: &mut Vec<crate::notification::Log>,
    ) -> crate::compat::Result<Vec<crate::notification::Outcome>> {
        let data = config_value(config)?;
        let mut outcomes = Vec::new();
        for item in &self.notifies {
            let notifier = config
                .notifiers
                .as_ref()
                .and_then(|ns| ns.get(&item.notifier))
                .ok_or_else(|| {
                    crate::compat::Error::new(
                        "ValueError",
                        format!("notifier not found: {}", item.notifier),
                    )
                })?;
            let message = super::engine().render(&item.message, &data)?;
            logs.push(crate::notification::Log::new(
                crate::notification::Level::Info,
                format!("Notify using slack@v{}", notifier.version),
            ));
            let outcome = crate::notification::notify(notifier, &message, context).await?;
            logs.extend(outcome.logs.iter().cloned());
            outcomes.push(outcome);
        }
        for outcome in &outcomes {
            if let Some(error) = &outcome.error {
                logs.push(crate::notification::Log::new(
                    crate::notification::Level::Error,
                    error,
                ));
            }
        }
        Ok(outcomes)
    }
}

/// 通知のテンプレートだけはto_dict(ignore_none=False)を使う。
fn config_value(config: &crate::config::Config) -> crate::compat::Result<serde_json::Value> {
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
    let mut data = serde_json::to_value(config)
        .map_err(|e| crate::compat::Error::new("ValueError", e.to_string()))?;
    fill(
        &mut data,
        &[
            "processes",
            "title",
            "description",
            "tags",
            "input_files",
            "notifiers",
        ],
    );
    for key in ["one", "other"] {
        fill(
            &mut data[key],
            &["path", "query", "proxy", "default_response_encoding"],
        );
        if data[key]["query"].is_object() {
            fill(&mut data[key]["query"], &["overwrite", "remove"]);
        }
    }
    fill(&mut data["output"], &["logger"]);
    if let Some(notifiers) = data["notifiers"].as_object_mut() {
        for notifier in notifiers.values_mut() {
            fill(notifier, &["channel", "icon_emoji", "icon_url"]);
        }
    }
    for layer in crate::addons::Layer::ALL {
        let values: Vec<_> = layer.configured(&config.addons).iter().map(|addon| {
            json!({"name":addon.name,"cls_name":addon.cls_name,"config":addon.config,"include":addon.include,"tags":addon.tags})
        }).collect();
        data["addons"][layer.name()] = if layer == crate::addons::Layer::Log2Reqs {
            values[0].clone()
        } else {
            json!(values)
        };
    }
    Ok(data)
}
