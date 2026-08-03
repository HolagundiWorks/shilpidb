//! `#[tauri::command]` handlers the frontend calls via `invoke(...)`. Each one
//! is a thin translation between JS-friendly JSON shapes and a
//! `shilpi_protocol::Request`/`Response` round trip through [`ConnectionState`].

use std::net::TcpStream;
use std::time::Duration;

use serde::Serialize;
use shilpi_protocol::Request;
use shilpidb::Bbox;
use tauri::{AppHandle, State};

use crate::client::ConnectionState;
use crate::sidecar::SidecarState;

#[derive(Serialize, Clone)]
pub struct RecordBox {
    pub id: u64,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl From<(u64, Bbox)> for RecordBox {
    fn from((id, bbox): (u64, Bbox)) -> Self {
        RecordBox {
            id,
            min_x: bbox.min.x,
            min_y: bbox.min.y,
            max_x: bbox.max.x,
            max_y: bbox.max.y,
        }
    }
}

#[derive(Serialize)]
pub struct RecordDetail {
    pub id: u64,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub payload_text: String,
    pub payload_hex: String,
}

#[derive(Serialize)]
pub struct StatsInfo {
    pub count: u32,
    pub cell_size: f64,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn rect(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Bbox {
    Bbox::from_corners(min_x, min_y, max_x, max_y)
}

#[tauri::command]
pub fn connect(state: State<ConnectionState>, host: String) -> Result<(), String> {
    let stream = TcpStream::connect(&host).map_err(|e| format!("connect to {host}: {e}"))?;
    stream
        .set_nodelay(true)
        .map_err(|e| format!("connect to {host}: {e}"))?;
    state.set(stream);
    match state.call(Request::Ping) {
        Ok(shilpi_protocol::Response::Pong) => Ok(()),
        Ok(other) => {
            state.clear();
            Err(format!("unexpected reply to ping: {other:?}"))
        }
        Err(e) => {
            state.clear();
            Err(e)
        }
    }
}

#[tauri::command]
pub fn disconnect(state: State<ConnectionState>) {
    state.clear();
}

#[tauri::command]
pub fn is_connected(state: State<ConnectionState>) -> bool {
    state.is_connected()
}

#[tauri::command]
pub fn list_boxes(state: State<ConnectionState>) -> Result<Vec<RecordBox>, String> {
    match state.call(Request::ListBoxes)? {
        shilpi_protocol::Response::Records(records) => {
            Ok(records.into_iter().map(RecordBox::from).collect())
        }
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn get_record(state: State<ConnectionState>, id: u64) -> Result<Option<RecordDetail>, String> {
    match state.call(Request::Get { id })? {
        shilpi_protocol::Response::Found { bbox, payload } => Ok(Some(RecordDetail {
            id,
            min_x: bbox.min.x,
            min_y: bbox.min.y,
            max_x: bbox.max.x,
            max_y: bbox.max.y,
            payload_text: String::from_utf8_lossy(&payload).to_string(),
            payload_hex: to_hex(&payload),
        })),
        shilpi_protocol::Response::NotFound => Ok(None),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn insert_record(
    state: State<ConnectionState>,
    id: u64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    payload: String,
) -> Result<(), String> {
    match state.call(Request::Insert {
        id,
        bbox: rect(min_x, min_y, max_x, max_y),
        payload: payload.into_bytes(),
    })? {
        shilpi_protocol::Response::Ok => Ok(()),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn delete_record(state: State<ConnectionState>, id: u64) -> Result<bool, String> {
    match state.call(Request::Delete { id })? {
        shilpi_protocol::Response::Deleted(existed) => Ok(existed),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn query_bbox(
    state: State<ConnectionState>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Result<Vec<u64>, String> {
    match state.call(Request::QueryBbox {
        rect: rect(min_x, min_y, max_x, max_y),
    })? {
        shilpi_protocol::Response::Ids(ids) => Ok(ids),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn query_point(
    state: State<ConnectionState>,
    x: f64,
    y: f64,
    tol: f64,
) -> Result<Vec<u64>, String> {
    match state.call(Request::QueryPoint { x, y, tol })? {
        shilpi_protocol::Response::Ids(ids) => Ok(ids),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn stats(state: State<ConnectionState>) -> Result<StatsInfo, String> {
    match state.call(Request::Stats)? {
        shilpi_protocol::Response::Stats { count, cell_size } => Ok(StatsInfo { count, cell_size }),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub fn save(state: State<ConnectionState>) -> Result<(), String> {
    match state.call(Request::Save)? {
        shilpi_protocol::Response::Ok => Ok(()),
        shilpi_protocol::Response::Err(msg) => Err(msg),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

/// Launch the bundled `shilpid` sidecar on an OS-assigned port and return the
/// address it bound. Convenience only — connecting to a `shilpid` started any
/// other way (Start Menu shortcut, a remote host) works the same via
/// [`connect`].
#[tauri::command]
pub async fn start_local_server(
    app: AppHandle,
    sidecars: State<'_, SidecarState>,
    data_path: Option<String>,
) -> Result<String, String> {
    crate::sidecar::start(&app, &sidecars, data_path, Duration::from_secs(5)).await
}

#[tauri::command]
pub fn stop_local_server(sidecars: State<SidecarState>) {
    sidecars.stop();
}
