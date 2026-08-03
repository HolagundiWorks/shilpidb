//! Layered configuration for `shilpid`.
//!
//! Sources overlay each other, lowest precedence first:
//!
//! 1. built-in [`Config::default`]
//! 2. an optional config file (a tiny TOML subset — see [`apply_file_str`])
//! 3. `SHILPID_*` environment variables
//! 4. command-line flags
//!
//! So a flag beats an env var beats the file beats the default — the same
//! precedence `mongod --config` uses. The file location comes from
//! `--config PATH`, else `$SHILPID_CONFIG`, else `./shilpid.conf` if it exists.
//!
//! Everything here is std-only and hand-rolled: no `serde`, no `toml` crate,
//! matching the engine's dependency-free stance.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use shilpidb::GridConfig;

pub const DEFAULT_BIND: &str = "127.0.0.1:7420";
pub const DEFAULT_DATA: &str = "./shilpidb.vdb";
pub const DEFAULT_CONFIG: &str = "./shilpid.conf";

pub const USAGE: &str = "\
shilpid — the ShilpiDB server

USAGE:
    shilpid [OPTIONS]

OPTIONS:
    --config PATH             Load a config file (TOML subset). Default: ./shilpid.conf if present.
    --bind ADDR:PORT          Listen address                    [default: 127.0.0.1:7420]
    --data PATH               Database file                     [default: ./shilpidb.vdb]
    --autosave-secs N         Flush to disk every N seconds     [default: 0 = disabled]
    --max-connections N       Cap concurrent connections        [default: 0 = unlimited]
    --read-only[=BOOL]        Reject inserts/deletes            [default: false]
    --log-level LEVEL         error | warn | info | debug       [default: info]
    --cell-size F             Fixed spatial-grid cell size      [default: 0 = auto]
    --max-cells-per-entity N  Grid overflow threshold           [default: 256]
    --max-query-cells N       Huge-query fallback threshold     [default: 8192]
    -h, --help                Print this help and exit
    -V, --version             Print version and exit

ENVIRONMENT (override the file, are overridden by flags):
    SHILPID_CONFIG, SHILPID_BIND, SHILPID_DATA, SHILPID_AUTOSAVE_SECS,
    SHILPID_MAX_CONNECTIONS, SHILPID_READ_ONLY, SHILPID_LOG_LEVEL,
    SHILPID_CELL_SIZE, SHILPID_MAX_CELLS_PER_ENTITY, SHILPID_MAX_QUERY_CELLS";

// --- Log level --------------------------------------------------------------

/// Verbosity, ordered so `level <= current` means "emit".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }

    pub fn parse(s: &str) -> Result<LogLevel, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" | "err" => Ok(LogLevel::Error),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" | "dbg" => Ok(LogLevel::Debug),
            other => Err(format!("invalid log level '{other}'")),
        }
    }
}

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

/// Install the process-wide log level (call once, from `main`).
pub fn set_log_level(level: LogLevel) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Whether a message at `level` should be emitted under the current setting.
pub fn log_enabled(level: LogLevel) -> bool {
    (level as u8) <= LOG_LEVEL.load(Ordering::Relaxed)
}

/// Emit a log line to stderr if `level` is enabled.
pub fn log(level: LogLevel, msg: &str) {
    if log_enabled(level) {
        eprintln!("shilpid [{}] {msg}", level.as_str());
    }
}

// --- Config -----------------------------------------------------------------

/// The fully-resolved server configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub bind: String,
    pub data: PathBuf,
    /// Flush to disk every N seconds in the background; `0` disables it.
    pub autosave_secs: u64,
    /// Maximum concurrent client connections; `0` is unlimited.
    pub max_connections: usize,
    /// When set, the server rejects inserts and deletes.
    pub read_only: bool,
    pub log_level: LogLevel,
    /// Spatial-index tunables passed to `SpatialGrid::build_with_config`.
    pub grid: GridConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: DEFAULT_BIND.to_string(),
            data: PathBuf::from(DEFAULT_DATA),
            autosave_secs: 0,
            max_connections: 0,
            read_only: false,
            log_level: LogLevel::Info,
            grid: GridConfig::default(),
        }
    }
}

impl Config {
    /// Resolve the configuration from real argv / environment / files, exiting
    /// the process on `--help`, `--version`, or a fatal config error.
    pub fn load() -> Config {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match Self::resolve(&args) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("shilpid: {e}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    /// Resolve from an explicit arg list (env is read live). `--help` /
    /// `--version` short-circuit by printing and exiting.
    fn resolve(args: &[String]) -> Result<Config, String> {
        let cli = Cli::parse(args)?;
        if cli.help {
            println!("{USAGE}");
            std::process::exit(0);
        }
        if cli.version {
            println!("shilpid {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        let mut cfg = Config::default();

        // Locate the config file: CLI flag, then env, then a default path.
        let (path, explicit) = match cli
            .config
            .clone()
            .or_else(|| std::env::var("SHILPID_CONFIG").ok().map(PathBuf::from))
        {
            Some(p) => (Some(p), true),
            None => {
                let def = PathBuf::from(DEFAULT_CONFIG);
                (def.exists().then_some(def), false)
            }
        };
        if let Some(path) = path {
            match std::fs::read_to_string(&path) {
                Ok(text) => apply_file_str(&mut cfg, &text)
                    .map_err(|e| format!("config file '{}': {e}", path.display()))?,
                Err(e) if explicit => {
                    return Err(format!("cannot read config file '{}': {e}", path.display()))
                }
                Err(_) => {} // default path vanished between exists() and read — ignore
            }
        }

        apply_env(&mut cfg)?;
        cli.apply(&mut cfg);
        Ok(cfg)
    }
}

// --- File parsing (TOML subset) --------------------------------------------

/// Apply a config file's text to `cfg`. The grammar is intentionally tiny:
///
/// - `key = value` lines; whitespace around `=` is ignored.
/// - `[section]` headers — only `[grid]` is recognized (top-level keys need
///   no header).
/// - `#` or `;` start a comment (outside quotes).
/// - values may be bare or wrapped in `"..."`.
pub fn apply_file_str(cfg: &mut Config, text: &str) -> Result<(), String> {
    let mut section = String::new();
    for (i, raw) in text.lines().enumerate() {
        let line = strip_comment(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = inner.trim().to_ascii_lowercase();
            continue;
        }
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected 'key = value'", i + 1))?;
        let key = key.trim().to_ascii_lowercase();
        let val = unquote(val.trim());
        apply_kv(cfg, &section, &key, val).map_err(|e| format!("line {}: {e}", i + 1))?;
    }
    Ok(())
}

/// Drop a trailing `#`/`;` comment, respecting a double-quoted span.
fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_quotes = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                out.push(c);
            }
            '#' | ';' if !in_quotes => break,
            _ => out.push(c),
        }
    }
    out
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

/// Apply one `section` + `key = val` pair. Unknown keys/sections are errors so
/// typos surface instead of being silently ignored.
fn apply_kv(cfg: &mut Config, section: &str, key: &str, val: &str) -> Result<(), String> {
    match (section, key) {
        ("", "bind") => cfg.bind = val.to_string(),
        ("", "data") => cfg.data = PathBuf::from(val),
        ("", "autosave_secs") => cfg.autosave_secs = parse_num(val, "autosave_secs")?,
        ("", "max_connections") => cfg.max_connections = parse_num(val, "max_connections")?,
        ("", "read_only") => cfg.read_only = parse_bool(val)?,
        ("", "log_level") => cfg.log_level = LogLevel::parse(val)?,
        ("grid", "cell_size") => cfg.grid.cell = cell_from(parse_num(val, "cell_size")?),
        ("grid", "max_cells_per_entity") => {
            cfg.grid.max_cells_per_entity = parse_num(val, "max_cells_per_entity")?
        }
        ("grid", "max_query_cells") => {
            cfg.grid.max_query_cells = parse_num(val, "max_query_cells")?
        }
        ("", k) => return Err(format!("unknown key '{k}'")),
        (s, k) => return Err(format!("unknown key '[{s}] {k}'")),
    }
    Ok(())
}

// --- Environment ------------------------------------------------------------

fn apply_env(cfg: &mut Config) -> Result<(), String> {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|s| !s.is_empty())
    }
    let env = |name: &str| var(name);

    if let Some(v) = env("SHILPID_BIND") {
        cfg.bind = v;
    }
    if let Some(v) = env("SHILPID_DATA") {
        cfg.data = PathBuf::from(v);
    }
    if let Some(v) = env("SHILPID_AUTOSAVE_SECS") {
        cfg.autosave_secs = parse_num(&v, "SHILPID_AUTOSAVE_SECS")?;
    }
    if let Some(v) = env("SHILPID_MAX_CONNECTIONS") {
        cfg.max_connections = parse_num(&v, "SHILPID_MAX_CONNECTIONS")?;
    }
    if let Some(v) = env("SHILPID_READ_ONLY") {
        cfg.read_only = parse_bool(&v)?;
    }
    if let Some(v) = env("SHILPID_LOG_LEVEL") {
        cfg.log_level = LogLevel::parse(&v)?;
    }
    if let Some(v) = env("SHILPID_CELL_SIZE") {
        cfg.grid.cell = cell_from(parse_num(&v, "SHILPID_CELL_SIZE")?);
    }
    if let Some(v) = env("SHILPID_MAX_CELLS_PER_ENTITY") {
        cfg.grid.max_cells_per_entity = parse_num(&v, "SHILPID_MAX_CELLS_PER_ENTITY")?;
    }
    if let Some(v) = env("SHILPID_MAX_QUERY_CELLS") {
        cfg.grid.max_query_cells = parse_num(&v, "SHILPID_MAX_QUERY_CELLS")?;
    }
    Ok(())
}

// --- CLI --------------------------------------------------------------------

/// Parsed command-line overrides. Every field is optional so it can layer on
/// top of the file/env without clobbering unspecified values.
#[derive(Debug, Default, PartialEq)]
struct Cli {
    help: bool,
    version: bool,
    config: Option<PathBuf>,
    bind: Option<String>,
    data: Option<PathBuf>,
    autosave_secs: Option<u64>,
    max_connections: Option<usize>,
    read_only: Option<bool>,
    log_level: Option<LogLevel>,
    cell_size: Option<f64>,
    max_cells_per_entity: Option<i64>,
    max_query_cells: Option<i64>,
}

impl Cli {
    fn parse(args: &[String]) -> Result<Cli, String> {
        let mut cli = Cli::default();
        let mut it = args.iter();
        // Accept both `--flag value` and `--flag=value`.
        while let Some(arg) = it.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (arg.as_str(), None),
            };
            let mut value = |what: &str| -> Result<String, String> {
                if let Some(v) = inline.clone() {
                    Ok(v)
                } else {
                    it.next()
                        .cloned()
                        .ok_or_else(|| format!("{flag} requires a {what} value"))
                }
            };
            match flag {
                "-h" | "--help" => cli.help = true,
                "-V" | "--version" => cli.version = true,
                "--config" => cli.config = Some(PathBuf::from(value("PATH")?)),
                "--bind" => cli.bind = Some(value("ADDR:PORT")?),
                "--data" => cli.data = Some(PathBuf::from(value("PATH")?)),
                "--autosave-secs" => {
                    cli.autosave_secs = Some(parse_num(&value("N")?, "--autosave-secs")?)
                }
                "--max-connections" => {
                    cli.max_connections = Some(parse_num(&value("N")?, "--max-connections")?)
                }
                // Bare `--read-only` means true; `--read-only=false` also works.
                "--read-only" => {
                    cli.read_only = Some(match inline {
                        Some(v) => parse_bool(&v)?,
                        None => true,
                    })
                }
                "--log-level" => cli.log_level = Some(LogLevel::parse(&value("LEVEL")?)?),
                "--cell-size" => cli.cell_size = Some(parse_num(&value("F")?, "--cell-size")?),
                "--max-cells-per-entity" => {
                    cli.max_cells_per_entity =
                        Some(parse_num(&value("N")?, "--max-cells-per-entity")?)
                }
                "--max-query-cells" => {
                    cli.max_query_cells = Some(parse_num(&value("N")?, "--max-query-cells")?)
                }
                other => return Err(format!("unrecognized argument '{other}'")),
            }
        }
        Ok(cli)
    }

    fn apply(&self, cfg: &mut Config) {
        if let Some(v) = &self.bind {
            cfg.bind = v.clone();
        }
        if let Some(v) = &self.data {
            cfg.data = v.clone();
        }
        if let Some(v) = self.autosave_secs {
            cfg.autosave_secs = v;
        }
        if let Some(v) = self.max_connections {
            cfg.max_connections = v;
        }
        if let Some(v) = self.read_only {
            cfg.read_only = v;
        }
        if let Some(v) = self.log_level {
            cfg.log_level = v;
        }
        if let Some(v) = self.cell_size {
            cfg.grid.cell = cell_from(v);
        }
        if let Some(v) = self.max_cells_per_entity {
            cfg.grid.max_cells_per_entity = v;
        }
        if let Some(v) = self.max_query_cells {
            cfg.grid.max_query_cells = v;
        }
    }
}

// --- Value parsing ----------------------------------------------------------

fn parse_num<T: std::str::FromStr>(val: &str, what: &str) -> Result<T, String> {
    val.trim()
        .parse::<T>()
        .map_err(|_| format!("'{what}' has an invalid value '{val}'"))
}

fn parse_bool(val: &str) -> Result<bool, String> {
    match val.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(format!("expected a boolean, got '{other}'")),
    }
}

/// A configured cell size: positive & finite means "fixed", anything else
/// (0, negative, NaN) means "auto-choose".
fn cell_from(v: f64) -> Option<f64> {
    (v.is_finite() && v > 0.0).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic layering for tests: default → file → CLI (env skipped).
    fn resolve(file: &str, args: &[&str]) -> Result<Config, String> {
        let mut cfg = Config::default();
        apply_file_str(&mut cfg, file)?;
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        Cli::parse(&owned)?.apply(&mut cfg);
        Ok(cfg)
    }

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.bind, DEFAULT_BIND);
        assert_eq!(c.data, PathBuf::from(DEFAULT_DATA));
        assert_eq!(c.autosave_secs, 0);
        assert_eq!(c.max_connections, 0);
        assert!(!c.read_only);
        assert_eq!(c.log_level, LogLevel::Info);
        assert_eq!(c.grid, GridConfig::default());
    }

    #[test]
    fn full_config_file_parses() {
        let file = "\
# server
bind = 0.0.0.0:9000
data = \"/var/lib/shilpi/data.vdb\"
autosave_secs = 30
max_connections = 64
read_only = yes
log_level = debug

[grid]
cell_size = 2.5   ; fixed
max_cells_per_entity = 128
max_query_cells = 4096
";
        let c = resolve(file, &[]).unwrap();
        assert_eq!(c.bind, "0.0.0.0:9000");
        assert_eq!(c.data, PathBuf::from("/var/lib/shilpi/data.vdb"));
        assert_eq!(c.autosave_secs, 30);
        assert_eq!(c.max_connections, 64);
        assert!(c.read_only);
        assert_eq!(c.log_level, LogLevel::Debug);
        assert_eq!(c.grid.cell, Some(2.5));
        assert_eq!(c.grid.max_cells_per_entity, 128);
        assert_eq!(c.grid.max_query_cells, 4096);
    }

    #[test]
    fn cli_overrides_file() {
        let file = "bind = 127.0.0.1:1111\nlog_level = warn\n";
        let c = resolve(file, &["--bind", "127.0.0.1:2222", "--log-level=debug"]).unwrap();
        assert_eq!(c.bind, "127.0.0.1:2222");
        assert_eq!(c.log_level, LogLevel::Debug);
    }

    #[test]
    fn read_only_flag_forms() {
        assert!(resolve("", &["--read-only"]).unwrap().read_only);
        assert!(resolve("", &["--read-only=true"]).unwrap().read_only);
        assert!(!resolve("", &["--read-only=false"]).unwrap().read_only);
        assert!(resolve("read_only = on", &[]).unwrap().read_only);
    }

    #[test]
    fn cell_size_zero_or_negative_is_auto() {
        assert_eq!(
            resolve("[grid]\ncell_size = 0", &[]).unwrap().grid.cell,
            None
        );
        assert_eq!(
            resolve("[grid]\ncell_size = -3", &[]).unwrap().grid.cell,
            None
        );
        assert_eq!(
            resolve("", &["--cell-size", "5"]).unwrap().grid.cell,
            Some(5.0)
        );
    }

    #[test]
    fn unknown_key_and_section_error() {
        assert!(apply_file_str(&mut Config::default(), "nope = 1").is_err());
        assert!(apply_file_str(&mut Config::default(), "[bogus]\nx = 1").is_err());
    }

    #[test]
    fn bad_values_error() {
        assert!(apply_file_str(&mut Config::default(), "autosave_secs = later").is_err());
        assert!(apply_file_str(&mut Config::default(), "log_level = loud").is_err());
        assert!(Cli::parse(&["--bind".to_string()]).is_err()); // missing value
        assert!(Cli::parse(&["--frobnicate".to_string()]).is_err()); // unknown flag
    }

    #[test]
    fn comments_and_quotes() {
        let c = resolve("bind = \"1.2.3.4:5\"  # inline comment\n", &[]).unwrap();
        assert_eq!(c.bind, "1.2.3.4:5");
    }
}
