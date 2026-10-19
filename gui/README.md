# ShilpiDB Desktop

A Windows desktop app for browsing and querying a `shilpid` server — the
Compass equivalent for ShilpiDB. The UI is WinUI 3 in C#
([`desktop/`](desktop)). It speaks `shilpi-protocol` directly over TCP, so it
does not go through the Tauri shell.

## Run it

Build the server once, then start the app:

```powershell
cargo build -p shilpid
dotnet run --project gui/desktop/src/ShilpiDB.Desktop
```

Requires the .NET 8 SDK and Windows 10 1809 or later. The project is
unpackaged and carries its own Windows App SDK runtime, so a separate runtime
install is not required. Open [`desktop/ShilpiDB.Desktop.sln`](desktop/ShilpiDB.Desktop.sln)
in Visual Studio if you want the designer.

`Start local server` looks for `shilpid.exe` in this order: `SHILPID_PATH`,
next to the app, `target/debug` or `target/release` while walking up from the
app directory, then `PATH`. The database file for that local server is
`%LOCALAPPDATA%\ShilpiDB\desktop.vdb`.

Protocol tests, including a live round trip when `shilpid` is on disk:

```powershell
dotnet test gui/desktop/ShilpiDB.Desktop.sln
```

## What the app can do

- **Connect** to any running `shilpid` at `host:port`, or **Start local server**
  to launch `shilpid` on an OS-assigned port (stopped when the window closes).
- Browse records in a table and on a spatial canvas. Click a row or a box to
  inspect the payload as text and hex.
- Run a bbox query. Matches stay bright, other records dim, and the query
  window is drawn as a dashed rectangle.
- Insert or replace a record, delete the selected one, and flush the database
  to disk (**Save to disk**).

## Tauri shell

[`src-tauri/`](src-tauri) is the earlier Tauri v2 shell (plain HTML/CSS/JS in
[`frontend/`](frontend), Rust commands behind it). It is its own Cargo
workspace, excluded from the root one. The Windows NSIS installer described in
`src-tauri/tauri.conf.json` still comes from that shell:

```powershell
powershell -File scripts/prepare-sidecars.ps1
cargo install tauri-cli --version "^2" --locked
cargo tauri dev
```

Run the Tauri commands from `gui/src-tauri`. `frontend/` can also be opened in
a browser; it uses an in-memory mock when `window.__TAURI__` is absent.
