# ShilpiDB

A small, **dependency-free** spatial vector-store engine in Rust, plus a
MongoDB-shaped product around it: a server, a shell, and a desktop GUI.

**Where this is headed:** a database manager purpose-built for 2D vector
graphics — storing drawing files (CAD-style geometry: lines, polylines, arcs,
text, layers) as structured vector data rather than pixels, so it can be
queried spatially today and, later, embedded and searched semantically for AI
tooling. See [ARCHITECTURE.md](ARCHITECTURE.md) for that long-term vision and
[ROADMAP.md](ROADMAP.md) for the concrete plan getting there. What's below is
what's actually implemented right now.

## Components

| Crate / dir                                 | What it is                                                                          |
| -------------------------------------------- | ------------------------------------------------------------------------------------ |
| [`crates/shilpidb`](crates/shilpidb)         | The storage engine: a hand-rolled little-endian binary codec (`Writer`/`Reader`) and a uniform-grid spatial index (`SpatialGrid`) over `(u64, Bbox)` records. `#![forbid(unsafe_code)]`, zero dependencies. |
| [`crates/shilpi-protocol`](crates/shilpi-protocol) | The length-framed request/response wire protocol `shilpid` and its clients (the shell, the GUI) share, built on the `shilpidb` codec. |
| [`crates/shilpid`](crates/shilpid)           | The server: a TCP daemon, an in-memory record store backed by `SpatialGrid`, file persistence. |
| [`crates/shilpi`](crates/shilpi)             | A `mongosh`-style CLI shell that talks to `shilpid`.                                 |
| [`crates/shilpi-http`](crates/shilpi-http)   | A dependency-free HTTP/JSON gateway in front of `shilpid`, so non-Rust hosts (e.g. the AORMS TypeScript/Python stack) can reach the store over REST. |
| [`gui/desktop`](gui/desktop)                 | ShilpiDB Desktop — a WinUI 3 app for browsing and querying a `shilpid` server. See [`gui/README.md`](gui/README.md). |

Everything except `gui` is one Cargo workspace rooted here, and stays
dependency-free (std only). The Windows desktop app is WinUI 3 under
`gui/desktop` (its own .NET solution). An earlier Tauri shell still lives in
`gui/src-tauri` and is excluded from this workspace — see the `exclude` note
in [`Cargo.toml`](Cargo.toml).

## Install the CLI (Linux, macOS, Windows)

Prebuilt `shilpid` + `shilpi` binaries are published to
[GitHub Releases](https://github.com/HolagundiWorks/shilpidb/releases) for
Linux (x86_64), macOS (x86_64 and Apple Silicon), and Windows (x86_64) —
built by [`.github/workflows/release.yml`](.github/workflows/release.yml).

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/HolagundiWorks/shilpidb/main/scripts/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/HolagundiWorks/shilpidb/main/scripts/install.ps1 | iex
```

Both scripts detect your OS/arch, download the matching release archive,
install `shilpid`/`shilpi` to a per-user directory, and add it to your PATH.
Pin a version with `SHILPIDB_VERSION=v0.1.1` (env var on Unix, or
`$env:SHILPIDB_VERSION` before the PowerShell command) instead of installing
latest. See the scripts themselves — [`scripts/install.sh`](scripts/install.sh),
[`scripts/install.ps1`](scripts/install.ps1) — for every option.

No release has been published yet as of this writing; the workflow runs (and
these scripts start working) once someone with push access cuts one:

```bash
git tag v0.1.1 && git push origin v0.1.1
```

Until then, build from source with `cargo build --release -p shilpid -p shilpi`
or run in place per the quick start below.

## Quick start: server + shell

```bash
cargo run -p shilpid -- --bind 127.0.0.1:7420 --data ./mydb.vdb
```

`shilpid --help` lists every option: a config file (`--config PATH`, or
`./shilpid.conf` if present), `SHILPID_*` environment variables, autosave
interval, a connection cap, read-only mode, log level, and the spatial-grid
tunables — flags override env vars override the config file override
built-in defaults, same precedence as `mongod --config`.

In another terminal:

```bash
cargo run -p shilpi -- --host 127.0.0.1:7420
shilpi> insert 1 0 0 10 10 hello
ok
shilpi> query bbox -5 -5 5 5
1
shilpi> get 1
bbox=(0, 0)-(10, 10) payload="hello"
shilpi> save
ok
shilpi> help
```

`shilpi -e "<command>"` runs one command non-interactively (scripting/tests).

## Reach it over HTTP (non-Rust hosts)

`shilpid` speaks a binary protocol with a Rust-only client. For hosts in other
languages, `shilpi-http` puts a small JSON-over-HTTP gateway in front of it:

```bash
# terminal 1: the server
cargo run -p shilpid -- --bind 127.0.0.1:7420 --data ./mydb.vdb
# terminal 2: the gateway (defaults: HTTP :7421, backend 127.0.0.1:7420)
cargo run -p shilpi-http -- --bind 127.0.0.1:7421 --backend 127.0.0.1:7420
```

```bash
# insert a record (bbox in query params, payload is the raw body), then query it
curl -X PUT "http://127.0.0.1:7421/records/1?minX=0&minY=0&maxX=10&maxY=10" --data-binary hello
curl "http://127.0.0.1:7421/query/bbox?minX=-5&minY=-5&maxX=5&maxY=5"   # -> {"ids":[1]}
curl "http://127.0.0.1:7421/records/1"   # -> {"id":1,"bbox":{...},"payloadHex":"68656c6c6f","payloadText":"hello"}
```

Endpoints: `GET /health`, `GET /stats`, `GET /records`, `GET /records/{id}`,
`PUT /records/{id}`, `DELETE /records/{id}`, `GET /query/bbox`,
`GET /query/point`, `POST /save`. See
[`crates/shilpi-http`](crates/shilpi-http) for details, and
[`docs/AORMS-INTEGRATION.md`](docs/AORMS-INTEGRATION.md) for why it exists.

## Use the storage engine directly

```rust
use shilpidb::{Bbox, SpatialGrid};

let grid = SpatialGrid::build(vec![
    (1, Bbox::from_corners(0.0, 0.0, 1.0, 1.0)),
    (2, Bbox::from_corners(5.0, 5.0, 6.0, 6.0)),
]);
assert_eq!(grid.query_bbox(Bbox::from_corners(0.5, 0.5, 0.9, 0.9)), vec![1]);
```

A host composes a database file by writing the `MAGIC` header + a version, its
own payload sections with the codec, and a `SpatialGrid` built from its records'
boxes — `shilpid`'s own [`db.rs`](crates/shilpid/src/db.rs) is a worked example.
`SpatialGrid::build_with_config` exposes the grid's tunables (fixed cell size,
overflow/huge-query thresholds) via `GridConfig`, for hosts that outgrow the
auto-chosen defaults `build` uses. See the crate docs (`cargo doc -p shilpidb
--open`) for the codec and a full round trip.

## Reference application: AADT CAD

[**AADT CAD**](https://github.com/HolagundiWorks/AADT) is the primary consumer
of ShilpiDB and the reference implementation of the "single source of truth"
vision in [ARCHITECTURE.md](ARCHITECTURE.md) — a 2D engineering drafting app
that stores every entity as a structured object in ShilpiDB rather than in a
binary drawing file.

It uses ShilpiDB two ways, both pinned to a single commit so one `shilpidb`
type is shared across them:

- **Embedded engine** — AADT's `aadt-vdb` crate re-exports the `shilpidb` codec
  (`Reader`/`Writer`) and spatial index (`SpatialGrid`) directly, and composes
  its `.vdb` files from them (native vector records + a persisted grid).
- **Server client** — `aadt-vdb`'s `VdbClient` speaks AADT's entity model to a
  running `shilpid` over `shilpi-client`, for the shared/multi-client path.

The two projects split responsibilities cleanly:

| ShilpiDB (this repo) | AADT CAD |
|----------------------|----------|
| Storage codec + `SpatialGrid` | CAD editor, command system, rendering |
| `shilpid` server, `shilpi` shell, desktop GUI | DXF/DWG import + parsing, semantic recognition |
| The `.vdb` engine primitives | Entity model, transactions/undo, the layer standard & metadata |

### AORMS platform

[**AORMS**](https://github.com/HolagundiWorks/aorms) (Architecture Office
Resources Management System) is the broader platform for AEC consulting firms
from the same authors. Its plan-measurement and quantity-takeoff surface is the
natural consumer of ShilpiDB's geometry substrate. There is no live integration
yet — the [`shilpi-http`](crates/shilpi-http) gateway above is the first step
toward one. See [`docs/AORMS-INTEGRATION.md`](docs/AORMS-INTEGRATION.md) for the
ecosystem map and the phased plan.

So the ambitious import / semantic / knowledge-graph layers in ARCHITECTURE.md
are, in practice, being built out **on the AADT side today** on top of this
engine. For how the layer standard turns a stored drawing into an
agent-actionable model, see AADT's
[layers & AI agents guide](https://github.com/HolagundiWorks/AADT/blob/main/docs/standards/shilpidb-layers-and-agents.md)
and its
[CAD + ShilpiDB architecture](https://github.com/HolagundiWorks/AADT/blob/main/docs/architecture/aadt-shilpidb-architecture.md).

## Desktop GUI

The Windows app is WinUI 3. See [`gui/README.md`](gui/README.md) — connect to
a `shilpid`, browse records on a spatial canvas and in a table, run queries,
insert/delete, and save to disk. `Start local server` launches `shilpid` from
the window.

```powershell
cargo build -p shilpid
dotnet run --project gui/desktop/src/ShilpiDB.Desktop
```

## Roadmap

1. **Drawing data model** — entity types (polyline, arc, text, …), layers,
   a documented `.vdb` layout.
2. **Import/export** — SVG and DXF interchange.
3. **Metadata, topology, knowledge graph** — engineering objects and
   relationships, not just geometry with an id.
4. **AI integration layer** — optional embedding storage per entity/drawing,
   a similarity index alongside `SpatialGrid`, bring-your-own-model hooks.
5. **Hardening** — crash-safe writes, benchmarks, format stability.

Full detail in [ROADMAP.md](ROADMAP.md); the destination those phases build
toward is in [ARCHITECTURE.md](ARCHITECTURE.md).

## License

Apache-2.0.
