//! The `shilpid` wire protocol: length-framed requests and responses built on
//! the `shilpidb` codec, shared by the server, the CLI shell, and the GUI.
//!
//! A message on the wire is a `u32` little-endian byte length followed by
//! that many bytes — an opcode byte, then the codec-encoded fields for that
//! variant. [`write_message`] / [`read_message`] handle the framing over any
//! `Write` / `Read`; [`Request::encode`] / [`Request::decode`] (and the
//! `Response` equivalents) handle the payload.

use std::io::{self, Read, Write};

use shilpidb::{Bbox, Point, Reader, Writer};

/// Frames larger than this are rejected instead of allocated — a
/// malformed or hostile length prefix must not force an unbounded read.
pub const MAX_MESSAGE_LEN: u32 = 64 * 1024 * 1024;

/// Errors decoding a [`Request`] or [`Response`] from bytes.
#[derive(Debug)]
pub enum ProtocolError {
    /// The codec couldn't decode the fields for the opcode.
    Codec(shilpidb::Error),
    /// The first byte wasn't a known opcode.
    UnknownOpcode(u8),
    /// The frame was empty (no opcode byte).
    EmptyMessage,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::Codec(e) => write!(f, "shilpi-protocol: {e}"),
            ProtocolError::UnknownOpcode(op) => {
                write!(f, "shilpi-protocol: unknown opcode {op}")
            }
            ProtocolError::EmptyMessage => write!(f, "shilpi-protocol: empty message"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<shilpidb::Error> for ProtocolError {
    fn from(e: shilpidb::Error) -> Self {
        ProtocolError::Codec(e)
    }
}

/// Read one length-framed message: a `u32` LE length, then that many bytes.
///
/// Returns `Ok(None)` on a clean EOF before any bytes of a new frame arrive
/// (the normal way a connection ends); an `Err` for anything else, including
/// a frame declaring a length over [`MAX_MESSAGE_LEN`].
pub fn read_message(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_MESSAGE_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message length {len} exceeds max {MAX_MESSAGE_LEN}"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// Write one length-framed message: a `u32` LE length, then `bytes`.
pub fn write_message(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

fn write_bbox(w: &mut Writer, bb: Bbox) {
    w.f64(bb.min.x);
    w.f64(bb.min.y);
    w.f64(bb.max.x);
    w.f64(bb.max.y);
}

fn read_bbox(r: &mut Reader) -> Result<Bbox, shilpidb::Error> {
    let min = Point::new(r.f64()?, r.f64()?);
    let max = Point::new(r.f64()?, r.f64()?);
    Ok(Bbox::new(min, max))
}

fn write_payload(w: &mut Writer, payload: &[u8]) {
    w.u32(payload.len() as u32);
    w.raw(payload);
}

fn read_payload(r: &mut Reader) -> Result<Vec<u8>, shilpidb::Error> {
    let n = r.u32()? as usize;
    Ok(r.raw(n)?.to_vec())
}

fn write_ids(w: &mut Writer, ids: &[u64]) {
    w.u32(ids.len() as u32);
    for id in ids {
        w.u64(*id);
    }
}

fn read_ids(r: &mut Reader) -> Result<Vec<u64>, shilpidb::Error> {
    let n = r.u32()? as usize;
    let mut ids = Vec::with_capacity(n);
    for _ in 0..n {
        ids.push(r.u64()?);
    }
    Ok(ids)
}

macro_rules! opcodes {
    ($($name:ident = $val:expr),* $(,)?) => {
        $(const $name: u8 = $val;)*
    };
}

opcodes! {
    OP_PING = 0,
    OP_INSERT = 1,
    OP_GET = 2,
    OP_DELETE = 3,
    OP_QUERY_BBOX = 4,
    OP_QUERY_POINT = 5,
    OP_LIST = 6,
    OP_STATS = 7,
    OP_SAVE = 8,
    OP_LIST_BOXES = 9,
    OP_INSERT_MANY = 10,
    OP_PONG = 64,
    OP_OK = 65,
    OP_ERR = 66,
    OP_FOUND = 67,
    OP_NOT_FOUND = 68,
    OP_DELETED = 69,
    OP_IDS = 70,
    OP_STATS_REPLY = 71,
    OP_RECORDS_REPLY = 72,
}

/// A request sent from a client (the CLI shell, the GUI, or any other
/// caller of `shilpi-protocol`) to `shilpid`.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Liveness check; expect [`Response::Pong`].
    Ping,
    /// Insert or replace the record at `id`.
    Insert {
        id: u64,
        bbox: Bbox,
        payload: Vec<u8>,
    },
    /// Insert or replace many records in one round-trip; expect
    /// [`Response::Ok`]. The whole batch shares one grid rebuild server-side.
    InsertMany { records: Vec<(u64, Bbox, Vec<u8>)> },
    /// Fetch the record at `id`, if any.
    Get { id: u64 },
    /// Remove the record at `id`, if any.
    Delete { id: u64 },
    /// Ids of records whose box intersects `rect`.
    QueryBbox { rect: Bbox },
    /// Ids of records within `tol` of point `(x, y)`.
    QueryPoint { x: f64, y: f64, tol: f64 },
    /// All record ids currently stored.
    List,
    /// Summary stats about the database.
    Stats,
    /// Flush the in-memory database to its data file.
    Save,
    /// All record ids and boxes (no payload bytes) — for browsing/rendering
    /// a whole database without an N+1 `Get` per record.
    ListBoxes,
}

/// A reply sent from `shilpid` back to a client.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Pong,
    Ok,
    Err(String),
    Found { bbox: Bbox, payload: Vec<u8> },
    NotFound,
    Deleted(bool),
    Ids(Vec<u64>),
    Stats { count: u32, cell_size: f64 },
    Records(Vec<(u64, Bbox)>),
}

impl Request {
    /// Encode this request as a message body (no length frame).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Request::Ping => w.u8(OP_PING),
            Request::Insert { id, bbox, payload } => {
                w.u8(OP_INSERT);
                w.u64(*id);
                write_bbox(&mut w, *bbox);
                write_payload(&mut w, payload);
            }
            Request::InsertMany { records } => {
                w.u8(OP_INSERT_MANY);
                w.u32(records.len() as u32);
                for (id, bbox, payload) in records {
                    w.u64(*id);
                    write_bbox(&mut w, *bbox);
                    write_payload(&mut w, payload);
                }
            }
            Request::Get { id } => {
                w.u8(OP_GET);
                w.u64(*id);
            }
            Request::Delete { id } => {
                w.u8(OP_DELETE);
                w.u64(*id);
            }
            Request::QueryBbox { rect } => {
                w.u8(OP_QUERY_BBOX);
                write_bbox(&mut w, *rect);
            }
            Request::QueryPoint { x, y, tol } => {
                w.u8(OP_QUERY_POINT);
                w.f64(*x);
                w.f64(*y);
                w.f64(*tol);
            }
            Request::List => w.u8(OP_LIST),
            Request::Stats => w.u8(OP_STATS),
            Request::Save => w.u8(OP_SAVE),
            Request::ListBoxes => w.u8(OP_LIST_BOXES),
        }
        w.into_bytes()
    }

    /// Decode a request previously produced by [`encode`](Self::encode).
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let op = r.u8().map_err(|_| ProtocolError::EmptyMessage)?;
        Ok(match op {
            OP_PING => Request::Ping,
            OP_INSERT => {
                let id = r.u64()?;
                let bbox = read_bbox(&mut r)?;
                let payload = read_payload(&mut r)?;
                Request::Insert { id, bbox, payload }
            }
            OP_INSERT_MANY => {
                let n = r.u32()? as usize;
                let mut records = Vec::with_capacity(n);
                for _ in 0..n {
                    let id = r.u64()?;
                    let bbox = read_bbox(&mut r)?;
                    let payload = read_payload(&mut r)?;
                    records.push((id, bbox, payload));
                }
                Request::InsertMany { records }
            }
            OP_GET => Request::Get { id: r.u64()? },
            OP_DELETE => Request::Delete { id: r.u64()? },
            OP_QUERY_BBOX => Request::QueryBbox {
                rect: read_bbox(&mut r)?,
            },
            OP_QUERY_POINT => Request::QueryPoint {
                x: r.f64()?,
                y: r.f64()?,
                tol: r.f64()?,
            },
            OP_LIST => Request::List,
            OP_STATS => Request::Stats,
            OP_SAVE => Request::Save,
            OP_LIST_BOXES => Request::ListBoxes,
            other => return Err(ProtocolError::UnknownOpcode(other)),
        })
    }
}

impl Response {
    /// Encode this response as a message body (no length frame).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Response::Pong => w.u8(OP_PONG),
            Response::Ok => w.u8(OP_OK),
            Response::Err(msg) => {
                w.u8(OP_ERR);
                w.str(msg);
            }
            Response::Found { bbox, payload } => {
                w.u8(OP_FOUND);
                write_bbox(&mut w, *bbox);
                write_payload(&mut w, payload);
            }
            Response::NotFound => w.u8(OP_NOT_FOUND),
            Response::Deleted(existed) => {
                w.u8(OP_DELETED);
                w.bool(*existed);
            }
            Response::Ids(ids) => {
                w.u8(OP_IDS);
                write_ids(&mut w, ids);
            }
            Response::Stats { count, cell_size } => {
                w.u8(OP_STATS_REPLY);
                w.u32(*count);
                w.f64(*cell_size);
            }
            Response::Records(records) => {
                w.u8(OP_RECORDS_REPLY);
                w.u32(records.len() as u32);
                for (id, bbox) in records {
                    w.u64(*id);
                    write_bbox(&mut w, *bbox);
                }
            }
        }
        w.into_bytes()
    }

    /// Decode a response previously produced by [`encode`](Self::encode).
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let op = r.u8().map_err(|_| ProtocolError::EmptyMessage)?;
        Ok(match op {
            OP_PONG => Response::Pong,
            OP_OK => Response::Ok,
            OP_ERR => Response::Err(r.str()?),
            OP_FOUND => {
                let bbox = read_bbox(&mut r)?;
                let payload = read_payload(&mut r)?;
                Response::Found { bbox, payload }
            }
            OP_NOT_FOUND => Response::NotFound,
            OP_DELETED => Response::Deleted(r.bool()?),
            OP_IDS => Response::Ids(read_ids(&mut r)?),
            OP_STATS_REPLY => Response::Stats {
                count: r.u32()?,
                cell_size: r.f64()?,
            },
            OP_RECORDS_REPLY => {
                let n = r.u32()? as usize;
                let mut records = Vec::with_capacity(n);
                for _ in 0..n {
                    let id = r.u64()?;
                    let bbox = read_bbox(&mut r)?;
                    records.push((id, bbox));
                }
                Response::Records(records)
            }
            other => return Err(ProtocolError::UnknownOpcode(other)),
        })
    }
}

/// Send a [`Request`] and frame it on the wire in one call.
pub fn send_request(w: &mut impl Write, req: &Request) -> io::Result<()> {
    write_message(w, &req.encode())
}

/// Read one framed message and decode it as a [`Request`].
///
/// Returns `Ok(None)` on clean connection close, matching [`read_message`].
pub fn recv_request(r: &mut impl Read) -> io::Result<Option<Request>> {
    match read_message(r)? {
        None => Ok(None),
        Some(bytes) => Request::decode(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
    }
}

/// Send a [`Response`] and frame it on the wire in one call.
pub fn send_response(w: &mut impl Write, resp: &Response) -> io::Result<()> {
    write_message(w, &resp.encode())
}

/// Read one framed message and decode it as a [`Response`].
///
/// Returns `Ok(None)` on clean connection close, matching [`read_message`].
pub fn recv_response(r: &mut impl Read) -> io::Result<Option<Response>> {
    match read_message(r)? {
        None => Ok(None),
        Some(bytes) => Response::decode(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(x0: f64, y0: f64, x1: f64, y1: f64) -> Bbox {
        Bbox::from_corners(x0, y0, x1, y1)
    }

    #[test]
    fn requests_round_trip() {
        let reqs = vec![
            Request::Ping,
            Request::Insert {
                id: 7,
                bbox: bb(0.0, 0.0, 1.0, 1.0),
                payload: vec![1, 2, 3],
            },
            Request::Get { id: 7 },
            Request::Delete { id: 7 },
            Request::QueryBbox {
                rect: bb(-1.0, -1.0, 1.0, 1.0),
            },
            Request::QueryPoint {
                x: 1.0,
                y: 2.0,
                tol: 0.5,
            },
            Request::List,
            Request::Stats,
            Request::Save,
            Request::ListBoxes,
            Request::InsertMany {
                records: vec![
                    (1, bb(0.0, 0.0, 1.0, 1.0), vec![1, 2]),
                    (2, bb(5.0, 5.0, 6.0, 6.0), vec![]),
                ],
            },
        ];
        for req in reqs {
            let bytes = req.encode();
            assert_eq!(Request::decode(&bytes).unwrap(), req);
        }
    }

    #[test]
    fn responses_round_trip() {
        let resps = vec![
            Response::Pong,
            Response::Ok,
            Response::Err("boom".into()),
            Response::Found {
                bbox: bb(0.0, 0.0, 2.0, 2.0),
                payload: vec![9, 9],
            },
            Response::NotFound,
            Response::Deleted(true),
            Response::Ids(vec![1, 2, 3]),
            Response::Stats {
                count: 42,
                cell_size: 1.5,
            },
            Response::Records(vec![
                (1, bb(0.0, 0.0, 1.0, 1.0)),
                (2, bb(5.0, 5.0, 6.0, 6.0)),
            ]),
        ];
        for resp in resps {
            let bytes = resp.encode();
            assert_eq!(Response::decode(&bytes).unwrap(), resp);
        }
    }

    #[test]
    fn framing_round_trips_over_a_pipe() {
        let req = Request::Insert {
            id: 1,
            bbox: bb(0.0, 0.0, 1.0, 1.0),
            payload: b"hello".to_vec(),
        };
        let mut buf = Vec::new();
        send_request(&mut buf, &req).unwrap();
        let mut cursor = io::Cursor::new(buf);
        let got = recv_request(&mut cursor).unwrap().unwrap();
        assert_eq!(got, req);
        // A second read on the now-exhausted cursor is a clean EOF.
        assert!(recv_request(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn oversized_length_prefix_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_MESSAGE_LEN + 1).to_le_bytes());
        let mut cursor = io::Cursor::new(buf);
        assert!(read_message(&mut cursor).is_err());
    }

    #[test]
    fn unknown_opcode_is_a_protocol_error() {
        assert!(matches!(
            Request::decode(&[255]),
            Err(ProtocolError::UnknownOpcode(255))
        ));
    }
}
