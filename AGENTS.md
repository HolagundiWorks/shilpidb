# AGENTS.md

## Cursor Cloud specific instructions

ShilpiDB is a dependency-free Rust workspace (a spatial vector-store engine plus
a MongoDB-shaped stack around it). The whole thing builds and runs with plain
`cargo` — there is no external database, cache, or message broker to stand up.

### Toolchain

The workspace declares `rust-version = "1.90"` (MSRV) in `Cargo.toml`, so a
toolchain `>= 1.90` is required. The startup update script installs and defaults
to `stable` (currently 1.97.x) with the `rustfmt` and `clippy` components. If
`cargo` ever complains that a package requires a newer rustc, run
`rustup default stable` (or `rustup update stable`) to refresh it.

### Services / components

- `shilpid` (`crates/shilpid`) — the server daemon. This is the only long-running
  service. Default bind is `127.0.0.1:7420`, default data file `./shilpidb.vdb`.
- `shilpi` (`crates/shilpi`) — a `mongosh`-style CLI shell (a client, not a
  service). Use `-e "<cmd>"` to run a single command non-interactively.
- `shilpidb`, `shilpi-protocol`, `shilpi-client` — libraries.
- `gui/desktop` (ShilpiDB Desktop) — a WinUI 3 / C# app. Run it with
  `dotnet run --project gui/desktop/src/ShilpiDB.Desktop` after
  `cargo build -p shilpid`. It needs Windows and a display. Protocol tests:
  `dotnet test gui/desktop/ShilpiDB.Desktop.sln`.
- `gui/src-tauri` — the earlier Tauri v2 shell, its own Cargo workspace
  (excluded from the root workspace, so `cargo build --workspace` never touches
  it). Optional. The NSIS installer path still lives there and needs a display
  plus `scripts/prepare-sidecars.ps1`.

### Lint / test / build / run

Standard commands, mirroring `.github/workflows/ci.yml`:

- Lint: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- Test: `cargo test --workspace`
- Build: `cargo build --workspace`
- Run the server: `cargo run -p shilpid -- --bind 127.0.0.1:7420 --data ./mydb.vdb`
- Talk to it: `cargo run -p shilpi -- --host 127.0.0.1:7420 -e "insert 1 0 0 10 10 hello"`

### Gotchas

- Start `shilpid` in a background/tmux session (it blocks). The `shilpi` shell is
  a separate process that connects over TCP — run it in a different shell.
- Config precedence is CLI flag > `SHILPID_*` env var > config file
  (`--config`, `$SHILPID_CONFIG`, or `./shilpid.conf`) > built-in default.
- `.vdb` data files (e.g. `./shilpidb.vdb`, or any `--data` path you pass) are
  **not** gitignored — don't `git add` them.
