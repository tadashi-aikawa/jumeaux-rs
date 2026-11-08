pub mod free;

use crate::{
    addons::{AddonConfig, AddonExecutor, StoreCriterion, parse_config},
    compat::Result,
    models::Addon,
};

pub struct Factory;
impl crate::addons::Factory<StoreCriterion> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<StoreCriterion>>> {
        match parse_config(
            crate::addons::Layer::StoreCriterion,
            addon,
            "addons.store_criterion",
        )? {
            AddonConfig::StoreCriterionFree(config) => Ok(Box::new(free::Executor::new(config)?)),
            _ => unreachable!("store_criterion registration"),
        }
    }
}
