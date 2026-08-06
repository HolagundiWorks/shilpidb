//! Maps HTTP requests onto [`Backend`] operations and builds JSON responses.
//!
//! Input convention (no JSON *parsing* needed): identifiers come from the path,
//! bounding boxes / points from query params, and an insert payload from the
//! raw request body. Every response is JSON.

use crate::http::{Request, Response};
use crate::json;
use crate::Backend;
use shilpidb::Bbox;

/// Dispatch one request against the backend.
pub fn route(req: &Request, backend: &mut dyn Backend) -> Response {
    let seg: Vec<&str> = req.path.iter().map(String::as_str).collect();
    match seg.as_slice() {
        [] => Response::json(
            200,
            "{\"service\":\"shilpi-http\",\"status\":\"ok\"}".to_string(),
        ),
        ["health"] => require(req, "GET", |_| match backend.ping() {
            Ok(()) => Response::json(200, "{\"status\":\"ok\"}".to_string()),
            Err(e) => backend_error(&e),
        }),
        ["stats"] => require(req, "GET", |_| match backend.stats() {
            Ok(s) => Response::json(
                200,
                format!(
                    "{{\"count\":{},\"cellSize\":{}}}",
                    s.count,
                    json::number(s.cell_size)
                ),
            ),
            Err(e) => backend_error(&e),
        }),
        ["records"] => require(req, "GET", |_| match backend.list_boxes() {
            Ok(records) => {
                let items: Vec<String> = records
                    .iter()
                    .map(|(id, bb)| format!("{{\"id\":{},\"bbox\":{}}}", id, json::bbox(bb)))
                    .collect();
                Response::json(200, format!("{{\"records\":[{}]}}", items.join(",")))
            }
            Err(e) => backend_error(&e),
        }),
        ["records", id] => {
            let id = match parse_u64(id) {
                Ok(v) => v,
                Err(r) => return r,
            };
            match req.method.as_str() {
                "GET" => match backend.get(id) {
                    Ok(Some(rec)) => Response::json(200, record_json(id, &rec.bbox, &rec.payload)),
                    Ok(None) => Response::error(404, "record not found"),
                    Err(e) => backend_error(&e),
                },
                "PUT" => {
                    let bbox = match bbox_from_query(req) {
                        Ok(b) => b,
                        Err(r) => return r,
                    };
                    match backend.insert(id, bbox, req.body.clone()) {
                        Ok(()) => Response::json(200, format!("{{\"status\":\"ok\",\"id\":{id}}}")),
                        Err(e) => backend_error(&e),
                    }
                }
                "DELETE" => match backend.delete(id) {
                    Ok(existed) => Response::json(200, format!("{{\"deleted\":{existed}}}")),
                    Err(e) => backend_error(&e),
                },
                _ => method_not_allowed("GET, PUT, DELETE"),
            }
        }
        ["query", "bbox"] => require(req, "GET", |req| {
            let bbox = match bbox_from_query(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            match backend.query_bbox(bbox) {
                Ok(hits) => Response::json(200, format!("{{\"ids\":{}}}", json::ids(&hits))),
                Err(e) => backend_error(&e),
            }
        }),
        ["query", "point"] => require(req, "GET", |req| {
            let x = match f64_param(req, "x") {
                Ok(v) => v,
                Err(r) => return r,
            };
            let y = match f64_param(req, "y") {
                Ok(v) => v,
                Err(r) => return r,
            };
            // `tol` is optional and defaults to an exact-point (0.0) query.
            let tol = match req.query_get("tol") {
                Some(v) => match v.parse::<f64>() {
                    Ok(t) => t,
                    Err(_) => return Response::error(400, "invalid number for 'tol'"),
                },
                None => 0.0,
            };
            match backend.query_point(x, y, tol) {
                Ok(hits) => Response::json(200, format!("{{\"ids\":{}}}", json::ids(&hits))),
                Err(e) => backend_error(&e),
            }
        }),
        ["save"] => require(req, "POST", |_| match backend.save() {
            Ok(()) => Response::json(200, "{\"status\":\"ok\"}".to_string()),
            Err(e) => backend_error(&e),
        }),
        _ => Response::error(404, "not found"),
    }
}

/// Run `handler` only if the method matches, else `405`.
fn require(req: &Request, method: &str, handler: impl FnOnce(&Request) -> Response) -> Response {
    if req.method == method {
        handler(req)
    } else {
        method_not_allowed(method)
    }
}

fn method_not_allowed(allowed: &str) -> Response {
    Response::error(405, &format!("method not allowed (allowed: {allowed})"))
}

/// A backend/transport failure becomes `502 Bad Gateway` — the gateway is up,
/// but `shilpid` behind it errored or is unreachable.
fn backend_error(msg: &str) -> Response {
    Response::error(502, &format!("backend: {msg}"))
}

fn parse_u64(s: &str) -> Result<u64, Response> {
    s.parse::<u64>()
        .map_err(|_| Response::error(400, &format!("invalid record id '{s}'")))
}

fn f64_param(req: &Request, key: &str) -> Result<f64, Response> {
    match req.query_get(key) {
        Some(v) => v
            .parse::<f64>()
            .map_err(|_| Response::error(400, &format!("invalid number for '{key}'"))),
        None => Err(Response::error(
            400,
            &format!("missing query param '{key}'"),
        )),
    }
}

fn bbox_from_query(req: &Request) -> Result<Bbox, Response> {
    let min_x = f64_param(req, "minX")?;
    let min_y = f64_param(req, "minY")?;
    let max_x = f64_param(req, "maxX")?;
    let max_y = f64_param(req, "maxY")?;
    Ok(Bbox::from_corners(min_x, min_y, max_x, max_y))
}

fn record_json(id: u64, bbox: &Bbox, payload: &[u8]) -> String {
    let text_field = match std::str::from_utf8(payload) {
        Ok(t) => json::string(t),
        Err(_) => "null".to_string(),
    };
    format!(
        "{{\"id\":{},\"bbox\":{},\"payloadHex\":{},\"payloadText\":{}}}",
        id,
        json::bbox(bbox),
        json::string(&json::hex(payload)),
        text_field,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Backend;
    use shilpi_client::{Record, Stats};
    use std::collections::HashMap;

    /// An in-memory fake backend so routing can be tested without a live server.
    #[derive(Default)]
    struct FakeBackend {
        records: HashMap<u64, (Bbox, Vec<u8>)>,
        saved: bool,
        fail: bool,
    }

    impl Backend for FakeBackend {
        fn ping(&mut self) -> Result<(), String> {
            self.guard()
        }
        fn insert(&mut self, id: u64, bbox: Bbox, payload: Vec<u8>) -> Result<(), String> {
            self.guard()?;
            self.records.insert(id, (bbox, payload));
            Ok(())
        }
        fn get(&mut self, id: u64) -> Result<Option<Record>, String> {
            self.guard()?;
            Ok(self.records.get(&id).map(|(bbox, payload)| Record {
                bbox: *bbox,
                payload: payload.clone(),
            }))
        }
        fn delete(&mut self, id: u64) -> Result<bool, String> {
            self.guard()?;
            Ok(self.records.remove(&id).is_some())
        }
        fn query_bbox(&mut self, rect: Bbox) -> Result<Vec<u64>, String> {
            self.guard()?;
            let mut ids: Vec<u64> = self
                .records
                .iter()
                .filter(|(_, (bb, _))| bb.intersects(rect))
                .map(|(id, _)| *id)
                .collect();
            ids.sort_unstable();
            Ok(ids)
        }
        fn query_point(&mut self, x: f64, y: f64, tol: f64) -> Result<Vec<u64>, String> {
            self.query_bbox(Bbox::from_corners(x - tol, y - tol, x + tol, y + tol))
        }
        fn list_boxes(&mut self) -> Result<Vec<(u64, Bbox)>, String> {
            self.guard()?;
            let mut out: Vec<(u64, Bbox)> = self
                .records
                .iter()
                .map(|(id, (bb, _))| (*id, *bb))
                .collect();
            out.sort_unstable_by_key(|(id, _)| *id);
            Ok(out)
        }
        fn stats(&mut self) -> Result<Stats, String> {
            self.guard()?;
            Ok(Stats {
                count: self.records.len() as u32,
                cell_size: 1.0,
            })
        }
        fn save(&mut self) -> Result<(), String> {
            self.guard()?;
            self.saved = true;
            Ok(())
        }
    }

    impl FakeBackend {
        fn guard(&self) -> Result<(), String> {
            if self.fail {
                Err("boom".to_string())
            } else {
                Ok(())
            }
        }
    }

    fn req(method: &str, path: &str, query: &[(&str, &str)], body: &[u8]) -> Request {
        Request {
            method: method.to_string(),
            path: path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            query: query
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        }
    }

    #[test]
    fn insert_get_query_delete_flow() {
        let mut b = FakeBackend::default();

        let r = route(
            &req(
                "PUT",
                "/records/1",
                &[("minX", "0"), ("minY", "0"), ("maxX", "10"), ("maxY", "10")],
                b"wall",
            ),
            &mut b,
        );
        assert_eq!(r.status, 200);
        assert!(r.body.contains("\"id\":1"));

        let r = route(&req("GET", "/records/1", &[], b""), &mut b);
        assert_eq!(r.status, 200);
        assert!(r.body.contains("\"payloadText\":\"wall\""));
        assert!(r.body.contains("\"payloadHex\":\"77616c6c\""));

        let r = route(
            &req(
                "GET",
                "/query/bbox",
                &[("minX", "1"), ("minY", "1"), ("maxX", "2"), ("maxY", "2")],
                b"",
            ),
            &mut b,
        );
        assert_eq!(r.body, "{\"ids\":[1]}");

        let r = route(&req("DELETE", "/records/1", &[], b""), &mut b);
        assert_eq!(r.body, "{\"deleted\":true}");
        let r = route(&req("DELETE", "/records/1", &[], b""), &mut b);
        assert_eq!(r.body, "{\"deleted\":false}");
    }

    #[test]
    fn missing_record_is_404() {
        let mut b = FakeBackend::default();
        let r = route(&req("GET", "/records/99", &[], b""), &mut b);
        assert_eq!(r.status, 404);
    }

    #[test]
    fn missing_query_param_is_400() {
        let mut b = FakeBackend::default();
        let r = route(&req("GET", "/query/bbox", &[("minX", "0")], b""), &mut b);
        assert_eq!(r.status, 400);
        assert!(r.body.contains("missing query param"));
    }

    #[test]
    fn wrong_method_is_405() {
        let mut b = FakeBackend::default();
        let r = route(&req("POST", "/stats", &[], b""), &mut b);
        assert_eq!(r.status, 405);
    }

    #[test]
    fn unknown_route_is_404() {
        let mut b = FakeBackend::default();
        let r = route(&req("GET", "/nope", &[], b""), &mut b);
        assert_eq!(r.status, 404);
    }

    #[test]
    fn backend_failure_is_502() {
        let mut b = FakeBackend {
            fail: true,
            ..Default::default()
        };
        let r = route(&req("GET", "/health", &[], b""), &mut b);
        assert_eq!(r.status, 502);
        assert!(r.body.contains("backend"));
    }

    #[test]
    fn save_requires_post() {
        let mut b = FakeBackend::default();
        assert_eq!(route(&req("POST", "/save", &[], b""), &mut b).status, 200);
        assert!(b.saved);
    }
}
