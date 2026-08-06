//! # shilpi-http — a dependency-free HTTP/JSON gateway for `shilpid`
//!
//! `shilpid` speaks a compact binary [`shilpi_protocol`] wire format with a
//! Rust-only client. That's ideal for Rust hosts, but leaves non-Rust
//! consumers — notably the AORMS TypeScript/Python stack — with no way in.
//! This gateway bridges that gap: a small HTTP/1.1 + JSON server that fronts a
//! `shilpid` backend over [`shilpi_client`], so any language with an HTTP
//! client can read and write the spatial store.
//!
//! Like the rest of ShilpiDB it is **std-only** — the HTTP parsing
//! ([`http`]) and JSON emission ([`json`]) are hand-rolled rather than pulling
//! in a web framework.
//!
//! ## Endpoints
//!
//! | Method + path | Purpose |
//! |---|---|
//! | `GET /health` | Ping the backend |
//! | `GET /stats` | Record count + grid cell size |
//! | `GET /records` | List all ids + boxes (no payloads) |
//! | `GET /records/{id}` | Fetch one record (bbox + payload as hex/text) |
//! | `PUT /records/{id}?minX=&minY=&maxX=&maxY=` | Insert/replace; body = payload bytes |
//! | `DELETE /records/{id}` | Delete one record |
//! | `GET /query/bbox?minX=&minY=&maxX=&maxY=` | Ids intersecting a window |
//! | `GET /query/point?x=&y=&tol=` | Ids near a point (`tol` optional) |
//! | `POST /save` | Flush the backend to its data file |

mod http;
mod json;
mod router;

use std::io::{self, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use shilpi_client::{Client, Record, Stats};
use shilpidb::Bbox;

pub use http::{Request, Response};
pub use router::route;

/// Per-connection read/write timeout, so a slow or stuck HTTP client can't tie
/// up a worker thread forever.
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// The spatial-store operations the router needs. Abstracted as a trait so the
/// router can be unit-tested against an in-memory fake, and so an alternative
/// backend (e.g. an embedded engine) could be dropped in later.
pub trait Backend {
    fn ping(&mut self) -> Result<(), String>;
    fn insert(&mut self, id: u64, bbox: Bbox, payload: Vec<u8>) -> Result<(), String>;
    fn get(&mut self, id: u64) -> Result<Option<Record>, String>;
    fn delete(&mut self, id: u64) -> Result<bool, String>;
    fn query_bbox(&mut self, rect: Bbox) -> Result<Vec<u64>, String>;
    fn query_point(&mut self, x: f64, y: f64, tol: f64) -> Result<Vec<u64>, String>;
    fn list_boxes(&mut self) -> Result<Vec<(u64, Bbox)>, String>;
    fn stats(&mut self) -> Result<Stats, String>;
    fn save(&mut self) -> Result<(), String>;
}

/// Bridge the typed [`shilpi_client::Client`] onto [`Backend`], folding its
/// error enum into a display string the router turns into a `502`.
impl<S: Read + Write> Backend for Client<S> {
    fn ping(&mut self) -> Result<(), String> {
        Client::ping(self).map_err(|e| e.to_string())
    }
    fn insert(&mut self, id: u64, bbox: Bbox, payload: Vec<u8>) -> Result<(), String> {
        Client::insert(self, id, bbox, payload).map_err(|e| e.to_string())
    }
    fn get(&mut self, id: u64) -> Result<Option<Record>, String> {
        Client::get(self, id).map_err(|e| e.to_string())
    }
    fn delete(&mut self, id: u64) -> Result<bool, String> {
        Client::delete(self, id).map_err(|e| e.to_string())
    }
    fn query_bbox(&mut self, rect: Bbox) -> Result<Vec<u64>, String> {
        Client::query_bbox(self, rect).map_err(|e| e.to_string())
    }
    fn query_point(&mut self, x: f64, y: f64, tol: f64) -> Result<Vec<u64>, String> {
        Client::query_point(self, x, y, tol).map_err(|e| e.to_string())
    }
    fn list_boxes(&mut self) -> Result<Vec<(u64, Bbox)>, String> {
        Client::list_boxes(self).map_err(|e| e.to_string())
    }
    fn stats(&mut self) -> Result<Stats, String> {
        Client::stats(self).map_err(|e| e.to_string())
    }
    fn save(&mut self) -> Result<(), String> {
        Client::save(self).map_err(|e| e.to_string())
    }
}

/// Bind an HTTP listener on `bind` and serve requests by proxying to the
/// `shilpid` backend at `backend_addr`, one thread per connection. Runs until
/// the listener errors.
///
/// The bound address is printed to stderr as `shilpi-http listening on <addr>`
/// (regardless of anything else) so tooling and tests can discover an
/// OS-assigned port when binding to `:0`.
pub fn serve(bind: &str, backend_addr: &str) -> io::Result<()> {
    let listener = TcpListener::bind(bind)?;
    eprintln!(
        "shilpi-http listening on {} (backend {backend_addr})",
        listener.local_addr()?
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let backend_addr = backend_addr.to_string();
                thread::spawn(move || {
                    // A broken client connection isn't fatal to the server.
                    let _ = handle_connection(stream, &backend_addr);
                });
            }
            Err(e) => eprintln!("shilpi-http: accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_connection(stream: TcpStream, backend_addr: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let req = match Request::read(&mut reader) {
        Ok(Some(req)) => req,
        Ok(None) => return Ok(()), // client closed without sending a request
        Err(e) => {
            // Malformed request: tell the client, then close.
            return Response::error(400, &e.to_string()).write(&mut writer);
        }
    };

    // A fresh backend connection per HTTP connection keeps the gateway
    // stateless; `Connection: close` means one request per connection.
    let resp = match Client::connect(backend_addr) {
        Ok(mut client) => route(&req, &mut client),
        Err(e) => Response::error(502, &format!("cannot reach backend {backend_addr}: {e}")),
    };
    resp.write(&mut writer)
}
