//! Just enough JSON *emission* to serialize gateway responses by hand.
//!
//! ShilpiDB is deliberately dependency-free, so — like the rest of the stack —
//! this gateway hand-rolls what it needs instead of pulling in `serde_json`.
//! Only serialization is implemented (the gateway never *parses* JSON: request
//! inputs arrive as path segments, query params, and the raw request body), so
//! there is no general-purpose JSON reader here.

use shilpidb::Bbox;

/// Escape a string's contents for embedding inside JSON double quotes. Does not
/// add the surrounding quotes — callers wrap with `"` themselves via
/// [`string`].
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A JSON string literal (escaped and quoted).
pub fn string(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// A JSON number, or `null` for a non-finite `f64` (JSON has no NaN/Infinity).
pub fn number(v: f64) -> String {
    if v.is_finite() {
        // `{}` on an `f64` yields the shortest round-trippable decimal, which is
        // valid JSON (e.g. `10`, `3.5`, `-0.25`).
        format!("{v}")
    } else {
        "null".to_string()
    }
}

/// Lowercase hex for opaque payload bytes (unambiguous regardless of encoding).
pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// A bounding box as `{"minX":..,"minY":..,"maxX":..,"maxY":..}`.
pub fn bbox(bb: &Bbox) -> String {
    format!(
        "{{\"minX\":{},\"minY\":{},\"maxX\":{},\"maxY\":{}}}",
        number(bb.min.x),
        number(bb.min.y),
        number(bb.max.x),
        number(bb.max.y),
    )
}

/// A JSON array of `u64` ids: `[1,2,3]`.
pub fn ids(ids: &[u64]) -> String {
    let mut out = String::from("[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&id.to_string());
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_control_and_quotes() {
        assert_eq!(escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(escape("line\nbreak\ttab"), "line\\nbreak\\ttab");
        assert_eq!(escape("\u{01}"), "\\u0001");
        // Non-ASCII passes through untouched (valid in a JSON string).
        assert_eq!(escape("héllo"), "héllo");
    }

    #[test]
    fn numbers_and_non_finite() {
        assert_eq!(number(10.0), "10");
        assert_eq!(number(3.5), "3.5");
        assert_eq!(number(f64::NAN), "null");
        assert_eq!(number(f64::INFINITY), "null");
    }

    #[test]
    fn hex_encoding() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn bbox_and_ids() {
        let bb = Bbox::from_corners(0.0, 1.0, 2.0, 3.0);
        assert_eq!(bbox(&bb), "{\"minX\":0,\"minY\":1,\"maxX\":2,\"maxY\":3}");
        assert_eq!(ids(&[1, 2, 3]), "[1,2,3]");
        assert_eq!(ids(&[]), "[]");
    }
}
