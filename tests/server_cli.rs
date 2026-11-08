use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
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
    fn start(viewer: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "jumeaux-server-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir_all(root.join("responses/first")).unwrap();
        std::fs::write(
            root.join("responses/first/index.html"),
            "<!doctype html><title>Viewer</title>",
        )
        .unwrap();
        std::fs::write(
            root.join("responses/first/report.json"),
            "{\"key\":\"first\"}",
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("first", root.join("responses/latest")).unwrap();
        std::fs::write(root.join("日本語.json"), "{\"ok\":true}").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut command = Command::new(env!("CARGO_BIN_EXE_jumeaux"));
        command
            .current_dir(&root)
            .args([
                if viewer { "viewer" } else { "server" },
                "--port",
                &port.to_string(),
            ])
            .env("JUMEAUX_NO_BROWSER", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if !viewer {
            command.arg("-v");
        }
        let mut child = command.spawn().unwrap();
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let mut first = String::new();
        stderr.read_line(&mut first).unwrap();
        assert!(first.starts_with("Serving HTTP on"), "{first}");
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
    let s = Server::start(false);
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

#[cfg(unix)]
#[test]
fn viewer_tracks_report_changes_and_latest_replacement_without_browser() {
    let s = Server::start(true);
    let html = s.request("GET", "/", "", "text/plain");
    assert!(html.contains("<title>Viewer</title>"));
    assert!(html.contains("/__jumeaux_reload"));
    assert!(html.contains("location.reload()"));
    let first = s
        .request("GET", "/__jumeaux_reload", "", "text/plain")
        .split("\r\n\r\n")
        .nth(1)
        .unwrap()
        .to_owned();
    std::fs::write(
        s.root.join("responses/first/report.json"),
        "{\"key\":\"changed\"}",
    )
    .unwrap();
    let second = s.request("GET", "/__jumeaux_reload", "", "text/plain");
    assert_ne!(second.split("\r\n\r\n").nth(1).unwrap(), first);
    std::fs::create_dir(s.root.join("responses/next")).unwrap();
    std::fs::write(
        s.root.join("responses/next/report.json"),
        "{\"key\":\"next\"}",
    )
    .unwrap();
    std::fs::remove_file(s.root.join("responses/latest")).unwrap();
    std::os::unix::fs::symlink("next", s.root.join("responses/latest")).unwrap();
    assert!(
        s.request("GET", "/report.json", "", "text/plain")
            .ends_with("{\"key\":\"next\"}")
    );
    let third = s.request("GET", "/__jumeaux_reload", "", "text/plain");
    assert_ne!(
        third.split("\r\n\r\n").nth(1),
        second.split("\r\n\r\n").nth(1)
    );
}
