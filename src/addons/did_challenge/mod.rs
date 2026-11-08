pub mod sleep;
pub mod tag;

use crate::{
    addons::{AddonConfig, AddonExecutor, DidChallenge, parse_config},
    compat::Result,
    models::Addon,
};

pub struct Factory;
impl crate::addons::Factory<DidChallenge> for Factory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<DidChallenge>>> {
        Ok(
            match parse_config(
                crate::addons::Layer::DidChallenge,
                addon,
                "addons.did_challenge",
            )? {
                AddonConfig::DidChallengeTag(config) => Box::new(tag::Executor::new(config)?),
                AddonConfig::DidChallengeSleep(config) => Box::new(config),
                _ => unreachable!("did_challenge registration"),
            },
        )
    }
}
