pub mod add;
pub mod empty_guard;
pub mod filter;
pub mod head;
pub mod rename;
pub mod repeat;
pub mod replace;
pub mod shuffle;

use super::{AddonConfig, Layer, parse_config};
use crate::{
    compat::{Error, Result, template::TemplateEngine},
    config::Config,
    models::{Addon, Request},
    notification::{Context, Failure, Level, Log, LoggedResult, Outcome},
};
use std::sync::LazyLock;

fn engine() -> &'static TemplateEngine {
    static ENGINE: LazyLock<TemplateEngine> = LazyLock::new(TemplateEngine::new);
    &ENGINE
}
fn matches(when: Option<&str>, req: &Request) -> Result<bool> {
    when.map(|when| {
        let context = crate::compat::values::checked_context(req.to_dict_value())?;
        engine().when(when, &context).map(|v| v.truthy())
    })
    .unwrap_or(Ok(true))
}
fn load_when(when: &str) -> Result<()> {
    engine().load(&format!("{{{{{when}}}}}"))
}

/// 終了判断とログの出力は呼出側が担う。empty_guardは後続アドオンを実行しない。
#[derive(Debug)]
pub struct Execution {
    pub requests: Vec<Request>,
    pub notifications: Vec<Outcome>,
    pub logs: Vec<Log>,
    pub exit_code: Option<u8>,
}
pub struct Pipeline {
    configs: Vec<AddonConfig>,
    runtime: std::sync::Arc<super::exec::Runtime>,
}
impl Pipeline {
    pub fn build(addons: &[Addon]) -> Result<Self> {
        let configs = addons
            .iter()
            .enumerate()
            .map(|(i, addon)| {
                let c = parse_config(Layer::Reqs2Reqs, addon, &format!("addons.reqs2reqs[{i}]"))?;
                match &c {
                    AddonConfig::Reqs2ReqsFilter(c) => load_when(&c.when)?,
                    AddonConfig::Reqs2ReqsReplace(c) => {
                        for item in &c.items {
                            if let Some(w) = &item.when {
                                load_when(w)?;
                            }
                        }
                    }
                    AddonConfig::Reqs2ReqsRename(c) => {
                        for item in &c.conditions {
                            if let Some(w) = &item.when {
                                load_when(w)?;
                            }
                        }
                    }
                    AddonConfig::Reqs2ReqsEmptyGuard(c) => {
                        for item in &c.notifies {
                            engine().load(&item.message)?;
                        }
                    }
                    _ => (),
                }
                Ok(c)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            configs,
            runtime: super::exec::Runtime::prepare_layer(Layer::Reqs2Reqs, addons)?,
        })
    }
    pub fn with_runtime(mut self, runtime: std::sync::Arc<super::exec::Runtime>) -> Self {
        self.runtime = runtime;
        self
    }
    pub async fn exec(
        &self,
        requests: Vec<Request>,
        config: &Config,
        context: &Context,
    ) -> LoggedResult<Execution> {
        let mut result = Execution {
            requests,
            notifications: Vec::new(),
            logs: Vec::new(),
            exit_code: None,
        };
        for (index, c) in self.configs.iter().enumerate() {
            self.runtime.check_cancelled()?;
            result.requests = match c {
                AddonConfig::Exec(_) => {
                    self.runtime
                        .payload::<super::Reqs2Reqs>(
                            index,
                            super::payload::Reqs2ReqsPayload {
                                requests: result.requests,
                            },
                            config,
                            Default::default(),
                        )?
                        .requests
                }
                AddonConfig::Reqs2ReqsHead(c) => c.apply(result.requests),
                AddonConfig::Reqs2ReqsAdd(c) => c.apply(result.requests),
                AddonConfig::Reqs2ReqsFilter(c) => c.apply(result.requests)?,
                AddonConfig::Reqs2ReqsRepeat(c) => c.apply(result.requests)?,
                AddonConfig::Reqs2ReqsReplace(c) => c.apply(result.requests)?,
                AddonConfig::Reqs2ReqsRename(c) => c.apply(result.requests)?,
                AddonConfig::Reqs2ReqsShuffle(c) => c.apply(result.requests, None),
                AddonConfig::Reqs2ReqsEmptyGuard(c) => {
                    if result.requests.is_empty() {
                        result.logs.push(Log::new(
                            Level::Warning,
                            "Requests are empty. Exit executor.",
                        ));
                        match c.notify(config, context, &mut result.logs).await {
                            Ok(outcomes) => result.notifications = outcomes,
                            Err(error) => {
                                return Err(Failure {
                                    error,
                                    logs: result.logs,
                                });
                            }
                        }
                        result.exit_code = Some(1);
                        return Ok(result);
                    }
                    result.requests
                }
                _ => {
                    return Err(Error::new("ValueError", "expected reqs2reqs configuration").into());
                }
            };
        }
        Ok(result)
    }
}
