//! Parsing for `shilpi`'s small, mongosh-flavored command language, and
//! formatting for the responses it gets back.

use shilpi_protocol::{Request, Response};
use shilpidb::Bbox;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Ping,
    Insert {
        id: u64,
        rect: Bbox,
        payload: String,
    },
    Get {
        id: u64,
    },
    Delete {
        id: u64,
    },
    QueryBbox {
        rect: Bbox,
    },
    QueryPoint {
        x: f64,
        y: f64,
        tol: f64,
    },
    List,
    ListBoxes,
    Stats,
    Save,
    Help,
    Exit,
}

impl Command {
    pub fn into_request(self) -> Option<Request> {
        match self {
            Command::Ping => Some(Request::Ping),
            Command::Insert { id, rect, payload } => Some(Request::Insert {
                id,
                bbox: rect,
                payload: payload.into_bytes(),
            }),
            Command::Get { id } => Some(Request::Get { id }),
            Command::Delete { id } => Some(Request::Delete { id }),
            Command::QueryBbox { rect } => Some(Request::QueryBbox { rect }),
            Command::QueryPoint { x, y, tol } => Some(Request::QueryPoint { x, y, tol }),
            Command::List => Some(Request::List),
            Command::ListBoxes => Some(Request::ListBoxes),
            Command::Stats => Some(Request::Stats),
            Command::Save => Some(Request::Save),
            Command::Help | Command::Exit => None,
        }
    }
}

fn parse_f64(tok: Option<&str>, what: &str) -> Result<f64, String> {
    tok.ok_or_else(|| format!("missing {what}"))?
        .parse::<f64>()
        .map_err(|_| format!("'{what}' must be a number"))
}

fn parse_u64(tok: Option<&str>, what: &str) -> Result<u64, String> {
    tok.ok_or_else(|| format!("missing {what}"))?
        .parse::<u64>()
        .map_err(|_| format!("'{what}' must be a non-negative integer"))
}

/// Parse one line of input. `Ok(None)` means a blank line (nothing to do).
pub fn parse(line: &str) -> Result<Option<Command>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut toks = line.split_whitespace();
    let cmd = toks.next().unwrap().to_ascii_lowercase();

    let cmd = match cmd.as_str() {
        "ping" => Command::Ping,
        "list" | "ls" => Command::List,
        "boxes" | "listboxes" => Command::ListBoxes,
        "stats" => Command::Stats,
        "save" => Command::Save,
        "help" | "?" => Command::Help,
        "exit" | "quit" => Command::Exit,
        "get" => Command::Get {
            id: parse_u64(toks.next(), "id")?,
        },
        "delete" | "del" | "remove" => Command::Delete {
            id: parse_u64(toks.next(), "id")?,
        },
        "insert" | "put" => {
            let id = parse_u64(toks.next(), "id")?;
            let x0 = parse_f64(toks.next(), "x0")?;
            let y0 = parse_f64(toks.next(), "y0")?;
            let x1 = parse_f64(toks.next(), "x1")?;
            let y1 = parse_f64(toks.next(), "y1")?;
            let payload = toks.collect::<Vec<_>>().join(" ");
            Command::Insert {
                id,
                rect: Bbox::from_corners(x0, y0, x1, y1),
                payload,
            }
        }
        "query" => {
            let kind = toks
                .next()
                .ok_or_else(|| "expected 'query bbox ...' or 'query point ...'".to_string())?
                .to_ascii_lowercase();
            match kind.as_str() {
                "bbox" | "rect" => {
                    let x0 = parse_f64(toks.next(), "x0")?;
                    let y0 = parse_f64(toks.next(), "y0")?;
                    let x1 = parse_f64(toks.next(), "x1")?;
                    let y1 = parse_f64(toks.next(), "y1")?;
                    Command::QueryBbox {
                        rect: Bbox::from_corners(x0, y0, x1, y1),
                    }
                }
                "point" => {
                    let x = parse_f64(toks.next(), "x")?;
                    let y = parse_f64(toks.next(), "y")?;
                    let tol = parse_f64(toks.next(), "tol")?;
                    Command::QueryPoint { x, y, tol }
                }
                other => return Err(format!("unknown query kind '{other}'")),
            }
        }
        other => return Err(format!("unknown command '{other}' (try 'help')")),
    };
    Ok(Some(cmd))
}

pub fn format_response(resp: &Response) -> String {
    match resp {
        Response::Pong => "pong".to_string(),
        Response::Ok => "ok".to_string(),
        Response::Err(msg) => format!("error: {msg}"),
        Response::Found { bbox, payload } => format!(
            "bbox=({}, {})-({}, {}) payload={:?}",
            bbox.min.x,
            bbox.min.y,
            bbox.max.x,
            bbox.max.y,
            String::from_utf8_lossy(payload)
        ),
        Response::NotFound => "not found".to_string(),
        Response::Deleted(existed) => {
            if *existed {
                "deleted".to_string()
            } else {
                "not found".to_string()
            }
        }
        Response::Ids(ids) => {
            if ids.is_empty() {
                "(no matches)".to_string()
            } else {
                ids.iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        Response::Stats { count, cell_size } => {
            format!("{count} record(s), cell_size={cell_size}")
        }
        Response::Records(records) => {
            if records.is_empty() {
                "(no records)".to_string()
            } else {
                records
                    .iter()
                    .map(|(id, bbox)| {
                        format!(
                            "{id}: ({}, {})-({}, {})",
                            bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }
}

pub const HELP_TEXT: &str = "\
Commands:
  ping
  insert <id> <x0> <y0> <x1> <y1> <payload text...>
  get <id>
  delete <id>
  query bbox <x0> <y0> <x1> <y1>
  query point <x> <y> <tol>
  list
  boxes
  stats
  save
  help
  exit";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_insert_with_multi_word_payload() {
        let cmd = parse("insert 1 0 0 1 1 hello world").unwrap().unwrap();
        assert_eq!(
            cmd,
            Command::Insert {
                id: 1,
                rect: Bbox::from_corners(0.0, 0.0, 1.0, 1.0),
                payload: "hello world".to_string(),
            }
        );
    }

    #[test]
    fn blank_and_comment_lines_are_none() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("   ").unwrap(), None);
        assert_eq!(parse("# a comment").unwrap(), None);
    }

    #[test]
    fn rejects_bad_numbers() {
        assert!(parse("insert x 0 0 1 1 p").is_err());
        assert!(parse("get notanumber").is_err());
    }

    #[test]
    fn parses_queries() {
        assert_eq!(
            parse("query bbox 0 0 1 1").unwrap().unwrap(),
            Command::QueryBbox {
                rect: Bbox::from_corners(0.0, 0.0, 1.0, 1.0)
            }
        );
        assert_eq!(
            parse("query point 1 2 0.5").unwrap().unwrap(),
            Command::QueryPoint {
                x: 1.0,
                y: 2.0,
                tol: 0.5
            }
        );
    }
}
