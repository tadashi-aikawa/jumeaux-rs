use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Server {
    child: Child,
    root: PathBuf,
    port: u16,
    logs: std::sync::mpsc::Receiver<String>,
}
impl Server {
    fn start() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "jumeaux-server-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("responses")).unwrap();
        std::fs::write(root.join("日本語.json"), "{\"ok\":true}").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_jumeaux"));
        command
            .current_dir(&root)
            .args(["server", "--port", "0", "-v"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let mut first = String::new();
        stderr.read_line(&mut first).unwrap();
        assert!(first.starts_with("Serving HTTP on"), "{first}");
        let port = first
            .split(" port ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // ログpipeを詰まらせず、各行を受け取れる状態にする。
        let (tx, logs) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in stderr.lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            root,
            port,
            logs,
        }
    }
    fn request(&self, method: &str, path: &str, body: &str, content_type: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(stream,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
    fn expect_log_suffix(&self, suffix: &str) {
        loop {
            let line = self
                .logs
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            if line.ends_with(suffix) {
                return;
            }
        }
    }
}

#[test]
fn server_logs_all_statuses_and_guesses_mime_types() {
    let s = Server::start();
    for (method, path, status, error) in [
        (
            "GET",
            "/missing",
            404,
            Some("code 404, message File not found"),
        ),
        (
            "HEAD",
            "/missing",
            404,
            Some("code 404, message File not found"),
        ),
        ("GET", "/responses", 301, None),
        ("GET", "/", 200, None),
        (
            "PUT",
            "/",
            501,
            Some("code 501, message Unsupported method ('PUT')"),
        ),
    ] {
        let response = s.request(method, path, "", "text/plain");
        assert!(
            response.starts_with(&format!("HTTP/1.0 {status}")),
            "{response}"
        );
        if let Some(error) = error {
            s.expect_log_suffix(error);
        }
        s.expect_log_suffix(&format!("\"{method} {path} HTTP/1.1\" {status} -"));
    }
    for (extension, mime) in [
        ("html", "text/html"),
        ("gif", "image/gif"),
        ("pdf", "application/pdf"),
        ("wasm", "application/wasm"),
        ("woff2", "font/woff2"),
        ("ico", "image/x-icon"),
        ("mp4", "video/mp4"),
        ("ZIP", "application/zip"),
        ("gz", "application/gzip"),
        ("bz2", "application/x-bzip2"),
        ("xz", "application/x-xz"),
        ("js", "text/javascript"),
        ("yml", "application/yaml"),
        ("yaml", "application/yaml"),
        ("YML", "application/yaml"),
        ("YAML", "application/yaml"),
        ("unknown-extension", "application/octet-stream"),
    ] {
        let name = format!("file.{extension}");
        std::fs::write(s.root.join(&name), "body").unwrap();
        let response = s.request("GET", &format!("/{name}"), "", "text/plain");
        assert!(
            response.contains(&format!("Content-Type: {mime}\r\n")),
            "{extension}: {response}"
        );
        assert!(response.ends_with("\r\n\r\nbody"), "{response}");
        s.expect_log_suffix(&format!("\"GET /{name} HTTP/1.1\" 200 -"));
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn static_server_supports_get_head_post_and_paths() {
    let s = Server::start();
    let path = "/%E6%97%A5%E6%9C%AC%E8%AA%9E.json";
    for (method, body, ct) in [
        ("GET", "", "text/plain"),
        ("POST", "{\"a\":1}", "application/json"),
        ("POST", "a=1&a=2&b=", "application/x-www-form-urlencoded"),
        ("POST", "日本語", "text/plain; charset=utf-8"),
    ] {
        let response = s.request(method, path, body, ct);
        assert!(response.starts_with("HTTP/1.0 200 OK"), "{response}");
        assert!(response.contains("Content-Type: application/json\r\n"));
        assert!(response.ends_with("{\"ok\":true}"));
    }
    let head = s.request("HEAD", path, "", "text/plain");
    assert!(head.ends_with("\r\n\r\n"));
    assert!(
        s.request("GET", "/missing", "", "text/plain")
            .starts_with("HTTP/1.0 404")
    );
    assert!(
        s.request("GET", "/responses", "", "text/plain")
            .contains("Location: /responses/\r\n")
    );
    assert!(
        s.request("GET", "/", "", "text/plain")
            .contains("日本語.json")
    );
    for expected in [
        "<<< Parse as json.. >>>",
        "{'a': 1}",
        "<<< Parse as x-www-form-urlencoded.. >>>",
        "{'a': ['1', '2'], 'b': ['']}",
        "<<< Parse as plain string.. >>>",
        "日本語",
    ] {
        loop {
            let line = s
                .logs
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            if line == expected {
                break;
            }
        }
    }
}
