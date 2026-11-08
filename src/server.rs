//! server/viewerの静的配信。Viewerはlatestをリクエストごとに解決する。
use crate::logging::{Logger, repr};
use jumeaux::{
    compat::{
        Error, Result,
        encoding::{self, Errors},
    },
    notification::Verbosity,
};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const RELOAD: &str = r#"<script>
(() => {
  let previous;
  async function check() {
    try {
      const response = await fetch('/__jumeaux_reload', {cache: 'no-store'});
      if (response.ok) {
        const current = await response.text();
        if (previous !== undefined && current !== previous) location.reload();
        previous = current;
      }
    } catch (_) {}
    setTimeout(check, 500);
  }
  check();
})();
</script>"#;

pub fn run(port: i64, root: PathBuf, viewer: bool, logger: Logger) -> Result<u8> {
    let port = u16::try_from(port)
        .map_err(|_| Error::new("ValueError", "port must be between 0 and 65535"))?;
    let listener = TcpListener::bind((if viewer { "127.0.0.1" } else { "0.0.0.0" }, port))
        .map_err(io_error)?;
    let address = listener.local_addr().map_err(io_error)?;
    logger.log(
        Verbosity::Normal,
        format!(
            "Serving HTTP on {} port {} (http://{}:{}/)",
            address.ip(),
            address.port(),
            address.ip(),
            address.port()
        ),
    );
    if viewer && std::env::var_os("JUMEAUX_NO_BROWSER").is_none() {
        let url = format!("http://127.0.0.1:{}/", address.port());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            #[cfg(target_os = "macos")]
            let result = std::process::Command::new("open").arg(&url).status();
            #[cfg(target_os = "windows")]
            let result = std::process::Command::new("rundll32")
                .args(["url.dll,FileProtocolHandler", &url])
                .status();
            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            let result = std::process::Command::new("xdg-open").arg(&url).status();
            if let Err(error) = result {
                logger.log(Verbosity::Warning, format!("Cannot open browser: {error}"));
            }
        });
    }
    let revision = Arc::new(Mutex::new(fingerprint(&root)));
    for stream in listener.incoming() {
        let stream = stream.map_err(io_error)?;
        let root = root.clone();
        let revision = Arc::clone(&revision);
        std::thread::spawn(move || {
            if let Err(error) = handle(stream, &root, viewer, logger, &revision) {
                logger.log(Verbosity::Error, error.to_string());
            }
        });
    }
    Ok(0)
}
fn io_error(e: impl std::fmt::Display) -> Error {
    Error::new("IOError", e.to_string())
}
fn fingerprint(root: &Path) -> String {
    let path = root.join("report.json");
    let stamp = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let body = std::fs::read(&path).unwrap_or_default();
    format!(
        "{stamp}:{:?}",
        ring::digest::digest(&ring::digest::SHA256, &body)
    )
}

fn handle(
    mut stream: TcpStream,
    root: &Path,
    viewer: bool,
    logger: Logger,
    revision: &Mutex<String>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(io_error)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(io_error)?);
    let mut first = String::new();
    reader.read_line(&mut first).map_err(io_error)?;
    if first.is_empty() {
        return Ok(());
    }
    let mut reply = |status, mime: &str, body: &[u8], head, headers: &[(&str, String)]| {
        access_log(&stream, &first, logger, status)?;
        respond(&mut stream, status, mime, body, head, headers)
    };
    let parts: Vec<_> = first.split_whitespace().collect();
    if parts.len() != 3 {
        return reply(400, "text/plain", b"Bad request", false, &[]);
    }
    let (method, target) = (parts[0], parts[1]);
    let mut raw_headers = String::new();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(io_error)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        raw_headers.push_str(&line.replace("\r\n", "\n"));
        if raw_headers.len() > 65_536 {
            return reply(431, "text/plain", b"Headers too large", false, &[]);
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    raw_headers.push('\n');
    let head = method == "HEAD";
    if !matches!(method, "GET" | "POST" | "HEAD") {
        return reply(501, "text/plain", b"Unsupported method", head, &[]);
    }
    if method != "HEAD" {
        logger.log(Verbosity::Verbose, "*".repeat(80));
        logger.log(Verbosity::Verbose, "<<< Request headers >>>");
        logger.log(Verbosity::Verbose, raw_headers);
    }
    if method == "POST" {
        let length = headers
            .get("content-length")
            .map(|s| s.parse::<usize>())
            .transpose()
            .map_err(io_error)?
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).map_err(io_error)?;
        let ct = headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("text/plain");
        let charset = ct
            .split(';')
            .skip(1)
            .find_map(|p| p.trim().strip_prefix("charset="))
            .unwrap_or("utf-8")
            .trim_matches(['\'', '"']);
        let body = encoding::decode(&body, charset, Errors::Strict)?;
        let (label, value) = match ct.split(';').next().unwrap().trim() {
            "application/json" => ("json", repr(&jumeaux::compat::json::parse(&body)?)),
            "application/x-www-form-urlencoded" => {
                let url =
                    reqwest::Url::parse(&format!("http://localhost/?{body}")).map_err(io_error)?;
                let mut form = serde_json::Map::new();
                for (key, value) in url.query_pairs() {
                    form.entry(key.into_owned())
                        .or_insert_with(|| serde_json::json!([]))
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!(value));
                }
                (
                    "x-www-form-urlencoded",
                    repr(&serde_json::Value::Object(form)),
                )
            }
            _ => ("plain string", body),
        };
        logger.log(Verbosity::Verbose, format!("<<< Parse as {label}.. >>>"));
        logger.log(Verbosity::Verbose, value);
    }
    let target_path = target.split(['?', '#']).next().unwrap();
    if viewer && target_path == "/__jumeaux_reload" {
        let current = fingerprint(root);
        let mut previous = revision.lock().map_err(io_error)?;
        if *previous != current {
            logger.log(Verbosity::Normal, "Reload viewer.");
            *previous = current.clone();
        }
        return reply(
            200,
            "text/plain",
            current.as_bytes(),
            head,
            &[("Cache-Control", "no-store".into())],
        );
    }
    let decoded = decode_path(target_path);
    let mut path = root.to_path_buf();
    let mut relative = PathBuf::new();
    for component in Path::new(&decoded).components() {
        match component {
            std::path::Component::Normal(name) => relative.push(name),
            std::path::Component::ParentDir => {
                relative.pop();
            }
            _ => (),
        }
    }
    path.push(relative);
    if path.is_dir() {
        if !target_path.ends_with('/') {
            let location = if let Some((p, q)) = target.split_once('?') {
                format!("{p}/?{q}")
            } else {
                format!("{target}/")
            };
            return reply(301, "text/html", b"", head, &[("Location", location)]);
        }
        if path.join("index.html").is_file() {
            path.push("index.html");
        } else if path.join("index.htm").is_file() {
            path.push("index.htm");
        } else {
            let mut entries = std::fs::read_dir(&path)
                .map_err(io_error)?
                .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(io_error)?;
            entries.sort_by_key(|s| s.to_lowercase());
            let mut html = format!(
                "<!DOCTYPE HTML><html><head><meta charset=\"utf-8\"><title>Directory listing for {}</title></head><body><h1>Directory listing for {}</h1><hr><ul>",
                escape(&decoded),
                escape(&decoded)
            );
            for name in entries {
                let suffix = if path.join(&name).is_dir() { "/" } else { "" };
                let href: String = name
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect();
                html.push_str(&format!(
                    "<li><a href=\"{href}{suffix}\">{}{suffix}</a></li>",
                    escape(&name)
                ));
            }
            html.push_str("</ul><hr></body></html>");
            return reply(200, "text/html; charset=utf-8", html.as_bytes(), head, &[]);
        }
    }
    let mut body=match std::fs::read(&path){Ok(body)=>body,Err(_)=>return reply(404,"text/html; charset=utf-8",b"<!DOCTYPE HTML><html><title>Error response</title><h1>Error response</h1><p>Error code: 404</p><p>Message: File not found.</p></html>",head,&[])};
    // Pythonの明示的な圧縮拡張子と、既存のJavaScript MIMEを優先する。
    let guessed = mime_guess::from_path(&path).first_or_octet_stream();
    let mime = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs") => "text/javascript",
        Some("gz") => "application/gzip",
        Some("z") => "application/octet-stream",
        Some("bz2") => "application/x-bzip2",
        Some("xz") => "application/x-xz",
        _ => guessed.essence_str(),
    };
    if viewer && mime == "text/html" {
        body.extend_from_slice(RELOAD.as_bytes());
    }
    let mut extra = vec![];
    if let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) {
        let at: chrono::DateTime<chrono::Utc> = modified.into();
        extra.push((
            "Last-Modified",
            at.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        ));
    }
    if viewer {
        extra.push(("Cache-Control", "no-cache".into()));
    }
    reply(200, mime, &body, head, &extra)
}
fn access_log(stream: &TcpStream, request: &str, logger: Logger, status: u16) -> Result<()> {
    let prefix = format!(
        "{} - - [{}]",
        stream.peer_addr().map_err(io_error)?.ip(),
        chrono::Local::now().format("%d/%b/%Y %H:%M:%S")
    );
    let message = match status {
        404 => Some("File not found".to_owned()),
        501 => Some(format!(
            "Unsupported method ('{}')",
            request.split_whitespace().next().unwrap_or("")
        )),
        400 => Some("Bad request".to_owned()),
        431 => Some("Headers too large".to_owned()),
        _ => None,
    };
    if let Some(message) = message {
        logger.log(
            Verbosity::Normal,
            format!("{prefix} code {status}, message {message}"),
        );
    }
    logger.log(
        Verbosity::Normal,
        format!("{prefix} \"{}\" {status} -", request.trim_end()),
    );
    Ok(())
}
fn respond(
    stream: &mut TcpStream,
    status: u16,
    mime: &str,
    body: &[u8],
    head: bool,
    headers: &[(&str, String)],
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        301 => "Moved Permanently",
        400 => "Bad Request",
        404 => "Not Found",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Error",
    };
    write!(stream,"HTTP/1.0 {status} {reason}\r\nServer: Jumeaux/{}\r\nDate: {}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n",env!("CARGO_PKG_VERSION"),chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT"),body.len()).map_err(io_error)?;
    for (key, value) in headers {
        write!(stream, "{key}: {value}\r\n").map_err(io_error)?;
    }
    stream.write_all(b"\r\n").map_err(io_error)?;
    if !head {
        stream.write_all(body).map_err(io_error)?;
    }
    Ok(())
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn decode_path(path: &str) -> String {
    let mut bytes = vec![];
    let mut input = path.as_bytes().iter().copied().peekable();
    while let Some(b) = input.next() {
        if b == b'%' {
            let mut peek = input.clone();
            if let (Some(a), Some(b)) = (peek.next(), peek.next())
                && let (Some(a), Some(b)) = ((a as char).to_digit(16), (b as char).to_digit(16))
            {
                bytes.push((a * 16 + b) as u8);
                input = peek;
                continue;
            }
        }
        bytes.push(b);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
