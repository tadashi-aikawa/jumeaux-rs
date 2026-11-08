use crate::{compat::Result, config::invalid, models::Object};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub command: Vec<String>,
    #[serde(default = "timeout", deserialize_with = "null_timeout")]
    pub timeout_sec: f64,
    pub cwd: Option<String>,
    #[serde(default = "inherit", deserialize_with = "null_inherit")]
    pub inherit_env: bool,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub env: IndexMap<String, Option<String>>,
    #[serde(default = "stdout_limit", deserialize_with = "null_stdout")]
    pub max_stdout_bytes: usize,
    #[serde(default = "stderr_limit", deserialize_with = "null_stderr")]
    pub max_stderr_bytes: usize,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub options: Object,
}
fn timeout() -> f64 {
    60.0
}
fn inherit() -> bool {
    true
}
fn stdout_limit() -> usize {
    64 * 1024 * 1024
}
fn stderr_limit() -> usize {
    64 * 1024
}
use crate::models::defaults::null_default_with;
null_default_with!(null_timeout, f64, timeout());
null_default_with!(null_inherit, bool, inherit());
null_default_with!(null_stdout, usize, stdout_limit());
null_default_with!(null_stderr, usize, stderr_limit());

impl Config {
    pub fn validate(&self, path: &str) -> Result<()> {
        if self.command.is_empty()
            || self.command[0].is_empty()
            || self.command.iter().any(|s| s.contains('\0'))
        {
            return Err(invalid(
                &format!("{path}.command"),
                "expected nonempty argv without NUL",
            ));
        }
        if !self.timeout_sec.is_finite()
            || self.timeout_sec <= 0.0
            || std::time::Duration::try_from_secs_f64(self.timeout_sec).is_err()
        {
            return Err(invalid(
                &format!("{path}.timeout_sec"),
                "expected positive finite duration",
            ));
        }
        if self.max_stdout_bytes == 0 {
            return Err(invalid(
                &format!("{path}.max_stdout_bytes"),
                "must be positive",
            ));
        }
        if self.cwd.as_ref().is_some_and(|s| s.contains('\0')) {
            return Err(invalid(&format!("{path}.cwd"), "NUL is not allowed"));
        }
        for (key, value) in &self.env {
            if key.is_empty()
                || key.contains(['=', '\0'])
                || value.as_ref().is_some_and(|s| s.contains('\0'))
            {
                return Err(invalid(
                    &format!("{path}.env"),
                    "invalid environment name or value",
                ));
            }
        }
        crate::compat::json::validate(&serde_json::Value::Object(self.options.clone())).map_err(
            |_| {
                invalid(
                    &format!("{path}.options"),
                    "non-finite number or excessive JSON depth",
                )
            },
        )?;
        Ok(())
    }
}
