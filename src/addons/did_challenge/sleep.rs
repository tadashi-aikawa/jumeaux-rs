//! Python addons/did_challenge/sleep.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub max: f64,
    pub min: f64,
}

use crate::{
    addons::{
        AddonExecutor, DidChallenge,
        payload::{DidChallengePayload, DidChallengeReference},
    },
    compat::{Error, Result},
};
use std::hash::{BuildHasher, Hasher};

impl Config {
    /// Python random.uniformと同じ式。unitは0以上1未満の乱数。
    pub fn seconds(&self, unit: f64) -> Result<std::time::Duration> {
        let seconds = self.min + (self.max - self.min) * unit;
        std::time::Duration::try_from_secs_f64(seconds)
            .map_err(|_| Error::new("ValueError", "sleep length must be non-negative and finite"))
    }
}
impl AddonExecutor<DidChallenge> for Config {
    fn exec(
        &self,
        payload: DidChallengePayload,
        _: &DidChallengeReference,
    ) -> Result<DidChallengePayload> {
        let bits = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let unit = (bits >> 11) as f64 / ((1u64 << 53) as f64);
        // challengeはこの同期Chain全体をspawn_blocking内で実行する。
        std::thread::sleep(self.seconds(unit)?);
        Ok(payload)
    }
}
