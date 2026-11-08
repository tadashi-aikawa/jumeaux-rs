//! Python addons/final/notify.py の設定契約。
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
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}

impl Config {
    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        report: &crate::models::Report,
        reference: &crate::addons::payload::FinalReference,
        context: &crate::notification::Context,
        engine: &crate::compat::template::TemplateEngine,
        logs: &mut impl super::LogSink,
        outcomes: &mut Vec<crate::notification::Outcome>,
        exit_code: &mut Option<u8>,
    ) -> crate::compat::Result<()> {
        use crate::{
            compat::{Error, values},
            notification::{self, Level, Verbosity},
        };
        let Some(notifiers) = &reference.notifiers else {
            logs.push(super::Log::new(
                Verbosity::Error,
                "[final/notify] There are no notifiers. Please set notifiers in config.",
            ));
            logs.push(super::Log::new(Verbosity::Error, "[final/notify] See https://tadashi-aikawa.github.io/jumeaux/ja/getstarted/configuration/"));
            *exit_code = Some(1);
            return Ok(());
        };
        let data = crate::report::template_value(report)?;
        let checked = values::from_json_for_when(&data)?;
        // Pythonは先に全whenを評価し、そのあとで選んだ通知を送る。
        let mut selected = Vec::new();
        for item in &self.notifies {
            let send = item
                .when
                .as_ref()
                .map(|w| engine.when(w, &checked).map(|v| v.truthy()))
                .transpose()?
                .unwrap_or(true);
            let when = item.when.as_deref().unwrap_or("None");
            let message = if send {
                format!(
                    "[final/notify] Notify by {}. (notify.when => {when})",
                    item.notifier
                )
            } else {
                format!(
                    "[final/notify] Don't Notify by {}. (notify.when => {when}",
                    item.notifier
                )
            };
            logs.push(super::Log::new(Verbosity::VeryVerbose, message));
            if send {
                selected.push(item);
            }
        }
        let start = outcomes.len();
        for item in selected {
            let notifier = notifiers.get(&item.notifier).ok_or_else(|| {
                Error::new(
                    "ValueError",
                    format!("notifier not found: {}", item.notifier),
                )
            })?;
            let message = engine.render(&item.message, &data)?;
            logs.push(super::Log::new(
                Verbosity::Normal,
                format!("Notify using slack@v{}", notifier.version),
            ));
            let outcome = notification::notify(notifier, &message, context).await?;
            for l in &outcome.logs {
                logs.push(super::Log::new(
                    match l.level {
                        Level::Info => Verbosity::Normal,
                        Level::Warning => Verbosity::Warning,
                        Level::Error => Verbosity::Error,
                    },
                    l.message.clone(),
                ));
            }
            outcomes.push(outcome);
        }
        for outcome in &outcomes[start..] {
            if let Some(error) = &outcome.error {
                logs.push(super::Log::new(Verbosity::Error, error));
                *exit_code = Some(1);
            }
        }
        Ok(())
    }
}
