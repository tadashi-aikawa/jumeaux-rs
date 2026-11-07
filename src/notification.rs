//! Slack payloadの生成と専用HTTP送信。ライブラリからログは出さない。
use crate::{
    compat::{Error, Result},
    models::Notifier,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub webhook_url: Option<String>,
    pub thread_ts: Option<String>,
}
impl Context {
    pub fn from_env() -> Self {
        Self {
            webhook_url: std::env::var("SLACK_INCOMING_WEBHOOKS_URL").ok(),
            thread_ts: std::env::var("SLACK_THREAD_TS").ok(),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub skipped: bool,
}

pub fn payload(notifier: &Notifier, message: &str, context: &Context) -> Result<Value> {
    let mut result = match notifier.version {
        1 => {
            let channel = notifier
                .channel
                .as_ref()
                .ok_or_else(|| Error::new("ValueError", "Slack v1 requires channel"))?;
            let mut result = json!({"text": message, "channel": channel, "username": notifier.username, "link_names": 1});
            if let Some(emoji) = &notifier.icon_emoji {
                result["icon_emoji"] = json!(format!(":{emoji}:"));
            }
            if let Some(url) = &notifier.icon_url {
                result["icon_url"] = json!(url);
            }
            result
        }
        2 => {
            let mut result = json!({"link_names": 1});
            if notifier.use_blocks {
                // PythonのTOption[TList[Block]]もlist以外を拒否する。
                let blocks: Vec<Value> = serde_json::from_str(message)
                    .map_err(|e| Error::new("JSONDecodeError", e.to_string()))?;
                result["blocks"] = json!(blocks);
            } else if !message.is_empty() {
                result["text"] = json!(message);
            }
            if let Some(thread) = context.thread_ts.as_ref().filter(|s| !s.is_empty()) {
                result["thread_ts"] = json!(thread);
            }
            result
        }
        _ => {
            return Err(Error::new(
                "ValueError",
                format!("unsupported slack version: {}", notifier.version),
            ));
        }
    };
    // OwlMixinは任意JSONのnullも再帰的に省略する。
    omit_nulls(&mut result);
    Ok(result)
}

pub fn notify(notifier: &Notifier, message: &str, context: &Context) -> Result<Outcome> {
    let Some(url) = &context.webhook_url else {
        return Ok(Outcome { warnings: vec![
            "[notification_handlers] SLACK_INCOMING_WEBHOOKS_URL is not defined in environmental variables.".into(),
            "[notification_handlers] Please set SLACK_INCOMING_WEBHOOKS_URL.".into(),
            "[notification_handlers] Skip notify to...".into(),
        ], skipped: true, ..Outcome::default() });
    };
    let data = payload(notifier, message, context)?;
    send(url, &data)
}

/// requests.post(data=bytes)に合わせ、Content-Typeを自動設定しない。
fn send(url: &str, payload: &Value) -> Result<Outcome> {
    let response = reqwest::blocking::Client::new()
        .post(url)
        .body(json_bytes(payload)?)
        .send()
        .map_err(|e| Error::new("ConnectionError", e.to_string()))?;
    let ok = response.status().as_u16() == 200;
    let error = if ok {
        None
    } else {
        Some(
            response
                .text()
                .map_err(|e| Error::new("ConnectionError", e.to_string()))?,
        )
    };
    Ok(Outcome {
        error,
        ..Outcome::default()
    })
}

fn omit_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for v in map.values_mut() {
                omit_nulls(v);
            }
        }
        Value::Array(list) => {
            list.retain(|v| !v.is_null());
            for v in list {
                omit_nulls(v);
            }
        }
        _ => (),
    }
}
fn json_bytes(value: &Value) -> Result<Vec<u8>> {
    fn sort(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.sort_keys();
                for v in map.values_mut() {
                    sort(v);
                }
            }
            Value::Array(items) => {
                for v in items {
                    sort(v);
                }
            }
            _ => (),
        }
    }
    struct Formatter;
    impl serde_json::ser::Formatter for Formatter {
        fn begin_object_value<W: ?Sized + std::io::Write>(
            &mut self,
            writer: &mut W,
        ) -> std::io::Result<()> {
            writer.write_all(b": ")
        }
    }
    let mut value = value.clone();
    sort(&mut value);
    let mut bytes = Vec::new();
    serde::Serialize::serialize(
        &value,
        &mut serde_json::Serializer::with_formatter(&mut bytes, Formatter),
    )
    .map_err(|e| Error::new("ValueError", e.to_string()))?;
    Ok(bytes)
}
