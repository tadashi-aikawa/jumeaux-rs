pub mod json;
pub mod json_sort;
pub mod r#type;

use crate::addons::{AddonConfig, AddonExecutor, Res2Res, parse_config};
use crate::{compat::Result, models::Addon};

pub struct Factory;
impl crate::addons::Factory<Res2Res> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<Res2Res>>> {
        Ok(
            match parse_config(crate::addons::Layer::Res2Res, addon, "addons.res2res")? {
                AddonConfig::Res2ResJsonSort(config) => Box::new(json_sort::Executor::new(config)?),
                AddonConfig::Res2ResType(config) => Box::new(r#type::Executor::new(config)?),
                _ => unreachable!("res2res registration"),
            },
        )
    }
}
