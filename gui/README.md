# ShilpiDB Desktop

A [Tauri](https://tauri.app) GUI for browsing and querying a `shilpid`
server — the Compass equivalent for ShilpiDB. Plain HTML/CSS/JS frontend
(`frontend/`, no npm build step), Rust backend (`src-tauri/`) that talks to
`shilpid` over `shilpi-protocol` on a plain TCP socket.

This is its own Cargo workspace, separate from the root one — see the
`exclude` note in the repo's top-level `Cargo.toml` for why.

## Run it in development

`shilpid` and `shilpi` ship as bundled sidecar binaries, but Tauri doesn't
build them for you — stage them first:

```powershell
powershell -File ../scripts/prepare-sidecars.ps1
```

Then, from `gui/src-tauri`:

```bash
cargo install tauri-cli --version "^2" --locked   # once
cargo tauri dev
```

This opens the app window pointed at the static `frontend/` folder with hot
reload on file save (no bundler in the loop — it's plain files).

## What the GUI can do

- **Connect** to any running `shilpid` at `host:port`, or click **Start
  local server** to launch the bundled `shilpid` sidecar itself (an
  OS-assigned port, spawned and torn down with the GUI window).
- Browse all records (id + bounding box) in a table and on a spatial canvas.
- Run a bbox query and see matches highlighted in both views.
- Insert/replace a record, inspect one (payload as text and hex), delete it.
- Flush the in-memory database to disk (`Save`).

## Build the installer

```bash
powershell -File ../scripts/prepare-sidecars.ps1 -Release
cargo tauri build
```

Produces a Windows NSIS installer (see `src-tauri/tauri.conf.json`'s
`bundle` section) bundling the GUI plus the `shilpid` and `shilpi` sidecars,
so installing the app gives you the server, the shell, and the GUI together.

## Frontend without Tauri

`frontend/` can be opened in a plain browser for layout/interaction checks —
it falls back to an in-memory mock (`mockInvoke` in `app.js`) when
`window.__TAURI__` isn't present, so nothing there talks to a real server.
