//! `shilpid` — the ShilpiDB server. Listens for `shilpi-protocol` clients
//! over TCP and serves an in-memory, file-backed spatial record store.
//!
//! Run `shilpid --help` for the full option list (bind address, data file,
//! autosave, connection cap, read-only mode, log level, spatial-grid
//! tunables) — see [`config`] for the config-file/env/flag layering, and
//! `--config PATH` / `./shilpid.conf` for file-based configuration.
//!
//! The data file is loaded at startup if present, and flushed on an
//! explicit `Save` request (the CLI shell's `db.save()`), the configured
//! autosave interval, or a clean shutdown.

mod config;
mod db;
mod server;

use config::{Config, LogLevel};
use db::Database;
use server::Server;

fn main() {
    let cfg = Config::load();
    config::set_log_level(cfg.log_level);

    let mut db = match Database::load(&cfg.data) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("shilpid: failed to load '{}': {e}", cfg.data.display());
            std::process::exit(1);
        }
    };
    db.set_grid_config(cfg.grid);
    config::log(
        LogLevel::Info,
        &format!(
            "loaded {} record(s) from '{}'",
            db.len(),
            cfg.data.display()
        ),
    );

    let server = Server::new(db, &cfg);
    if let Err(e) = server.run() {
        eprintln!("shilpid: server error: {e}");
        std::process::exit(1);
    }
}
