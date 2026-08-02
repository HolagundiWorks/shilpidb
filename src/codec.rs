//! Little-endian binary read/write primitives.
//!
//! Deliberately hand-rolled (no `serde`, no external crate) — ShilpiDB is a
//! *custom* format we fully control. Strings are `u32`-length-prefixed UTF-8;
//! integers and floats are fixed-width little-endian. The codec is intentionally
//! **scalar-only** (no geometry types) so it stays domain-agnostic; a host
//! composes points/records from `f64`s.

/// Errors from ShilpiDB encoding / decoding / I/O.
#[derive(Debug)]
pub enum Error {
    /// Underlying filesystem error.
    Io(std::io::Error),
    /// The bytes are not a valid payload.
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "shilpidb i/o: {e}"),
            Error::Format(m) => write!(f, "shilpidb: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Append-only little-endian byte writer.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// `u32` length prefix + UTF-8 bytes.
    pub fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// Raw bytes (no length prefix) — magic, fixed-size fields.
    pub fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// Cursor over an encoded byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.data.len())
            .ok_or_else(|| Error::Format("unexpected end of data".into()))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    pub fn bool(&mut self) -> Result<bool, Error> {
        Ok(self.u8()? != 0)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn i64(&mut self) -> Result<i64, Error> {
        Ok(self.u64()? as i64)
    }

    pub fn f64(&mut self) -> Result<f64, Error> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn str(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| Error::Format("invalid utf-8 string".into()))
    }

    pub fn raw(&mut self, n: usize) -> Result<&'a [u8], Error> {
        self.take(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_round_trip() {
        let mut w = Writer::new();
        w.u8(7);
        w.bool(true);
        w.u32(70_000);
        w.u64(9_000_000_000);
        w.i64(-42);
        w.f64(3.5);
        w.str("hÉllo");
        let bytes = w.into_bytes();

        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8().unwrap(), 7);
        assert!(r.bool().unwrap());
        assert_eq!(r.u32().unwrap(), 70_000);
        assert_eq!(r.u64().unwrap(), 9_000_000_000);
        assert_eq!(r.i64().unwrap(), -42);
        assert_eq!(r.f64().unwrap(), 3.5);
        assert_eq!(r.str().unwrap(), "hÉllo");
    }

    #[test]
    fn truncated_input_is_a_format_error() {
        let mut w = Writer::new();
        w.u32(4);
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes[..2]); // half a u32
        assert!(matches!(r.u32(), Err(Error::Format(_))));
    }
}
