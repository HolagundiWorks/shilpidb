//! ShilpiDB Desktop — a Tauri GUI that browses and queries a `shilpid`
//! server. The frontend (`gui/frontend`, plain HTML/CSS/JS, no build step)
//! talks to `shilpid` only through the `#[tauri::command]`s in
//! [`commands`], which in turn speak `shilpi-protocol` over a plain TCP
//! socket held in [`client::ConnectionState`].

mod client;
mod commands;
mod sidecar;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(client::ConnectionState::default())
        .manage(sidecar::SidecarState::default())
        .invoke_handler(tauri::generate_handler![
            commands::connect,
            commands::disconnect,
            commands::is_connected,
            commands::list_boxes,
            commands::get_record,
            commands::insert_record,
            commands::delete_record,
            commands::query_bbox,
            commands::query_point,
            commands::stats,
            commands::save,
            commands::start_local_server,
            commands::stop_local_server,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                sidecar::stop_on_exit(window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running ShilpiDB Desktop");
}
