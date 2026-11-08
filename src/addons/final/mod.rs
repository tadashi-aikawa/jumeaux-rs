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

/// 保存用Vecと逐次通知を同じアドオン実装で扱う。ライブラリはstderrへ書かない。
pub trait LogSink {
    fn push(&mut self, log: Log);
}
impl LogSink for Vec<Log> {
    fn push(&mut self, log: Log) {
        Vec::push(self, log);
    }
}
struct Observer<F>(F);
impl<F: FnMut(&Log)> LogSink for Observer<F> {
    fn push(&mut self, log: Log) {
        (self.0)(&log);
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
        Ok(Self { configs, engine })
    }

    /// stdoutの書き込み先は呼出元で指定する。ログは戻り値に保持する。
    pub async fn exec(
        &self,
        payload: FinalPayload,
        reference: &FinalReference,
        context: &Context,
        stdout: impl FnMut(&str) -> Result<()>,
    ) -> LoggedResult<Execution, Log> {
        let mut logs = Vec::new();
        match self
            .exec_to(payload, reference, context, stdout, &mut logs)
            .await
        {
            Ok(mut result) => {
                result.logs = logs;
                Ok(result)
            }
            Err(error) => Err(Failure { error, logs }),
        }
    }

    /// 各アドオンのログを発生時に通知する。成功・失敗の戻り値へは重複保持しない。
    pub async fn exec_with_log(
        &self,
        payload: FinalPayload,
        reference: &FinalReference,
        context: &Context,
        stdout: impl FnMut(&str) -> Result<()>,
        observer: impl FnMut(&Log),
    ) -> LoggedResult<Execution, Log> {
        self.exec_to(payload, reference, context, stdout, &mut Observer(observer))
            .await
            .map_err(Failure::from)
    }

    async fn exec_to(
        &self,
        payload: FinalPayload,
        reference: &FinalReference,
        context: &Context,
        mut stdout: impl FnMut(&str) -> Result<()>,
        logs: &mut impl LogSink,
    ) -> Result<Execution> {
        let mut result = Execution {
            payload,
            logs: vec![],
            notifications: vec![],
            exit_code: None,
        };
        for config in &self.configs {
            self.apply(config, &mut result, reference, context, &mut stdout, logs)
                .await?;
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
        logs: &mut impl LogSink,
    ) -> Result<()> {
        let payload = &result.payload;
        let root = std::path::PathBuf::from(format!(
            "{}/{}",
            payload.output_summary.response_dir, payload.report.key
        ));
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
            AddonConfig::FinalViewer(_) => viewer::save(&root, logs)?,
            AddonConfig::FinalMiroir(c) => c.send(payload, logs).await?,
            AddonConfig::FinalNotify(c) => {
                c.send(
                    &payload.report,
                    reference,
                    context,
                    &self.engine,
                    logs,
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
