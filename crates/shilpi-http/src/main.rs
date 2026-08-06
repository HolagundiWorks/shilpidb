//! `shilpi-http` — an HTTP/JSON gateway in front of `shilpid`.
//!
//! ```text
//! shilpi-http [--bind ADDR:PORT] [--backend ADDR:PORT]
//! ```
//!
//! `--bind` is where the gateway listens for HTTP (default `127.0.0.1:7421`);
//! `--backend` is the `shilpid` server it proxies to (default `127.0.0.1:7420`).
//! Both also read from the environment (`SHILPI_HTTP_BIND`,
//! `SHILPI_HTTP_BACKEND`), with the CLI flag taking precedence.

use std::process::ExitCode;

const DEFAULT_BIND: &str = "127.0.0.1:7421";
const DEFAULT_BACKEND: &str = "127.0.0.1:7420";

struct Args {
    bind: String,
    backend: String,
}

fn parse_args() -> Args {
    let mut bind = std::env::var("SHILPI_HTTP_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
    let mut backend =
        std::env::var("SHILPI_HTTP_BACKEND").unwrap_or_else(|_| DEFAULT_BACKEND.to_string());

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args.next().expect("--bind requires an ADDR:PORT value");
            }
            "--backend" => {
                backend = args.next().expect("--backend requires an ADDR:PORT value");
            }
            "-h" | "--help" => {
                println!("shilpi-http [--bind ADDR:PORT] [--backend ADDR:PORT]");
                println!();
                println!("  --bind     HTTP listen address (default {DEFAULT_BIND})");
                println!("  --backend  shilpid server to proxy to (default {DEFAULT_BACKEND})");
                println!();
                println!("Env: SHILPI_HTTP_BIND, SHILPI_HTTP_BACKEND (flags take precedence).");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("shilpi-http {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("shilpi-http: unrecognized argument '{other}'");
                std::process::exit(2);
            }
        }
    }
    Args { bind, backend }
}

fn main() -> ExitCode {
    let args = parse_args();
    match shilpi_http::serve(&args.bind, &args.backend) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("shilpi-http: {e}");
            ExitCode::FAILURE
        }
    }
}
