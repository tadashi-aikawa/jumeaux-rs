pub mod csv;
pub mod json;
pub mod miroir;
pub mod notify;
pub mod summary;
pub mod viewer;

use super::{
    AddonConfig, Layer, parse_config,
    payload::{FinalPayload, FinalReference},
};
use crate::{
    compat::{Error, Result, template::TemplateEngine},
    models::Addon,
    notification::{Context, Failure, LoggedResult, Outcome, Verbosity},
};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Log {
    pub level: Verbosity,
    pub message: String,
}
impl Log {
    pub fn new(level: Verbosity, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }
}

pub struct Execution {
    pub payload: FinalPayload,
    pub logs: Vec<Log>,
    pub notifications: Vec<Outcome>,
    pub exit_code: Option<u8>,
}

/// 通知をawaitするfinal専用チェーン。設定順を保ち、エラー後は実行しない。
pub struct Pipeline {
    configs: Vec<AddonConfig>,
    engine: TemplateEngine,
    runtime: std::sync::Arc<super::exec::Runtime>,
}
impl Pipeline {
    pub fn build(addons: &[Addon]) -> Result<Self> {
        let engine = TemplateEngine::new();
        let configs = addons
            .iter()
            .enumerate()
            .map(|(i, addon)| {
                let config = parse_config(Layer::Final, addon, &format!("addons.final[{i}]"))?;
                if let AddonConfig::FinalNotify(c) = &config {
                    for item in &c.notifies {
                        engine.load(&item.message)?;
                        if let Some(when) = &item.when {
                            engine.load(&format!("{{{{{when}}}}}"))?;
                        }
                    }
                }
                Ok(config)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            configs,
            engine,
            runtime: super::exec::Runtime::prepare_layer(Layer::Final, addons)?,
        })
    }
    pub fn with_runtime(mut self, runtime: std::sync::Arc<super::exec::Runtime>) -> Self {
        self.runtime = runtime;
        self
    }

    /// stdoutの書き込み先は呼出元で指定する。ログは戻り値だけに保持する。
    pub async fn exec(
        &self,
        payload: FinalPayload,
        reference: &FinalReference,
        context: &Context,
        mut stdout: impl FnMut(&str) -> Result<()>,
    ) -> LoggedResult<Execution, Log> {
        let mut result = Execution {
            payload,
            logs: vec![],
            notifications: vec![],
            exit_code: None,
        };
        for (index, config) in self.configs.iter().enumerate() {
            self.runtime.check_cancelled()?;
            if matches!(config, AddonConfig::Exec(_)) {
                result.payload = self.runtime.payload::<super::Final>(
                    index,
                    result.payload,
                    reference,
                    Default::default(),
                )?;
                continue;
            }
            if let Err(error) = self
                .apply(config, &mut result, reference, context, &mut stdout)
                .await
            {
                return Err(Failure {
                    error,
                    logs: result.logs,
                });
            }
            if result.exit_code.is_some() {
                break;
            }
        }
        Ok(result)
    }

    async fn apply(
        &self,
        config: &AddonConfig,
        result: &mut Execution,
        reference: &FinalReference,
        context: &Context,
        stdout: &mut impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        let payload = &result.payload;
        let root = Path::new(&payload.output_summary.response_dir).join(&payload.report.key);
        match config {
            AddonConfig::FinalSummary(c) => {
                let text = summary::render(&payload.report);
                if c.sysout {
                    stdout(&format!("{text}\n"))?;
                } else {
                    write_text(&root.join("summary.txt"), &text, &payload.output_summary)?;
                }
            }
            AddonConfig::FinalJson(c) => {
                let text = c.render(&payload.report)?;
                if c.sysout {
                    stdout(&format!("{text}\n"))?;
                } else {
                    write_text(&root.join("report.json"), &text, &payload.output_summary)?;
                }
            }
            AddonConfig::FinalCsv(c) => {
                // owlmixin.to_csvfの既定encodingはUTF-8。output.encodingは使わない。
                std::fs::write(&c.output_path, c.render(&payload.report)?)
                    .map_err(|e| io_error(Path::new(&c.output_path), e))?;
            }
            AddonConfig::FinalViewer(_) => viewer::save(&root, &mut result.logs)?,
            AddonConfig::FinalMiroir(c) => c.send(payload, &mut result.logs).await?,
            AddonConfig::FinalNotify(c) => {
                c.send(
                    &payload.report,
                    reference,
                    context,
                    &self.engine,
                    &mut result.logs,
                    &mut result.notifications,
                    &mut result.exit_code,
                )
                .await?
            }
            _ => return Err(Error::new("ConfigError", "unsupported final configuration")),
        }
        Ok(())
    }
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::new("IOError", format!("{}: {error}", path.display()))
}
fn write_text(path: &Path, text: &str, output: &crate::models::OutputSummary) -> Result<()> {
    let file = std::fs::File::create(path).map_err(|e| io_error(path, e))?;
    output
        .write_summary(file, text)
        .map_err(|e| io_error(path, e))
}
