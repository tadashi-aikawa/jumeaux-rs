pub mod encoding;
pub mod html;
pub mod json;
pub mod xml;

use crate::{
    addons::{AddonConfig, AddonExecutor, Dump, parse_config, payload::DumpPayload},
    compat::{
        Error, Result,
        encoding::{self as codec, Errors},
    },
    models::Addon,
};

pub struct Factory;
impl crate::addons::Factory<Dump> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<Dump>>> {
        let config = parse_config(crate::addons::Layer::Dump, addon, "addons.dump")?;
        Ok(Box::new(Executor(config)))
    }
}
struct Executor(AddonConfig);
impl AddonExecutor<Dump> for Executor {
    fn exec(&self, mut payload: DumpPayload, _: &()) -> Result<DumpPayload> {
        let (default, force, kind) = match &self.0 {
            AddonConfig::DumpJson(c) => (&c.default_encoding, c.force, "json"),
            AddonConfig::DumpXml(c) => (&c.default_encoding, c.force, "xml"),
            AddonConfig::DumpHtml(c) => (&c.default_encoding, c.force, "html"),
            AddonConfig::DumpEncoding(c) => {
                let source = payload.encoding.as_deref().ok_or_else(|| {
                    Error::new("TypeError", "decode() encoding must be str, not None")
                })?;
                let text = codec::decode(&payload.body, source, Errors::Strict)?;
                payload.body = codec::encode(&text, &c.encoding, Errors::Strict)?;
                payload.encoding = Some(c.encoding.clone());
                return Ok(payload);
            }
            _ => unreachable!("dump registration"),
        };
        let encoding = payload.encoding.as_ref().unwrap_or(default).clone();
        if force || payload.response.response_type == kind {
            let text = codec::decode(&payload.body, &encoding, Errors::Replace)?;
            let text = match kind {
                "json" => {
                    crate::compat::json::dumps(&crate::compat::json::parse(&text)?, Some(4), true)?
                }
                "xml" => crate::markup::xml::pretty(&text).map_err(markup_error)?,
                "html" => crate::markup::html::pretty(&text).map_err(markup_error)?,
                _ => unreachable!(),
            };
            payload.body = codec::encode(&text, &encoding, Errors::Replace)?;
        }
        payload.encoding = Some(encoding);
        Ok(payload)
    }
}
fn markup_error(error: crate::markup::Error) -> Error {
    Error::new(error.python_exception(), error.to_string())
}
