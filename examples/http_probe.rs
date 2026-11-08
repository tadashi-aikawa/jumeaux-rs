//! HTTP受入用のJSONドライバー。製品CLIのアドオン実行には接続しない。
use jumeaux::{
    http::{ChallengeOutcome, HttpTransport, TransportOptions, prepare},
    models::{AccessPoint, Request, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Deserialize)]
struct Probe {
    one: AccessPoint,
    other: Option<AccessPoint>,
    requests: Vec<Request>,
    #[serde(default)]
    max_retries: i64,
    timeout: Option<f64>,
    #[serde(default)]
    trust_env: bool,
    ca_bundle: Option<std::path::PathBuf>,
    injected: Option<String>,
    action: Option<String>,
    notifier: Option<jumeaux::models::Notifier>,
    message: Option<String>,
}

struct Resolver {
    kind: io::ErrorKind,
    attempts: AtomicUsize,
}
impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let kind = self.kind;
        Box::pin(async move {
            Err(Box::new(io::Error::new(kind, "fixture"))
                as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

fn response(res: Response) -> Value {
    json!({"status_code": res.status_code, "body_hex": res.body.iter().map(|b| format!("{b:02x}")).collect::<String>(), "url": res.url, "headers": res.headers, "encoding": res.encoding, "type": res.response_type})
}

#[tokio::main]
async fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let probe: Probe = serde_json::from_str(&input).unwrap();
    let resolver = probe.injected.as_ref().map(|kind| {
        Arc::new(Resolver {
            kind: match kind.as_str() {
                "connect-timeout" => io::ErrorKind::TimedOut,
                "connection-refused" => io::ErrorKind::ConnectionRefused,
                _ => io::ErrorKind::NotFound,
            },
            attempts: AtomicUsize::new(0),
        })
    });
    let options = TransportOptions {
        trust_env: probe.trust_env,
        read_timeout: probe.timeout.map(Duration::from_secs_f64),
        connect_timeout: probe.timeout.map(Duration::from_secs_f64),
        ca_bundle: probe.ca_bundle,
        resolver: resolver
            .as_ref()
            .map(|r| r.clone() as Arc<dyn reqwest::dns::Resolve>),
    };
    let transport = match HttpTransport::with_options(
        &probe.one,
        probe.other.as_ref().unwrap_or(&probe.one),
        probe.max_retries,
        options,
    ) {
        Ok(transport) => transport,
        Err(error) => {
            println!("{}", json!({"error": error.kind, "message": error.message}));
            return;
        }
    };
    let mut results = Vec::new();
    for request in probe.requests {
        let result = match probe.action.as_deref() {
            Some("notify") => {
                let notifier = probe.notifier.clone().unwrap_or_else(|| serde_json::from_value(json!({"type": "slack", "version": 2})).unwrap());
                jumeaux::notification::notify(&notifier, probe.message.as_deref().unwrap_or("TLS fixture"), &jumeaux::notification::Context {
                    webhook_url: Some(format!("{}{}", probe.one.host, request.path)), thread_ts: None,
                }).await.map(|outcome| json!({"ok": outcome.error.is_none(), "notification_error": outcome.error}))
            }
            Some("prepare") => prepare(&probe.one, &request).map(|p| json!({"original_url": p.original_url, "url": p.url.to_string(), "headers": p.headers, "body_hex": p.body.iter().map(|b| format!("{b:02x}")).collect::<String>()})),
            Some("pair") => transport.concurrent_request(&request).await.map(|(one, other)| json!({"one": response(one), "other": response(other)})),
            Some("challenge") => transport.challenge(1, &request).await.map(|outcome| match outcome {
                ChallengeOutcome::Responses(pair) => json!({"one": response(pair.one), "other": response(pair.other)}),
                ChallengeOutcome::Failure(trial) => { let mut value = serde_json::to_value(trial).unwrap(); value.as_object_mut().unwrap().remove("request_time"); value }
            }),
            _ => transport.send_one(&request).await.map(response),
        };
        results.push(result.unwrap_or_else(|e| json!({"error": e.kind, "message": e.message})));
    }
    println!(
        "{}",
        json!({"results": results, "attempts": resolver.map(|r| r.attempts.load(Ordering::SeqCst))})
    );
}
