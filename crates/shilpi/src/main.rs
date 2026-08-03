//! `shilpi` — a mongosh-style interactive shell for `shilpid`.
//!
//! ```text
//! shilpi [--host ADDR:PORT] [-e COMMAND]
//! ```
//!
//! With `-e`, runs one command non-interactively and exits (handy for
//! scripting and tests). Without it, reads commands from stdin until
//! `exit`/`quit` or EOF. Run `help` inside the shell for the command list.

mod command;

use std::io::{self, BufRead, Write};

use command::{format_response, parse, Command, HELP_TEXT};
use shilpi_client::Client;

struct Args {
    host: String,
    eval: Option<String>,
}

fn parse_args() -> Args {
    let mut host = "127.0.0.1:7420".to_string();
    let mut eval = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => {
                host = args.next().expect("--host requires an ADDR:PORT value");
            }
            "-e" | "--eval" => {
                eval = Some(args.next().expect("-e requires a command string"));
            }
            "-h" | "--help" => {
                println!("shilpi [--host ADDR:PORT] [-e COMMAND]");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("shilpi {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("shilpi: unrecognized argument '{other}'");
                std::process::exit(2);
            }
        }
    }
    Args { host, eval }
}

/// Run one input line against the connected server. `Ok(false)` means the
/// shell should stop (an `exit` command, or the connection dropped).
fn run_one(client: &mut Client, line: &str) -> Result<bool, shilpi_client::Error> {
    let cmd = match parse(line) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => return Ok(true),
        Err(e) => {
            println!("error: {e}");
            return Ok(true);
        }
    };
    if cmd == Command::Help {
        println!("{HELP_TEXT}");
        return Ok(true);
    }
    if cmd == Command::Exit {
        return Ok(false);
    }
    let req = cmd
        .into_request()
        .expect("non-help, non-exit commands map to a request");
    // Use the raw pass-through so any reply (including a server error) is
    // rendered by `format_response`, exactly as before.
    match client.request(&req) {
        Ok(resp) => println!("{}", format_response(&resp)),
        Err(shilpi_client::Error::Closed) => {
            println!("error: connection closed by server");
            return Ok(false);
        }
        Err(e) => return Err(e),
    }
    Ok(true)
}

fn main() {
    let args = parse_args();
    let mut client = match Client::connect(&args.host) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("shilpi: failed to connect to {}: {e}", args.host);
            std::process::exit(1);
        }
    };

    if let Some(command_line) = args.eval {
        if let Err(e) = run_one(&mut client, &command_line) {
            eprintln!("shilpi: {e}");
            std::process::exit(1);
        }
        return;
    }

    println!(
        "shilpi — connected to {}. Type 'help' for commands.",
        args.host
    );
    let stdin = io::stdin();
    loop {
        print!("shilpi> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        let n = match stdin.lock().read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("shilpi: read error: {e}");
                break;
            }
        };
        if n == 0 {
            println!();
            break; // EOF
        }
        match run_one(&mut client, &line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                eprintln!("shilpi: connection error: {e}");
                break;
            }
        }
    }
}
