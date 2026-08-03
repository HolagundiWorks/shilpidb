//! End-to-end tests of the typed `shilpi-client` API against a real `shilpid`
//! process — the same surface a programmatic host (e.g. AAD) uses. The unit
//! tests in `src/lib.rs` cover response mapping with a fake stream; these
//! prove the whole path over a real socket, including server-side errors.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use shilpi_client::{Client, Error};
use shilpidb::Bbox;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Locate a workspace binary next to the test runner. A *library* crate's
/// integration tests don't get `CARGO_BIN_EXE_*`, so walk up from our own test
/// executable: `target/<profile>/deps/<test>` → `target/<profile>/<bin>`.
/// Returns `None` when the binary wasn't built (e.g. `cargo test -p
/// shilpi-client` without `--workspace`), so the caller can skip.
fn workspace_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let candidate = exe.parent()?.parent()?.join(file);
    candidate.exists().then_some(candidate)
}

/// Spawn `shilpid` on an ephemeral port with a fresh data file. Returns `None`
/// only when the binary isn't built (skip); any other failure panics.
fn spawn_shilpid(extra_args: &[&str]) -> Option<(ChildGuard, String, PathBuf)> {
    let exe = workspace_bin("shilpid")?;
    let data_path = std::env::temp_dir().join(format!(
        "shilpi-client-it-{}-{:?}.vdb",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&data_path);

    let mut child = Command::new(&exe)
        .args(["--bind", "127.0.0.1:0", "--data"])
        .arg(&data_path)
        .args(extra_args)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn shilpid");

    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    let addr = loop {
        let line = lines
            .next()
            .expect("shilpid exited before printing its bound address")
            .expect("failed reading shilpid stderr");
        if let Some(addr) = line.strip_prefix("shilpid listening on ") {
            break addr.to_string();
        }
    };
    Some((ChildGuard(child), addr, data_path))
}

/// Connect, retrying briefly while the accept loop comes up.
fn connect(addr: &str) -> Client {
    for _ in 0..50 {
        if let Ok(c) = Client::connect(addr) {
            return c;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("could not connect to {addr}");
}

fn bb(x0: f64, y0: f64, x1: f64, y1: f64) -> Bbox {
    Bbox::from_corners(x0, y0, x1, y1)
}

#[test]
fn typed_api_round_trips_against_a_live_server() {
    let Some((_guard, addr, data_path)) = spawn_shilpid(&[]) else {
        eprintln!("skipping: shilpid binary not built (run `cargo test --workspace`)");
        return;
    };
    let mut db = connect(&addr);

    db.ping().unwrap();
    assert!(db.get(1).unwrap().is_none());

    db.insert(1, bb(0.0, 0.0, 1.0, 1.0), b"wall".to_vec())
        .unwrap();
    let rec = db.get(1).unwrap().expect("record 1 exists after insert");
    assert_eq!(rec.payload, b"wall");
    assert_eq!(rec.bbox, bb(0.0, 0.0, 1.0, 1.0));

    assert_eq!(db.query_bbox(bb(-1.0, -1.0, 2.0, 2.0)).unwrap(), vec![1]);
    assert!(db
        .query_bbox(bb(100.0, 100.0, 200.0, 200.0))
        .unwrap()
        .is_empty());
    assert_eq!(db.query_point(0.5, 0.5, 0.1).unwrap(), vec![1]);

    db.insert(2, bb(10.0, 10.0, 11.0, 11.0), b"door".to_vec())
        .unwrap();
    assert_eq!(db.list().unwrap(), vec![1, 2]);
    assert_eq!(db.list_boxes().unwrap().len(), 2);
    assert_eq!(db.stats().unwrap().count, 2);

    db.save().unwrap();
    assert!(db.delete(1).unwrap());
    assert!(!db.delete(1).unwrap());

    let _ = std::fs::remove_file(&data_path);
}

#[test]
fn read_only_server_rejects_writes() {
    let Some((_guard, addr, data_path)) = spawn_shilpid(&["--read-only"]) else {
        eprintln!("skipping: shilpid binary not built (run `cargo test --workspace`)");
        return;
    };
    let mut db = connect(&addr);

    db.ping().unwrap();
    match db.insert(1, bb(0.0, 0.0, 1.0, 1.0), vec![]) {
        Err(Error::Server(msg)) => assert!(
            msg.contains("read-only"),
            "expected a read-only message, got: {msg}"
        ),
        other => panic!("expected Error::Server, got {other:?}"),
    }
    // Reads still succeed on a read-only server.
    assert!(db.list().unwrap().is_empty());

    let _ = std::fs::remove_file(&data_path);
}
