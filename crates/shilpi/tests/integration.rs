//! End-to-end test: spawn a real `shilpid`, then drive it through the real
//! `shilpi` binary in `-e` (non-interactive) mode, exactly as a user's
//! terminal invocation would. This is on top of `shilpid`'s own integration
//! test (which exercises the wire protocol directly) — this one exercises
//! the command parser and output formatting in `crates/shilpi/src/command.rs`
//! against a live server.

use std::io::{BufRead, BufReader};
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

/// `shilpid` lives in a sibling crate; both binaries land in the same
/// target/<profile>/ directory, so locate it relative to our own exe path.
/// Only present when the whole workspace was built (e.g. `cargo test
/// --workspace`) — a lone `cargo test -p shilpi` won't have built it.
fn shilpid_exe() -> Option<PathBuf> {
    let shilpi_exe = PathBuf::from(env!("CARGO_BIN_EXE_shilpi"));
    let name = if cfg!(windows) {
        "shilpid.exe"
    } else {
        "shilpid"
    };
    let candidate = shilpi_exe.with_file_name(name);
    candidate.exists().then_some(candidate)
}

fn spawn_shilpid(exe: &PathBuf, data_path: &std::path::Path) -> (ChildGuard, String) {
    let mut child = Command::new(exe)
        .args(["--bind", "127.0.0.1:0", "--data"])
        .arg(data_path)
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
    (ChildGuard(child), addr)
}

/// Run `shilpi --host <addr> -e <cmd>`, waiting briefly for the connection
/// to become acceptable (mirrors the retry `shilpid`'s own test does).
fn shilpi_eval(addr: &str, cmd: &str) -> String {
    let mut last_err = String::new();
    for _ in 0..50 {
        let output = Command::new(env!("CARGO_BIN_EXE_shilpi"))
            .args(["--host", addr, "-e", cmd])
            .output()
            .expect("failed to spawn shilpi");
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        last_err = String::from_utf8_lossy(&output.stderr).to_string();
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("shilpi -e '{cmd}' kept failing against {addr}: {last_err}");
}

#[test]
fn shell_drives_a_live_server() {
    let Some(shilpid_exe) = shilpid_exe() else {
        eprintln!("skipping: shilpid binary not built (run `cargo test --workspace`)");
        return;
    };

    let data_path = std::env::temp_dir().join(format!(
        "shilpi-shell-integration-{}-{:?}.vdb",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&data_path);

    let (_guard, addr) = spawn_shilpid(&shilpid_exe, &data_path);

    assert_eq!(shilpi_eval(&addr, "ping"), "pong");
    assert_eq!(shilpi_eval(&addr, "insert 1 0 0 1 1 hello world"), "ok");
    assert!(shilpi_eval(&addr, "get 1").contains("hello world"));
    assert_eq!(shilpi_eval(&addr, "query bbox -1 -1 2 2"), "1");
    assert_eq!(
        shilpi_eval(&addr, "query bbox 100 100 200 200"),
        "(no matches)"
    );
    assert!(shilpi_eval(&addr, "stats").starts_with("1 record"));
    assert_eq!(shilpi_eval(&addr, "delete 1"), "deleted");
    assert_eq!(shilpi_eval(&addr, "delete 1"), "not found");

    let _ = std::fs::remove_file(&data_path);
}
