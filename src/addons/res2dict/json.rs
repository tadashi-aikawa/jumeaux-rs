//! Python addons/res2dict/json.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub force: bool,
}

impl crate::addons::AddonExecutor<crate::addons::Res2Dict> for Config {
    fn exec(
        &self,
        payload: crate::addons::payload::Res2DictPayload,
        _: &(),
    ) -> crate::compat::Result<crate::addons::payload::Res2DictPayload> {
        self.exec_with_log(payload, &(), &|_| {})
    }
    fn exec_with_log(
        &self,
        mut payload: crate::addons::payload::Res2DictPayload,
        _: &(),
        log: &dyn Fn(crate::addons::log::AddonLog),
    ) -> crate::compat::Result<crate::addons::payload::Res2DictPayload> {
        log(crate::addons::log::conversion(
            "json",
            self.force,
            payload.result.is_some(),
            &payload.response.response_type,
        ));
        if payload.result.is_some() && !self.force {
            return Ok(payload);
        }
        payload.result = if self.force || payload.response.response_type == "json" {
            let value = crate::compat::json::parse(&payload.response.text()?)?;
            if value == serde_json::Value::Null || value.as_str() == Some("") {
                None
            } else {
                Some(value)
            }
        } else {
            None
        };
        Ok(payload)
    }
}
