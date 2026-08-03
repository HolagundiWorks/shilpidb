//! The TCP accept loop: one thread per connection, each request handled
//! against a shared, mutex-guarded [`Database`]. Behaviour (bind address,
//! autosave, connection cap, read-only, log level) comes from [`Config`].

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use shilpi_protocol::{recv_request, send_response, Request, Response};
use shilpidb::Point;

use crate::config::{self, Config, LogLevel};
use crate::db::Database;

pub struct Server {
    db: Arc<Mutex<Database>>,
    data_path: PathBuf,
    bind: String,
    autosave_secs: u64,
    max_connections: usize,
    read_only: bool,
}

impl Server {
    /// Build a server from an already-loaded [`Database`] and the resolved
    /// [`Config`] (which supplies the bind address, data path, and tunables).
    pub fn new(db: Database, config: &Config) -> Self {
        Server {
            db: Arc::new(Mutex::new(db)),
            data_path: config.data.clone(),
            bind: config.bind.clone(),
            autosave_secs: config.autosave_secs,
            max_connections: config.max_connections,
            read_only: config.read_only,
        }
    }

    /// Bind and serve forever.
    ///
    /// The configured bind address may use port `0` to let the OS pick a free
    /// port — the actually-bound address (which callers like tests need) is
    /// printed to stderr as `shilpid listening on <addr>` regardless of the
    /// log level.
    pub fn run(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(&self.bind)?;
        // Always emitted (not log-gated): tooling and tests rely on this line.
        eprintln!("shilpid listening on {}", listener.local_addr()?);
        if self.read_only {
            config::log(
                LogLevel::Info,
                "read-only mode: inserts and deletes are rejected",
            );
        }
        self.spawn_autosave();

        let active = Arc::new(AtomicUsize::new(0));
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    config::log(LogLevel::Warn, &format!("accept error: {e}"));
                    continue;
                }
            };
            let guard = match ConnGuard::try_admit(&active, self.max_connections) {
                Some(g) => g,
                None => {
                    config::log(
                        LogLevel::Warn,
                        &format!(
                            "connection refused: at max_connections={}",
                            self.max_connections
                        ),
                    );
                    drop(stream); // close immediately
                    continue;
                }
            };
            let db = Arc::clone(&self.db);
            let data_path = self.data_path.clone();
            let read_only = self.read_only;
            thread::spawn(move || {
                let _guard = guard; // released (counter--) when the thread ends
                if let Err(e) = handle_connection(stream, db, data_path, read_only) {
                    config::log(LogLevel::Debug, &format!("connection closed: {e}"));
                }
            });
        }
        Ok(())
    }

    /// Spawn a background thread that flushes the database to disk every
    /// `autosave_secs`. A value of `0` disables it (save only on request /
    /// shutdown).
    fn spawn_autosave(&self) {
        if self.autosave_secs == 0 {
            return;
        }
        let db = Arc::clone(&self.db);
        let data_path = self.data_path.clone();
        let period = Duration::from_secs(self.autosave_secs);
        config::log(
            LogLevel::Info,
            &format!("autosave every {}s", self.autosave_secs),
        );
        thread::spawn(move || loop {
            thread::sleep(period);
            let db = match db.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            match db.save(&data_path) {
                Ok(()) => config::log(
                    LogLevel::Debug,
                    &format!(
                        "autosaved {} record(s) to '{}'",
                        db.len(),
                        data_path.display()
                    ),
                ),
                Err(e) => config::log(LogLevel::Warn, &format!("autosave failed: {e}")),
            }
        });
    }
}

/// Reserves a slot in the active-connection counter and releases it on drop
/// (even on panic), so `max_connections` can't leak slots.
struct ConnGuard(Arc<AtomicUsize>);

impl ConnGuard {
    /// Atomically claim a slot, or return `None` if the cap (non-zero) is
    /// already reached. Increment-then-check avoids the check-then-increment
    /// race that could over-admit under a connection burst.
    fn try_admit(counter: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        let prev = counter.fetch_add(1, Ordering::SeqCst);
        if max != 0 && prev >= max {
            counter.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(ConnGuard(Arc::clone(counter)))
        }
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn handle_connection(
    mut stream: TcpStream,
    db: Arc<Mutex<Database>>,
    data_path: PathBuf,
    read_only: bool,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = stream.try_clone()?;
    loop {
        let req = match recv_request(&mut reader)? {
            Some(req) => req,
            None => return Ok(()), // client closed the connection
        };
        let resp = handle_request(&db, &data_path, read_only, req);
        send_response(&mut stream, &resp)?;
    }
}

fn handle_request(
    db: &Arc<Mutex<Database>>,
    data_path: &Path,
    read_only: bool,
    req: Request,
) -> Response {
    if read_only && matches!(req, Request::Insert { .. } | Request::Delete { .. }) {
        return Response::Err("server is read-only".to_string());
    }
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
