//! End-to-end test: spawn a real `shilpid`, put a real `shilpi-http` gateway in
//! front of it, and drive the gateway over actual HTTP — exercising the HTTP
//! parsing, routing, JSON emission, and the backend proxy hop together, across
//! the real process boundary (mirroring `shilpid`'s own integration test).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `shilpid` is a sibling crate; its binary lands in the same target dir as
/// ours. Only present when the whole workspace was built (`cargo test
/// --workspace`), so a lone `cargo test -p shilpi-http` skips the test.
fn shilpid_exe() -> Option<PathBuf> {
    let me = PathBuf::from(env!("CARGO_BIN_EXE_shilpi-http"));
    let name = if cfg!(windows) {
        "shilpid.exe"
    } else {
        "shilpid"
    };
    let candidate = me.with_file_name(name);
    candidate.exists().then_some(candidate)
}

fn wait_for_listen(child: &mut Child, prefix: &str) -> String {
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    loop {
        let line = lines
            .next()
            .expect("process exited before printing its bound address")
            .expect("failed reading process stderr");
        if let Some(rest) = line.strip_prefix(prefix) {
            // rest is like "127.0.0.1:PORT" or "127.0.0.1:PORT (backend ...)".
            return rest
                .split_whitespace()
                .next()
                .expect("no address after prefix")
                .to_string();
        }
    }
}

fn spawn_shilpid(exe: &PathBuf, data_path: &std::path::Path) -> (ChildGuard, String) {
    let mut child = Command::new(exe)
        .args(["--bind", "127.0.0.1:0", "--data"])
        .arg(data_path)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn shilpid");
    let addr = wait_for_listen(&mut child, "shilpid listening on ");
    (ChildGuard(child), addr)
}

fn spawn_gateway(backend: &str) -> (ChildGuard, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shilpi-http"))
        .args(["--bind", "127.0.0.1:0", "--backend", backend])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn shilpi-http");
    let addr = wait_for_listen(&mut child, "shilpi-http listening on ");
    (ChildGuard(child), addr)
}

/// Send one HTTP request and read the whole response (the gateway replies
/// `Connection: close`, so read-to-end gets the full body). Returns
/// `(status, body)`.
fn http(addr: &str, method: &str, path: &str, body: Option<&[u8]>) -> (u16, String) {
    let mut last_err = String::new();
    for _ in 0..50 {
        match TcpStream::connect(addr) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
                if let Some(b) = body {
                    req.push_str(&format!("Content-Length: {}\r\n", b.len()));
                }
                req.push_str("\r\n");
                let mut bytes = req.into_bytes();
                if let Some(b) = body {
                    bytes.extend_from_slice(b);
                }
                stream.write_all(&bytes).unwrap();
                stream.flush().unwrap();

                let mut raw = Vec::new();
                stream.read_to_end(&mut raw).unwrap();
                let text = String::from_utf8_lossy(&raw).into_owned();
                let status = text
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(0);
                let body = match text.split_once("\r\n\r\n") {
                    Some((_, b)) => b.to_string(),
                    None => String::new(),
                };
                return (status, body);
            }
            Err(e) => {
                last_err = e.to_string();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    panic!("could not connect to gateway at {addr}: {last_err}");
}

#[test]
fn gateway_proxies_the_full_record_lifecycle() {
    let Some(shilpid_exe) = shilpid_exe() else {
        eprintln!("skipping: shilpid binary not built (run `cargo test --workspace`)");
        return;
    };

    let data_path = std::env::temp_dir().join(format!(
        "shilpi-http-integration-{}-{:?}.vdb",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&data_path);

    let (_shilpid, backend) = spawn_shilpid(&shilpid_exe, &data_path);
    let (_gateway, addr) = spawn_gateway(&backend);

    // Health check hops all the way to shilpid and back.
    let (status, body) = http(&addr, "GET", "/health", None);
    assert_eq!(status, 200, "health body: {body}");
    assert!(body.contains("\"status\":\"ok\""));

    // Insert two records; payload is the raw request body.
    let (status, _) = http(
        &addr,
        "PUT",
        "/records/1?minX=0&minY=0&maxX=10&maxY=10",
        Some(b"wall"),
    );
    assert_eq!(status, 200);
    let (status, _) = http(
        &addr,
        "PUT",
        "/records/2?minX=50&minY=50&maxX=60&maxY=60",
        Some(b"window"),
    );
    assert_eq!(status, 200);

    // Fetch one back — bbox + payload as text and hex.
    let (status, body) = http(&addr, "GET", "/records/1", None);
    assert_eq!(status, 200);
    assert!(body.contains("\"payloadText\":\"wall\""), "body: {body}");
    assert!(body.contains("\"payloadHex\":\"77616c6c\""), "body: {body}");
    assert!(body.contains("\"minX\":0"), "body: {body}");

    // Spatial query returns only the overlapping id.
    let (status, body) = http(
        &addr,
        "GET",
        "/query/bbox?minX=1&minY=1&maxX=2&maxY=2",
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(body, "{\"ids\":[1]}");

    // Point query near the second record.
    let (_, body) = http(&addr, "GET", "/query/point?x=55&y=55&tol=1", None);
    assert_eq!(body, "{\"ids\":[2]}");

    // Stats reflect two records.
    let (_, body) = http(&addr, "GET", "/stats", None);
    assert!(body.contains("\"count\":2"), "body: {body}");

    // List all boxes.
    let (_, body) = http(&addr, "GET", "/records", None);
    assert!(
        body.contains("\"id\":1") && body.contains("\"id\":2"),
        "body: {body}"
    );

    // Persist, then delete.
    let (status, body) = http(&addr, "POST", "/save", None);
    assert_eq!(status, 200, "save body: {body}");
    let (_, body) = http(&addr, "DELETE", "/records/1", None);
    assert_eq!(body, "{\"deleted\":true}");
    let (_, body) = http(&addr, "DELETE", "/records/1", None);
    assert_eq!(body, "{\"deleted\":false}");

    // Error paths.
    let (status, _) = http(&addr, "GET", "/records/99", None);
    assert_eq!(status, 404);
    let (status, _) = http(&addr, "GET", "/query/bbox?minX=0", None);
    assert_eq!(status, 400);
    let (status, _) = http(&addr, "GET", "/nope", None);
    assert_eq!(status, 404);

    let _ = std::fs::remove_file(&data_path);
}
