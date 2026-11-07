//! 入力ファイルを正式Requestへ変換する。codecとquery解釈はcompatの責務。
pub mod csv;
pub mod json;
pub mod plain;
pub mod yaml;
use super::{
    AddonConfig, AddonExecutor, Factory, Layer, Log2Reqs, parse_config, payload::Log2ReqsPayload,
};
use crate::{
    compat::{Error, Result},
    models::{Addon, Request},
};
pub use plain::plain_requests;

/// バイト列の入口。ファイルI/Oを持たず、比較fixtureにも使う。
pub fn read(addon: &Addon, body: &[u8]) -> Result<Vec<Request>> {
    read_config(
        &parse_config(Layer::Log2Reqs, addon, "addons.log2reqs")?,
        body,
    )
}

/// CLI側の接続点。全ファイルを指定順に連結し、reqs2reqsとempty_guardまで実行する。
pub fn prepare(
    config: &crate::config::Config,
    context: &crate::notification::Context,
) -> Result<super::reqs2reqs::Execution> {
    let pipeline = super::reqs2reqs::Pipeline::build(&config.addons.reqs2reqs)?;
    let reader = super::Chain::<Log2Reqs>::build(&config.addons, &InputFactory)?;
    let files = config
        .input_files
        .as_ref()
        .ok_or_else(|| Error::new("RequiredError", "input_files is required"))?;
    let mut requests = Vec::new();
    for file in files {
        requests.extend(
            reader
                .exec(
                    Log2ReqsPayload {
                        file: file.clone(),
                        requests: Vec::new(),
                    },
                    &(),
                )?
                .requests,
        );
    }
    pipeline.exec(requests, config, context)
}
fn read_config(config: &AddonConfig, body: &[u8]) -> Result<Vec<Request>> {
    match config {
        AddonConfig::Log2ReqsPlain(c) => c.read(body),
        AddonConfig::Log2ReqsCsv(c) => c.read(body),
        AddonConfig::Log2ReqsJson(c) => c.read(body),
        AddonConfig::Log2ReqsYaml(c) => c.read(body),
        _ => Err(Error::new("ValueError", "expected log2reqs configuration")),
    }
}
pub struct InputFactory;
struct InputExecutor(AddonConfig);
impl Factory<Log2Reqs> for InputFactory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<Log2Reqs>>> {
        Ok(Box::new(InputExecutor(parse_config(
            Layer::Log2Reqs,
            addon,
            "addons.log2reqs",
        )?)))
    }
}
impl AddonExecutor<Log2Reqs> for InputExecutor {
    fn exec(&self, mut payload: Log2ReqsPayload, _: &()) -> Result<Log2ReqsPayload> {
        let body =
            std::fs::read(&payload.file).map_err(|e| Error::new("IOError", e.to_string()))?;
        payload.requests = read_config(&self.0, &body)?;
        Ok(payload)
    }
}

fn requests(value: serde_json::Value) -> Result<Vec<Request>> {
    if value.as_object().is_some_and(|v| v.is_empty()) {
        return Ok(Vec::new());
    }
    let items = value
        .as_array()
        .ok_or_else(|| Error::new("ValueError", "requests must be a list"))?;
    items
        .iter()
        .map(|item| {
            if item.get("path").is_none_or(serde_json::Value::is_null) {
                return Err(Error::new("RequiredError", "path is required"));
            }
            serde_json::from_value(item.clone()).map_err(|e| {
                let kind = if e.to_string().starts_with("unknown field") {
                    "UnknownPropertiesError"
                } else {
                    "ValueError"
                };
                Error::new(kind, e.to_string())
            })
        })
        .collect()
}
