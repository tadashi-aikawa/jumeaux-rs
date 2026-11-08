//! Python addons/res2res/json.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub transformer: crate::addons::exec::Config,
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub default_encoding: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<String>,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());

use crate::{
    addons::{
        AddonExecutor, Layer, Res2Res,
        exec::{Invocation, Runtime, wire},
        payload::Res2ResPayload,
    },
    compat::{
        Result,
        encoding::{self, Errors},
        template::TemplateEngine,
        values::checked_context,
    },
};
use serde_json::{Value, json};

pub struct Compiled {
    config: Config,
    engine: TemplateEngine,
    path: String,
}
impl Compiled {
    pub fn new(config: Config, path: String) -> Result<Self> {
        let engine = TemplateEngine::new();
        if let Some(when) = &config.when {
            engine
                .load(&format!("{{{{{when}}}}}"))
                .map_err(|e| crate::config::invalid(&format!("{path}.when"), e))?;
        }
        Ok(Self {
            config,
            engine,
            path,
        })
    }
    pub fn apply(
        &self,
        runtime: &Runtime,
        index: usize,
        mut payload: Res2ResPayload,
        invocation: Invocation,
    ) -> Result<Res2ResPayload> {
        if let Some(when) = &self.config.when {
            let context = checked_context(minijinja::context! {
                req => payload.req.to_template_value(), res => payload.response.to_template_value()
            })?;
            if !self
                .engine
                .when(when, &context)
                .map_err(|mut e| {
                    e.message = format!("{}.when: {}", self.path, e.message);
                    e
                })?
                .truthy()
            {
                return Ok(payload);
            }
        }
        let response = &mut payload.response;
        let text = runtime.invoke(
            Layer::Res2Res,
            index,
            "json_transformer",
            json!({"body":wire::bytes(&response.body),"encoding":response.encoding}),
            Value::Null,
            invocation,
            |v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| crate::addons::exec::json::protocol("$: expected JSON string"))
            },
        )?;
        let encoding = response
            .encoding
            .clone()
            .unwrap_or_else(|| self.config.default_encoding.clone());
        response.body = encoding::encode(&text, &encoding, Errors::Replace)
            .map_err(|mut e| {
                e.message = format!("{}.default_encoding: {}", self.path, e.message);
                e
            })?
            .into();
        response.encoding = Some(encoding);
        response.response_type = "json".into();
        Ok(payload)
    }
}
pub struct Executor {
    runtime: std::sync::Arc<Runtime>,
}
impl Executor {
    pub fn new(config: Config) -> Result<Self> {
        let addon = crate::models::Addon {
            name: "json".into(),
            cls_name: "Executor".into(),
            config: Some(
                serde_json::to_value(config)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            include: None,
            tags: None,
        };
        Ok(Self {
            runtime: Runtime::prepare_layer(Layer::Res2Res, &[addon])?,
        })
    }
}
impl AddonExecutor<Res2Res> for Executor {
    fn exec(&self, payload: Res2ResPayload, reference: &()) -> Result<Res2ResPayload> {
        self.runtime
            .payload::<Res2Res>(0, payload, reference, Invocation::default())
    }
}
