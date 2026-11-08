pub mod block;
pub mod html;
pub mod json;
pub mod xml;

use crate::addons::{AddonConfig, AddonExecutor, Res2Dict, parse_config};
use crate::compat::{Error, Result};
use crate::models::Addon;

pub struct Factory;
impl crate::addons::Factory<Res2Dict> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<Res2Dict>>> {
        Ok(
            match parse_config(crate::addons::Layer::Res2Dict, addon, "addons.res2dict")? {
                AddonConfig::Res2DictJson(config) => Box::new(config),
                AddonConfig::Res2DictXml(config) => Box::new(config),
                AddonConfig::Res2DictHtml(config) => Box::new(config),
                AddonConfig::Res2DictBlock(config) => Box::new(block::Executor::new(config)?),
                _ => unreachable!("res2dict registration"),
            },
        )
    }
}

fn markup_error(error: crate::markup::Error) -> Error {
    Error::new(error.python_exception(), error.to_string())
}
