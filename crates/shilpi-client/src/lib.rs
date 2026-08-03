//! # shilpi-client — a small blocking client for `shilpid`
//!
//! Wraps a TCP connection (or any `Read + Write` stream) speaking
//! [`shilpi_protocol`] into typed operations, so a host doesn't hand-roll the
//! request/response plumbing the way the shell and GUI used to. Everything is
//! blocking and dependency-free (std only), matching the rest of ShilpiDB.
//!
//! ```no_run
//! use shilpi_client::Client;
//! use shilpidb::Bbox;
//!
//! let mut db = Client::connect("127.0.0.1:7420")?;
//! db.insert(1, Bbox::from_corners(0.0, 0.0, 1.0, 1.0), b"wall".to_vec())?;
//! let hits = db.query_bbox(Bbox::from_corners(-1.0, -1.0, 2.0, 2.0))?;
//! assert_eq!(hits, vec![1]);
//! # Ok::<(), shilpi_client::Error>(())
//! ```
//!
//! [`Client::request`] exposes the raw [`Request`]/[`Response`] pass-through
//! (what the `shilpi` shell uses to format any reply); the typed methods
//! ([`Client::get`], [`Client::query_bbox`], …) are the ergonomic surface for
//! programmatic hosts, mapping a server-side [`Response::Err`] to
//! [`Error::Server`].

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

use shilpi_protocol::{recv_response, send_request, Request, Response};
use shilpidb::Bbox;

/// A failure talking to `shilpid`.
#[derive(Debug)]
pub enum Error {
    /// Transport failure reading from / writing to the stream.
    Io(io::Error),
    /// The server replied with an error message (e.g. read-only, save failed).
    Server(String),
    /// The server replied with a variant that doesn't fit the request — the
    /// `&str` names the operation.
    Unexpected(&'static str, Response),
    /// The connection closed before a response arrived.
    Closed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Server(msg) => write!(f, "server error: {msg}"),
            Error::Unexpected(op, resp) => {
                write!(f, "unexpected response to {op}: {resp:?}")
            }
            Error::Closed => write!(f, "connection closed by server"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// A stored record: its bounding box and opaque payload bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub bbox: Bbox,
    pub payload: Vec<u8>,
}

/// Summary stats reported by the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub count: u32,
    pub cell_size: f64,
}

/// A connection to a `shilpid` server.
///
/// Generic over the stream so it works over a real [`TcpStream`] (via
/// [`Client::connect`]) or any in-memory `Read + Write` (via
/// [`Client::from_stream`], handy for tests).
pub struct Client<S = TcpStream> {
    stream: S,
}

impl Client<TcpStream> {
    /// Connect to `shilpid` at `addr` (e.g. `"127.0.0.1:7420"`).
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Self, Error> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Client { stream })
    }
}

impl<S: Read + Write> Client<S> {
    /// Wrap an existing stream that already speaks the protocol.
    pub fn from_stream(stream: S) -> Self {
        Client { stream }
    }

    /// Consume the client and return the underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Send a request and return the raw response, **including**
    /// [`Response::Err`]. The shell uses this to format any reply uniformly;
    /// prefer the typed methods for programmatic use.
    pub fn request(&mut self, req: &Request) -> Result<Response, Error> {
        send_request(&mut self.stream, req)?;
        recv_response(&mut self.stream)?.ok_or(Error::Closed)
    }

    /// Like [`request`](Self::request) but folds a server error response into
    /// [`Error::Server`], so typed methods only match their success variants.
    fn call(&mut self, req: Request) -> Result<Response, Error> {
        match self.request(&req)? {
            Response::Err(msg) => Err(Error::Server(msg)),
            other => Ok(other),
        }
    }

    /// Liveness check.
    pub fn ping(&mut self) -> Result<(), Error> {
        match self.call(Request::Ping)? {
            Response::Pong => Ok(()),
            other => Err(Error::Unexpected("ping", other)),
        }
    }

    /// Insert or replace the record at `id`.
    pub fn insert(&mut self, id: u64, bbox: Bbox, payload: Vec<u8>) -> Result<(), Error> {
        match self.call(Request::Insert { id, bbox, payload })? {
            Response::Ok => Ok(()),
            other => Err(Error::Unexpected("insert", other)),
        }
    }

    /// Insert or replace many records in a single round-trip — far cheaper than
    /// a `insert` per record when storing a whole drawing.
    pub fn insert_many(&mut self, records: Vec<(u64, Bbox, Vec<u8>)>) -> Result<(), Error> {
        match self.call(Request::InsertMany { records })? {
            Response::Ok => Ok(()),
            other => Err(Error::Unexpected("insert_many", other)),
        }
    }

    /// Fetch the record at `id`, or `None` if it doesn't exist.
    pub fn get(&mut self, id: u64) -> Result<Option<Record>, Error> {
        match self.call(Request::Get { id })? {
            Response::Found { bbox, payload } => Ok(Some(Record { bbox, payload })),
            Response::NotFound => Ok(None),
            other => Err(Error::Unexpected("get", other)),
        }
    }

    /// Delete the record at `id`; returns whether it existed.
    pub fn delete(&mut self, id: u64) -> Result<bool, Error> {
        match self.call(Request::Delete { id })? {
            Response::Deleted(existed) => Ok(existed),
            other => Err(Error::Unexpected("delete", other)),
        }
    }

    /// Ids of records whose box intersects `rect`.
    pub fn query_bbox(&mut self, rect: Bbox) -> Result<Vec<u64>, Error> {
        match self.call(Request::QueryBbox { rect })? {
            Response::Ids(ids) => Ok(ids),
            other => Err(Error::Unexpected("query_bbox", other)),
        }
    }

    /// Ids of records within `tol` of point `(x, y)`.
    pub fn query_point(&mut self, x: f64, y: f64, tol: f64) -> Result<Vec<u64>, Error> {
        match self.call(Request::QueryPoint { x, y, tol })? {
            Response::Ids(ids) => Ok(ids),
            other => Err(Error::Unexpected("query_point", other)),
        }
    }

    /// All record ids currently stored.
    pub fn list(&mut self) -> Result<Vec<u64>, Error> {
        match self.call(Request::List)? {
            Response::Ids(ids) => Ok(ids),
            other => Err(Error::Unexpected("list", other)),
        }
    }

    /// All ids and boxes (no payloads) — for browsing a whole database.
    pub fn list_boxes(&mut self) -> Result<Vec<(u64, Bbox)>, Error> {
        match self.call(Request::ListBoxes)? {
            Response::Records(records) => Ok(records),
            other => Err(Error::Unexpected("list_boxes", other)),
        }
    }

    /// Summary stats about the database.
    pub fn stats(&mut self) -> Result<Stats, Error> {
        match self.call(Request::Stats)? {
            Response::Stats { count, cell_size } => Ok(Stats { count, cell_size }),
            other => Err(Error::Unexpected("stats", other)),
        }
    }

    /// Ask the server to flush the in-memory database to its data file.
    pub fn save(&mut self) -> Result<(), Error> {
        match self.call(Request::Save)? {
            Response::Ok => Ok(()),
            other => Err(Error::Unexpected("save", other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shilpi_protocol::{recv_request, write_message};
    use std::io::Cursor;

    /// A fake duplex stream: reads are served from a preloaded buffer of canned
    /// response frames; writes (the requests we send) are captured.
    struct Fake {
        read: Cursor<Vec<u8>>,
        wrote: Vec<u8>,
    }

    impl Read for Fake {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read.read(buf)
        }
    }

    impl Write for Fake {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.wrote.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A client whose next reads return `responses` in order.
    fn client_with(responses: &[Response]) -> Client<Fake> {
        let mut buf = Vec::new();
        for r in responses {
            write_message(&mut buf, &r.encode()).unwrap();
        }
        Client::from_stream(Fake {
            read: Cursor::new(buf),
            wrote: Vec::new(),
        })
    }

    fn bb() -> Bbox {
        Bbox::from_corners(0.0, 0.0, 1.0, 1.0)
    }

    #[test]
    fn ping_ok() {
        assert!(client_with(&[Response::Pong]).ping().is_ok());
    }

    #[test]
    fn get_found_and_missing() {
        let mut c = client_with(&[
            Response::Found {
                bbox: bb(),
                payload: b"x".to_vec(),
            },
            Response::NotFound,
        ]);
        assert_eq!(
            c.get(1).unwrap(),
            Some(Record {
                bbox: bb(),
                payload: b"x".to_vec()
            })
        );
        assert_eq!(c.get(2).unwrap(), None);
    }

    #[test]
    fn insert_many_sends_one_request() {
        let mut c = client_with(&[Response::Ok]);
        c.insert_many(vec![(1, bb(), vec![1, 2]), (2, bb(), vec![])])
            .unwrap();
        // A single InsertMany frame carried both records.
        let mut cur = Cursor::new(c.into_inner().wrote);
        match recv_request(&mut cur).unwrap().unwrap() {
            Request::InsertMany { records } => assert_eq!(records.len(), 2),
            other => panic!("expected InsertMany, got {other:?}"),
        }
        assert!(recv_request(&mut cur).unwrap().is_none(), "only one frame");
    }

    #[test]
    fn delete_and_query_and_stats() {
        let mut c = client_with(&[
            Response::Deleted(true),
            Response::Ids(vec![1, 2, 3]),
            Response::Stats {
                count: 3,
                cell_size: 1.5,
            },
        ]);
        assert!(c.delete(9).unwrap());
        assert_eq!(c.query_bbox(bb()).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            c.stats().unwrap(),
            Stats {
                count: 3,
                cell_size: 1.5
            }
        );
    }

    #[test]
    fn server_error_becomes_error_server() {
        let mut c = client_with(&[Response::Err("server is read-only".into())]);
        match c.insert(1, bb(), vec![]) {
            Err(Error::Server(msg)) => assert_eq!(msg, "server is read-only"),
            other => panic!("expected Error::Server, got {other:?}"),
        }
    }

    #[test]
    fn unexpected_variant_is_flagged() {
        // Server answers a ping with Ok — nonsensical for that op.
        assert!(matches!(
            client_with(&[Response::Ok]).ping(),
            Err(Error::Unexpected("ping", _))
        ));
    }

    #[test]
    fn closed_when_no_response() {
        assert!(matches!(client_with(&[]).ping(), Err(Error::Closed)));
    }

    #[test]
    fn sends_the_encoded_request() {
        let mut c = client_with(&[Response::Ok]);
        c.insert(7, bb(), b"p".to_vec()).unwrap();
        // What we wrote must decode back into the same request.
        let mut cur = Cursor::new(c.into_inner().wrote);
        let req = recv_request(&mut cur).unwrap().unwrap();
        assert_eq!(
            req,
            Request::Insert {
                id: 7,
                bbox: bb(),
                payload: b"p".to_vec()
            }
        );
    }
}
