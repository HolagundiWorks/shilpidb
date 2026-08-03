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
use std::net::TcpStream;

use command::{format_response, parse, Command, HELP_TEXT};
use shilpi_protocol::{recv_response, send_request};

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

fn run_one(stream: &mut TcpStream, line: &str) -> io::Result<bool> {
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
    send_request(stream, &req)?;
    match recv_response(stream)? {
        Some(resp) => println!("{}", format_response(&resp)),
        None => {
            println!("error: connection closed by server");
            return Ok(false);
        }
    }
    Ok(true)
}

fn main() {
    let args = parse_args();
    let mut stream = match TcpStream::connect(&args.host) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shilpi: failed to connect to {}: {e}", args.host);
            std::process::exit(1);
        }
    };
    stream.set_nodelay(true).ok();

    if let Some(command_line) = args.eval {
        if let Err(e) = run_one(&mut stream, &command_line) {
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
        match run_one(&mut stream, &line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                eprintln!("shilpi: connection error: {e}");
                break;
            }
        }
    }
}
