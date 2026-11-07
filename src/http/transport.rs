use super::{PreparedRequest, prepare, request::requote_url};
use crate::{
    compat::{Error, Result, encoding},
    models::{AccessPoint, Headers, HttpMethod, Request, Response},
};
use reqwest::{
    Client, Proxy,
    cookie::{CookieStore, Jar},
    header::{HeaderMap, HeaderName, HeaderValue},
};
use std::{
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// 製品ではDefaultを使用する。timeoutとresolverはローカル受入でも使える。
#[derive(Clone)]
pub struct TransportOptions {
    pub trust_env: bool,
    pub connect_timeout: Option<Duration>,
    pub read_timeout: Option<Duration>,
    pub ca_bundle: Option<PathBuf>,
    pub resolver: Option<Arc<dyn reqwest::dns::Resolve>>,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            trust_env: true,
            connect_timeout: None,
            read_timeout: None,
            ca_bundle: None,
            resolver: None,
        }
    }
}

pub(super) struct Endpoint {
    pub access: AccessPoint,
    client: Client,
}

pub struct HttpTransport {
    pub(super) one: Endpoint,
    pub(super) other: Endpoint,
    cookies: Arc<Jar>,
    max_retries: u64,
}

pub(super) struct ReceivedResponse {
    body: Vec<u8>,
    headers: Headers,
    url: String,
    status_code: u16,
    elapsed: f64,
}

impl ReceivedResponse {
    /// 両側の通信成功後にモデルへ変換する。encodingエラーより通信失敗を先に判定する。
    pub(super) fn into_response(self, default_encoding: Option<&str>) -> Result<Response> {
        let ct = self.headers.get("content-type").map(String::as_str);
        let encoding = encoding::http_encoding(ct, &self.body, default_encoding)?;
        let response_type = if let Some(ct) = ct.filter(|v| !v.is_empty()) {
            ct.split(';')
                .next()
                .unwrap()
                .split('/')
                .nth(1)
                .ok_or_else(|| Error::new("IndexError", "Content-Type has no subtype"))?
                .to_string()
        } else {
            "unknown".into()
        };
        Ok(Response {
            body: self.body.into(),
            headers: self.headers,
            url: self.url,
            status_code: self.status_code,
            elapsed: self.elapsed,
            elapsed_sec: (self.elapsed * 100.0).round_ties_even() / 100.0,
            encoding,
            response_type,
        })
    }
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::new("InvalidURL", error.to_string())
}

fn env_proxy(name: &str) -> Option<String> {
    std::env::var(name.to_ascii_lowercase())
        .ok()
        .or_else(|| {
            if name == "HTTP_PROXY" && std::env::var_os("REQUEST_METHOD").is_some() {
                None
            } else {
                std::env::var(name).ok()
            }
        })
        .filter(|s| !s.is_empty())
}

fn bypass_proxy(url: &reqwest::Url, bypass: Option<&str>) -> bool {
    let Some(spec) = bypass else {
        return false;
    };
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let with_port = url
        .port()
        .map(|port| format!("{host}:{port}"))
        .unwrap_or_else(|| host.to_string());
    spec.split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            if let Ok(address) = host.parse::<std::net::Ipv4Addr>() {
                if let Some((network, prefix)) = entry.split_once('/')
                    && let (Ok(network), Ok(prefix)) =
                        (network.parse::<std::net::Ipv4Addr>(), prefix.parse::<u32>())
                    && (1..=32).contains(&prefix)
                {
                    let mask = u32::MAX << (32 - prefix);
                    return u32::from(address) & mask == u32::from(network) & mask;
                }
                host == entry
            } else {
                host.ends_with(entry) || with_port.ends_with(entry)
            }
        })
}

fn build_endpoint(access: &AccessPoint, options: &TransportOptions) -> Result<Endpoint> {
    let mut builder = Client::builder()
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        // Pythonは各転送後Session.closeする。Cookieは独立したJarで共有する。
        .pool_max_idle_per_host(0)
        .no_proxy();
    if let Some(timeout) = options.connect_timeout {
        builder = builder.connect_timeout(timeout);
    }
    if let Some(timeout) = options.read_timeout {
        builder = builder.read_timeout(timeout);
    }
    if let Some(resolver) = &options.resolver {
        builder = builder.dns_resolver2(resolver.clone());
    }
    if let Some(proxy) = &access.proxy {
        // Python Proxy.from_hostはprotocolごとにproxy自体のschemeも切り替える。
        builder = builder
            .proxy(Proxy::http(format!("http://{proxy}")).map_err(invalid)?)
            .proxy(Proxy::https(format!("https://{proxy}")).map_err(invalid)?);
    } else if options.trust_env {
        let all = env_proxy("ALL_PROXY");
        let http = env_proxy("HTTP_PROXY").or_else(|| all.clone());
        let https = env_proxy("HTTPS_PROXY").or(all);
        let bypass = env_proxy("NO_PROXY");
        // OSのproxy設定は拾わずrequestsと同じ環境変数を使う。
        let proxy = Proxy::custom(move |url| {
            if bypass_proxy(url, bypass.as_deref()) {
                return None;
            }
            match url.scheme() {
                "http" => http.clone(),
                "https" => https.clone(),
                _ => None,
            }
        });
        builder = builder.proxy(proxy);
    }
    let ca_bundle = options.ca_bundle.clone().or_else(|| {
        options
            .trust_env
            .then(|| {
                std::env::var_os("REQUESTS_CA_BUNDLE")
                    .filter(|v| !v.is_empty())
                    .or_else(|| std::env::var_os("CURL_CA_BUNDLE").filter(|v| !v.is_empty()))
                    .map(PathBuf::from)
            })
            .flatten()
    });
    if let Some(path) = ca_bundle {
        let pem = std::fs::read(&path)
            .map_err(|e| Error::new("OSError", format!("{}: {e}", path.display())))?;
        let certificates = reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|e| Error::new("SSLError", e.to_string()))?;
        if certificates.is_empty() {
            return Err(Error::new("SSLError", "empty CA bundle"));
        }
        builder = builder.tls_built_in_root_certs(false);
        for certificate in certificates {
            builder = builder.add_root_certificate(certificate);
        }
    }
    let client = builder
        .build()
        .map_err(|e| Error::new("SSLError", e.to_string()))?;
    Ok(Endpoint {
        access: access.clone(),
        client,
    })
}

fn header_map(headers: &Headers) -> Result<HeaderMap> {
    let mut result = HeaderMap::new();
    result.insert("accept", HeaderValue::from_static("*/*"));
    result.insert("accept-encoding", HeaderValue::from_static("gzip, deflate"));
    result.insert("connection", HeaderValue::from_static("keep-alive"));
    for (name, value) in headers {
        if value.starts_with(char::is_whitespace) || value.contains(['\n', '\r']) {
            return Err(Error::new("InvalidHeader", name));
        }
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| Error::new("InvalidHeader", e.to_string()))?;
        let bytes = encoding::encode(value, "latin1", encoding::Errors::Strict)?;
        let value = HeaderValue::from_bytes(&bytes)
            .map_err(|e| Error::new("InvalidHeader", e.to_string()))?;
        result.insert(name, value);
    }
    Ok(result)
}

fn response_headers(headers: &HeaderMap) -> Headers {
    let mut result = Headers::new();
    for (key, value) in headers {
        let value: String = value.as_bytes().iter().map(|&b| b as char).collect();
        result
            .entry(key.as_str().into())
            .and_modify(|v| {
                v.push_str(", ");
                v.push_str(&value);
            })
            .or_insert(value);
    }
    result
}

fn send_error(error: reqwest::Error, method: HttpMethod, max_retries: u64) -> Error {
    let kind = if error.is_connect() && error.is_timeout() {
        "ConnectTimeout"
    } else if error.is_timeout() && (method == HttpMethod::Post || max_retries == 0) {
        "ReadTimeout"
    } else if error.is_builder() {
        "InvalidURL"
    } else {
        "ConnectionError"
    };
    Error::new(kind, format!("{error:?}"))
}

fn retry_after(value: &HeaderValue) -> Result<Duration> {
    let text = value
        .to_str()
        .map_err(|e| Error::new("InvalidHeader", e.to_string()))?
        .trim();
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return text
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| Error::new("InvalidHeader", e.to_string()));
    }
    chrono::DateTime::parse_from_rfc2822(text)
        .map(|at| {
            (at.with_timezone(&chrono::Utc) - chrono::Utc::now())
                .to_std()
                .unwrap_or_default()
        })
        .map_err(|e| Error::new("InvalidHeader", e.to_string()))
}

fn expand(mut body: Vec<u8>, headers: &Headers) -> Result<Vec<u8>> {
    if body.is_empty() {
        return Ok(body);
    }
    let encodings = headers
        .get("content-encoding")
        .map(String::as_str)
        .unwrap_or("");
    for coding in encodings.split(',').rev().map(str::trim) {
        let mut decoded = Vec::new();
        let result = match coding {
            "gzip" | "x-gzip" => {
                flate2::read::MultiGzDecoder::new(body.as_slice()).read_to_end(&mut decoded)
            }
            "deflate" => {
                let result =
                    flate2::read::ZlibDecoder::new(body.as_slice()).read_to_end(&mut decoded);
                if result.is_err() {
                    decoded.clear();
                    flate2::read::DeflateDecoder::new(body.as_slice()).read_to_end(&mut decoded)
                } else {
                    result
                }
            }
            _ => continue,
        };
        result.map_err(|e| Error::new("ContentDecodingError", e.to_string()))?;
        body = decoded;
    }
    Ok(body)
}

impl HttpTransport {
    pub fn new(one: &AccessPoint, other: &AccessPoint, max_retries: i64) -> Result<Self> {
        Self::with_options(one, other, max_retries, TransportOptions::default())
    }

    pub fn with_options(
        one: &AccessPoint,
        other: &AccessPoint,
        max_retries: i64,
        options: TransportOptions,
    ) -> Result<Self> {
        Ok(Self {
            one: build_endpoint(one, &options)?,
            other: build_endpoint(other, &options)?,
            cookies: Arc::new(Jar::default()),
            max_retries: max_retries.max(0) as u64,
        })
    }

    pub async fn send_one(&self, request: &Request) -> Result<Response> {
        self.send_prepared(&self.one, prepare(&self.one.access, request)?)
            .await?
            .into_response(self.one.access.default_response_encoding.as_deref())
    }

    pub async fn send_other(&self, request: &Request) -> Result<Response> {
        self.send_prepared(&self.other, prepare(&self.other.access, request)?)
            .await?
            .into_response(self.other.access.default_response_encoding.as_deref())
    }

    async fn send_hop(
        &self,
        endpoint: &Endpoint,
        prepared: &PreparedRequest,
        headers: &HeaderMap,
    ) -> Result<reqwest::Response> {
        let mut retries = 0;
        loop {
            let method = match prepared.method {
                HttpMethod::Get => reqwest::Method::GET,
                HttpMethod::Post => reqwest::Method::POST,
            };
            let mut request = endpoint
                .client
                .request(method, prepared.url.clone())
                .headers(headers.clone());
            if prepared.method == HttpMethod::Post {
                request = request.body(prepared.body.clone());
            }
            match request.send().await {
                Ok(response) => {
                    if prepared.method == HttpMethod::Get
                        && retries < self.max_retries
                        && matches!(response.status().as_u16(), 413 | 429 | 503)
                        && let Some(after) = response.headers().get("retry-after")
                    {
                        let delay = retry_after(after)?;
                        // urllib3もretry時の本文drain失敗を無視する。
                        let _ = response.bytes().await;
                        tokio::time::sleep(delay).await;
                        retries += 1;
                        continue;
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let retryable = !error.is_builder()
                        && (error.is_connect() || prepared.method == HttpMethod::Get);
                    if retryable && retries < self.max_retries {
                        retries += 1;
                        continue;
                    }
                    return Err(send_error(error, prepared.method, self.max_retries));
                }
            }
        }
    }

    pub(super) async fn send_prepared(
        &self,
        endpoint: &Endpoint,
        mut prepared: PreparedRequest,
    ) -> Result<ReceivedResponse> {
        let mut headers = header_map(&prepared.headers)?;
        for redirects in 0..=30 {
            if !headers.contains_key("cookie")
                && let Some(cookie) = self.cookies.cookies(&prepared.url)
            {
                headers.insert("cookie", cookie);
            }
            let start = Instant::now();
            let response = self.send_hop(endpoint, &prepared, &headers).await?;
            let elapsed = start.elapsed().as_secs_f64();
            let url = response.url().clone();
            let status_code = response.status().as_u16();
            let location = response.headers().get("location").cloned();
            self.cookies
                .set_cookies(&mut response.headers().get_all("set-cookie").iter(), &url);
            let response_headers = response_headers(response.headers());
            let bytes = response.bytes().await.map_err(|e| {
                Error::new(
                    if e.is_timeout() {
                        "ConnectionError"
                    } else {
                        "ChunkedEncodingError"
                    },
                    format!("{e:?}"),
                )
            })?;
            let body = expand(bytes.to_vec(), &response_headers)?;
            if matches!(status_code, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = location
            {
                if redirects == 30 {
                    return Err(Error::new("TooManyRedirects", "Exceeded 30 redirects"));
                }
                let location = std::str::from_utf8(location.as_bytes())
                    .map_err(|e| Error::new("UnicodeDecodeError", e.to_string()))?;
                let mut next = url.join(&requote_url(location)).map_err(invalid)?;
                if !matches!(next.scheme(), "http" | "https") {
                    return Err(Error::new("InvalidSchema", next.scheme()));
                }
                if next.fragment().is_none() {
                    next.set_fragment(url.fragment());
                }
                let upgrade = url.scheme() == "http"
                    && next.scheme() == "https"
                    && url.port_or_known_default() == Some(80)
                    && next.port_or_known_default() == Some(443);
                if url.host_str() != next.host_str()
                    || (!upgrade
                        && (url.scheme() != next.scheme()
                            || url.port_or_known_default() != next.port_or_known_default()))
                {
                    headers.remove("authorization");
                }
                headers.remove("cookie");
                if matches!(status_code, 301..=303) {
                    prepared.method = HttpMethod::Get;
                    prepared.body.clear();
                    for name in ["content-length", "content-type", "transfer-encoding"] {
                        headers.remove(name);
                    }
                }
                prepared.url = next;
                continue;
            }
            return Ok(ReceivedResponse {
                body,
                headers: response_headers,
                url: url.into(),
                status_code,
                elapsed,
            });
        }
        unreachable!("redirect limit returns an error")
    }
}
