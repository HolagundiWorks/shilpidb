//! Optional convenience: launch the bundled `shilpid` sidecar binary instead
//! of requiring the user to have started a server another way. The sidecar's
//! own stderr ("shilpid listening on <addr>") is how we learn the bound
//! address — same line format `crates/shilpid`'s own tests parse.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

#[derive(Default)]
pub struct SidecarState(Mutex<Option<CommandChild>>);

impl SidecarState {
    /// Kill the sidecar if one is running (app shutdown or an explicit stop).
    pub fn stop(&self) {
        if let Some(child) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
        }
    }
}

pub async fn start(
    app: &AppHandle,
    sidecars: &SidecarState,
    data_path: Option<String>,
    timeout: Duration,
) -> Result<String, String> {
    // Only one bundled server per GUI instance — replace, don't accumulate.
    sidecars.stop();

    let mut args = vec!["--bind".to_string(), "127.0.0.1:0".to_string()];
    if let Some(path) = data_path {
        args.push("--data".to_string());
        args.push(path);
    }

    let sidecar = app
        .shell()
        .sidecar("shilpid")
        .map_err(|e| format!("shilpid sidecar not available: {e}"))?;
    let (mut rx, child) = sidecar
        .args(args)
        .spawn()
        .map_err(|e| format!("failed to start shilpid: {e}"))?;

    let wait_for_addr = async {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(line) => {
                    let line = String::from_utf8_lossy(&line);
                    if let Some(addr) = line.trim().strip_prefix("shilpid listening on ") {
                        return Ok(addr.to_string());
                    }
                }
                CommandEvent::Error(e) => return Err(format!("shilpid error: {e}")),
                CommandEvent::Terminated(status) => {
                    return Err(format!(
                        "shilpid exited before it started listening (code {:?})",
                        status.code
                    ))
                }
                _ => {}
            }
        }
        Err("shilpid closed its output before it started listening".to_string())
    };

    let addr = match tokio::time::timeout(timeout, wait_for_addr).await {
        Ok(Ok(addr)) => addr,
        Ok(Err(e)) => {
            let _ = child.kill();
            return Err(e);
        }
        Err(_) => {
            let _ = child.kill();
            return Err("timed out waiting for shilpid to start".to_string());
        }
    };

    *sidecars.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok(addr)
}

/// Make sure the sidecar doesn't outlive the GUI window.
pub fn stop_on_exit(app: &AppHandle) {
    if let Some(state) = app.try_state::<SidecarState>() {
        state.stop();
    }
}
