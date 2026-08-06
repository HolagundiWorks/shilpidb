//! A tiny, hand-rolled HTTP/1.1 request reader and response writer.
//!
//! Scope is deliberately minimal — exactly what the gateway needs and no more:
//! read one request per connection (the gateway always replies
//! `Connection: close`), parse the request line, headers, and a
//! `Content-Length`-delimited body. Chunked transfer encoding is not supported.

use std::io::{self, BufRead, Write};

/// Requests whose body exceeds this are rejected — a hostile or mistaken
/// `Content-Length` must not force an unbounded allocation. Matches
/// `shilpi_protocol::MAX_MESSAGE_LEN` (64 MiB).
pub const MAX_BODY_LEN: usize = 64 * 1024 * 1024;

/// The header block (request line + headers) may not exceed this.
const MAX_HEADER_LEN: usize = 64 * 1024;

/// A parsed HTTP request: method, decoded path, raw query params, and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Percent-decoded path segments (no leading empty segment, no query).
    pub path: Vec<String>,
    /// Decoded `(key, value)` query params in order.
    pub query: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Read and parse one request from `r`. Returns `Ok(None)` on a clean EOF
    /// before any request bytes (the connection closed). A malformed request or
    /// oversized body is an `Err`.
    pub fn read(r: &mut impl BufRead) -> io::Result<Option<Request>> {
        let mut request_line = String::new();
        let n = read_line_limited(r, &mut request_line, MAX_HEADER_LEN)?;
        if n == 0 {
            return Ok(None); // clean EOF, no request
        }
        let request_line = request_line.trim_end_matches(['\r', '\n']);
        let mut parts = request_line.split(' ');
        let method = parts
            .next()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| bad("empty request line"))?
            .to_string();
        let target = parts.next().ok_or_else(|| bad("missing request target"))?;

        let mut header_budget = MAX_HEADER_LEN.saturating_sub(request_line.len());
        let mut content_length: usize = 0;
        let mut chunked = false;
        loop {
            let mut line = String::new();
            let read = read_line_limited(r, &mut line, header_budget)?;
            if read == 0 {
                return Err(bad("connection closed inside headers"));
            }
            header_budget = header_budget.saturating_sub(read);
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break; // end of header block
            }
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim();
                match name.as_str() {
                    "content-length" => {
                        content_length = value
                            .parse::<usize>()
                            .map_err(|_| bad("invalid Content-Length"))?;
                    }
                    "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => {
                        chunked = true;
                    }
                    _ => {}
                }
            }
        }

        if chunked {
            return Err(bad("chunked transfer encoding is not supported"));
        }
        if content_length > MAX_BODY_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("body length {content_length} exceeds max {MAX_BODY_LEN}"),
            ));
        }

        let mut body = vec![0u8; content_length];
        r.read_exact(&mut body)?;

        let (raw_path, raw_query) = match target.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (target, None),
        };
        let path = parse_path(raw_path);
        let query = raw_query.map(parse_query).unwrap_or_default();

        Ok(Some(Request {
            method,
            path,
            query,
            body,
        }))
    }

    /// First value for `key` in the query string, if present.
    pub fn query_get(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// A response to write back: status, JSON body.
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn json(status: u16, body: String) -> Self {
        Response { status, body }
    }

    /// `{"error": "<message>"}` with the given status.
    pub fn error(status: u16, message: &str) -> Self {
        Response {
            status,
            body: format!("{{\"error\":{}}}", crate::json::string(message)),
        }
    }

    /// Serialize to the wire: status line, headers, and body. Always closes the
    /// connection (`Connection: close`) — one request per connection.
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let reason = reason_phrase(self.status);
        let bytes = self.body.as_bytes();
        write!(
            w,
            "HTTP/1.1 {} {reason}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            self.status,
            bytes.len(),
        )?;
        w.write_all(bytes)?;
        w.flush()
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read a line (up to and including `\n`) but fail if it would exceed `limit`
/// bytes, so a header line can't drive an unbounded allocation.
fn read_line_limited(r: &mut impl BufRead, out: &mut String, limit: usize) -> io::Result<usize> {
    let mut buf = Vec::new();
    let mut total = 0;
    loop {
        let available = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            break; // EOF
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                buf.extend_from_slice(&available[..=i]);
                total += i + 1;
                r.consume(i + 1);
                break;
            }
            None => {
                buf.extend_from_slice(available);
                total += available.len();
                let consumed = available.len();
                r.consume(consumed);
            }
        }
        if total > limit {
            return Err(bad("header line too long"));
        }
    }
    if total > limit {
        return Err(bad("header line too long"));
    }
    let s = String::from_utf8(buf).map_err(|_| bad("invalid utf-8 in request header"))?;
    out.push_str(&s);
    Ok(total)
}

fn parse_path(raw: &str) -> Vec<String> {
    raw.split('/')
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect()
}

fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// Decode `%XX` escapes and `+` (form-encoded space). Invalid escapes are left
/// verbatim rather than erroring — a lenient reader for a small internal API.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h << 4) | l);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        502 => "Bad Gateway",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &str) -> Option<Request> {
        let mut cur = Cursor::new(raw.as_bytes().to_vec());
        Request::read(&mut cur).unwrap()
    }

    #[test]
    fn parses_get_with_query() {
        let req = parse("GET /query/bbox?minX=-5&maxY=10.5 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, vec!["query", "bbox"]);
        assert_eq!(req.query_get("minX"), Some("-5"));
        assert_eq!(req.query_get("maxY"), Some("10.5"));
        assert!(req.body.is_empty());
    }

    #[test]
    fn parses_put_with_body() {
        let req = parse("PUT /records/7 HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello").unwrap();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.path, vec!["records", "7"]);
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn clean_eof_is_none() {
        assert!(parse("").is_none());
    }

    #[test]
    fn percent_and_plus_decoding() {
        let req = parse("GET /a?name=hello+world%21 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(req.query_get("name"), Some("hello world!"));
    }

    #[test]
    fn chunked_is_rejected() {
        let mut cur =
            Cursor::new(b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec());
        assert!(Request::read(&mut cur).is_err());
    }

    #[test]
    fn response_serializes_with_headers() {
        let mut buf = Vec::new();
        Response::json(200, "{\"ok\":true}".to_string())
            .write(&mut buf)
            .unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"));
    }
}
