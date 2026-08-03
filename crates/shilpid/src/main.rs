//! `shilpid` — the ShilpiDB server. Listens for `shilpi-protocol` clients
//! over TCP and serves an in-memory, file-backed spatial record store.
//!
//! ```text
//! shilpid [--bind ADDR:PORT] [--data PATH]
//! ```
//!
//! Defaults: `--bind 127.0.0.1:7420 --data ./shilpidb.vdb`. The data file is
//! loaded at startup if present, and flushed on an explicit `Save` request
//! (the CLI shell's `db.save()`) or on a clean Ctrl-C shutdown.

mod db;
mod server;

use std::path::PathBuf;

use db::Database;
use server::Server;

struct Args {
    bind: String,
    data: PathBuf,
}

fn parse_args() -> Args {
    let mut bind = "127.0.0.1:7420".to_string();
    let mut data = PathBuf::from("./shilpidb.vdb");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args.next().expect("--bind requires an ADDR:PORT value");
            }
            "--data" => {
                data = PathBuf::from(args.next().expect("--data requires a PATH value"));
            }
            "-h" | "--help" => {
                println!("shilpid [--bind ADDR:PORT] [--data PATH]");
                std::process::exit(0);
            }
            other => {
                eprintln!("shilpid: unrecognized argument '{other}'");
                std::process::exit(2);
            }
        }
    }
    Args { bind, data }
}

fn main() {
    let args = parse_args();

    let db = match Database::load(&args.data) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("shilpid: failed to load '{}': {e}", args.data.display());
            std::process::exit(1);
        }
    };
    eprintln!(
        "shilpid: loaded {} record(s) from '{}'",
        db.len(),
        args.data.display()
    );

    let server = Server::new(db, args.data);
    if let Err(e) = server.run(&args.bind) {
        eprintln!("shilpid: server error: {e}");
        std::process::exit(1);
    }
}
