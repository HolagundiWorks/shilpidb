//! The GUI's connection to a running `shilpid`: a single TCP stream guarded
//! by a mutex, reused across every `#[tauri::command]` call. Deliberately not
//! pooled or async — one GUI window talks to one server at a time, and each
//! round trip is a handful of small messages, so a blocking `Mutex<TcpStream>`
//! is simplest thing that's still correct under Tauri's multi-threaded
//! command dispatch.

use std::net::TcpStream;
use std::sync::Mutex;

use shilpi_protocol::{recv_response, send_request, Request, Response};

#[derive(Default)]
pub struct ConnectionState(pub Mutex<Option<TcpStream>>);

impl ConnectionState {
    pub fn set(&self, stream: TcpStream) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream);
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn is_connected(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// Send `req` on the current connection and return the decoded response.
    pub fn call(&self, req: Request) -> Result<Response, String> {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let stream = guard.as_mut().ok_or("not connected to a shilpid server")?;
        send_request(stream, &req).map_err(|e| e.to_string())?;
        recv_response(stream)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "connection closed by server".to_string())
    }
}
