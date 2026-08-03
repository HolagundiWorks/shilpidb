//! End-to-end test: spawn the real `shilpid` binary, talk to it over TCP with
//! `shilpi-protocol` exactly like a client would, and confirm inserts,
//! queries, and a save/reload cycle all work through the actual process
//! boundary — not just in-process against `Database` directly.

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use shilpi_protocol::{recv_response, send_request, Request, Response};
use shilpidb::Bbox;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn `shilpid` on an OS-assigned port and return (guard, bound address).
fn spawn_shilpid(data_path: &std::path::Path) -> (ChildGuard, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shilpid"))
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

fn connect_with_retry(addr: &str) -> TcpStream {
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect(addr) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("could not connect to shilpid at {addr}");
}

fn roundtrip(stream: &mut TcpStream, req: Request) -> Response {
    send_request(stream, &req).expect("send_request failed");
    recv_response(stream)
        .expect("recv_response failed")
        .expect("connection closed unexpectedly")
}

#[test]
fn insert_query_and_persist_across_restart() {
    let data_path = std::env::temp_dir().join(format!(
        "shilpid-integration-{}-{:?}.vdb",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&data_path);

    // --- first run: insert, query, save ---
    {
        let (_guard, addr) = spawn_shilpid(&data_path);
        let mut stream = connect_with_retry(&addr);

        assert_eq!(roundtrip(&mut stream, Request::Ping), Response::Pong);

        assert_eq!(
            roundtrip(
                &mut stream,
                Request::Insert {
                    id: 1,
                    bbox: Bbox::from_corners(0.0, 0.0, 1.0, 1.0),
                    payload: b"alpha".to_vec(),
                },
            ),
            Response::Ok
        );
        assert_eq!(
            roundtrip(
                &mut stream,
                Request::Insert {
                    id: 2,
                    bbox: Bbox::from_corners(10.0, 10.0, 11.0, 11.0),
                    payload: b"beta".to_vec(),
                },
            ),
            Response::Ok
        );

        assert_eq!(
            roundtrip(&mut stream, Request::Get { id: 1 }),
            Response::Found {
                bbox: Bbox::from_corners(0.0, 0.0, 1.0, 1.0),
                payload: b"alpha".to_vec(),
            }
        );
        assert_eq!(
            roundtrip(&mut stream, Request::Get { id: 99 }),
            Response::NotFound
        );

        assert_eq!(
            roundtrip(
                &mut stream,
                Request::QueryBbox {
                    rect: Bbox::from_corners(-1.0, -1.0, 2.0, 2.0),
                },
            ),
            Response::Ids(vec![1])
        );

        match roundtrip(&mut stream, Request::Stats) {
            Response::Stats { count, cell_size } => {
                assert_eq!(count, 2);
                assert!(cell_size > 0.0);
            }
            other => panic!("expected Stats, got {other:?}"),
        }

        assert_eq!(roundtrip(&mut stream, Request::Save), Response::Ok);
    }

    // --- second run: same data file, fresh process, data must survive ---
    {
        let (_guard, addr) = spawn_shilpid(&data_path);
        let mut stream = connect_with_retry(&addr);

        let mut ids = match roundtrip(&mut stream, Request::List) {
            Response::Ids(ids) => ids,
            other => panic!("expected Ids, got {other:?}"),
        };
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);

        assert_eq!(
            roundtrip(&mut stream, Request::Get { id: 2 }),
            Response::Found {
                bbox: Bbox::from_corners(10.0, 10.0, 11.0, 11.0),
                payload: b"beta".to_vec(),
            }
        );

        assert_eq!(
            roundtrip(&mut stream, Request::Delete { id: 1 }),
            Response::Deleted(true)
        );
        assert_eq!(
            roundtrip(&mut stream, Request::Delete { id: 1 }),
            Response::Deleted(false)
        );
    }

    let _ = std::fs::remove_file(&data_path);
}
