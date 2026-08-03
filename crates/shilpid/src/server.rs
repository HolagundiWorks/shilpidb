//! The TCP accept loop: one thread per connection, each request handled
//! against a shared, mutex-guarded [`Database`].

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use shilpi_protocol::{recv_request, send_response, Request, Response};
use shilpidb::Point;

use crate::db::Database;

pub struct Server {
    db: Arc<Mutex<Database>>,
    data_path: PathBuf,
}

impl Server {
    pub fn new(db: Database, data_path: PathBuf) -> Self {
        Server {
            db: Arc::new(Mutex::new(db)),
            data_path,
        }
    }

    /// Bind and serve forever, logging each connection to stderr.
    ///
    /// `bind_addr` may use port `0` to let the OS pick a free port — the
    /// actually-bound address (which callers like tests need) is printed to
    /// stderr as `shilpid listening on <addr>`.
    pub fn run(&self, bind_addr: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(bind_addr)?;
        eprintln!("shilpid listening on {}", listener.local_addr()?);
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("accept error: {e}");
                    continue;
                }
            };
            let db = Arc::clone(&self.db);
            let data_path = self.data_path.clone();
            thread::spawn(move || {
                if let Err(e) = handle_connection(stream, db, data_path) {
                    eprintln!("connection error: {e}");
                }
            });
        }
        Ok(())
    }
}

fn handle_connection(
    mut stream: TcpStream,
    db: Arc<Mutex<Database>>,
    data_path: PathBuf,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = stream.try_clone()?;
    loop {
        let req = match recv_request(&mut reader)? {
            Some(req) => req,
            None => return Ok(()), // client closed the connection
        };
        let resp = handle_request(&db, &data_path, req);
        send_response(&mut stream, &resp)?;
    }
}

fn handle_request(db: &Arc<Mutex<Database>>, data_path: &Path, req: Request) -> Response {
    let mut db = match db.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    match req {
        Request::Ping => Response::Pong,
        Request::Insert { id, bbox, payload } => {
            db.insert(id, bbox, payload);
            Response::Ok
        }
        Request::Get { id } => match db.get(id) {
            Some((bbox, payload)) => Response::Found {
                bbox: *bbox,
                payload: payload.clone(),
            },
            None => Response::NotFound,
        },
        Request::Delete { id } => Response::Deleted(db.delete(id)),
        Request::QueryBbox { rect } => Response::Ids(db.query_bbox(rect)),
        Request::QueryPoint { x, y, tol } => Response::Ids(db.query_point(Point::new(x, y), tol)),
        Request::List => Response::Ids(db.list_ids()),
        Request::ListBoxes => Response::Records(db.list_boxes()),
        Request::Stats => Response::Stats {
            count: db.len() as u32,
            cell_size: db.cell_size(),
        },
        Request::Save => match db.save(data_path) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Err(e.to_string()),
        },
    }
}
