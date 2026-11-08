//! Python addons/res2dict/xml.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub force: bool,
}

impl crate::addons::AddonExecutor<crate::addons::Res2Dict> for Config {
    fn exec(
        &self,
        mut payload: crate::addons::payload::Res2DictPayload,
        _: &(),
    ) -> crate::compat::Result<crate::addons::payload::Res2DictPayload> {
        if payload.result.is_some() && !self.force {
            return Ok(payload);
        }
        payload.result = if self.force || payload.response.response_type == "xml" {
            Some(
                crate::markup::xml::to_dict(&payload.response.text()?)
                    .map_err(super::markup_error)?,
            )
        } else {
            None
        };
        Ok(payload)
    }
}
