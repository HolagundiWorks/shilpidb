# ShilpiDB Roadmap

This is the concrete, buildable path. For the long-term destination this is
in service of, see [ARCHITECTURE.md](ARCHITECTURE.md) — a geometry-native
database that turns CAD drawings into an AI-readable engineering knowledge
graph. Everything below maps onto that document's "Phase 1: Storage Engine,
Geometry Engine, Importers, Metadata, Query Engine" — broken into steps
small enough to actually ship one at a time.

## Status today (v0.1.x)

Shipped and tested — the storage engine, plus a full "MongoDB, but for this
engine" product shape around it:

- **Codec** ([`shilpidb/src/codec.rs`](crates/shilpidb/src/codec.rs)) —
  little-endian `Writer` / `Reader` for scalars and length-prefixed UTF-8
  strings, `#![forbid(unsafe_code)]`, zero dependencies.
- **Geometry** ([`shilpidb/src/geom.rs`](crates/shilpidb/src/geom.rs)) —
  minimal `Point` / `Bbox` with intersection tests.
- **Spatial index** ([`shilpidb/src/index.rs`](crates/shilpidb/src/index.rs)) —
  a uniform-grid `SpatialGrid` over `(u64, Bbox)` records, `O(cells + hits)`
  window/point queries, serializes into the host's file so it persists and
  rebuilds on load.
- **`shilpi-protocol`** — a length-framed request/response wire protocol on
  top of the codec: insert / get / delete / query-bbox / query-point / list
  / list-boxes / stats / save.
- **`shilpid`** — the server. Std-only TCP, threaded connections, an
  in-memory record store backed by `SpatialGrid`, atomic file persistence.
  Configuration ([`config.rs`](crates/shilpid/src/config.rs)) layers a config
  file (TOML subset, `--config`/`./shilpid.conf`), `SHILPID_*` env vars, and
  CLI flags — bind address, autosave interval, a connection cap, read-only
  mode, log level, and the spatial-grid tunables (`GridConfig`).
- **`shilpi`** — a `mongosh`-style CLI shell that connects to `shilpid` and
  runs the same operations interactively or via `-e "<command>"`.
- **`shilpi-http`** — a dependency-free JSON-over-HTTP gateway in front of
  `shilpid`, so non-Rust hosts can reach the store with a plain HTTP client.
  This is milestone 0 of the [AORMS integration](docs/AORMS-INTEGRATION.md).
- **`shilpi-drawing`** — a typed 2D drawing model (entities, layers, units) on
  top of the codec, with a versioned `.vdb` layout and a `shilpid`-records
  bridge. This is the start of Phase 1 below / milestone 1 of the AORMS
  integration.
- **ShilpiDB Desktop** ([`gui/`](gui)) — a Tauri GUI (the Compass
  equivalent): connect to a server, browse records in a table and on a
  spatial canvas, run bbox queries, insert/delete, save to disk, or launch
  the bundled `shilpid` sidecar directly from the app.
- **Windows installer** — Tauri's bundler, with `shilpid` and `shilpi`
  staged as sidecar binaries, so one installer sets up the server, the
  shell, and the GUI together (see [`gui/README.md`](gui/README.md)).
- **Cross-platform CLI installers** — [`.github/workflows/release.yml`](.github/workflows/release.yml)
  builds `shilpid`/`shilpi` natively for Linux, macOS (x86_64 + arm64), and
  Windows on a pushed `v*` tag and publishes them to a GitHub Release;
  [`scripts/install.sh`](scripts/install.sh) / [`scripts/install.ps1`](scripts/install.ps1)
  are the `curl | sh` / `irm | iex` installers for them.
- End-to-end integration tests spawn the real `shilpid`/`shilpi` binaries
  (not just in-process calls) to cover the actual process boundary.
- CI on every push/PR: `cargo fmt`, `cargo clippy -D warnings`, build, test
  across the whole workspace.

This is Storage Engine + a first Index Engine from ARCHITECTURE.md's module
list, wrapped in a real server/shell/GUI product shape — and nothing else
from that list yet: no entity/geometry types beyond `Bbox`, no metadata, no
topology, no embeddings, no importers (DWG/DXF/PDF/SVG).

## Ecosystem: AORMS integration

Beyond the engine's own phases below, [docs/AORMS-INTEGRATION.md](docs/AORMS-INTEGRATION.md)
tracks the concrete path to making ShilpiDB the geometry/measurement engine
behind the [AORMS](https://github.com/HolagundiWorks/aorms) platform. Milestone 0
(the `shilpi-http` gateway, so the TypeScript/Python stack can reach `shilpid`)
has started; the later milestones there build on the engine phases below.

## Phase 1 — Drawing data model (in progress)

Goal: give hosts a real, reusable schema for 2D vector drawings instead of
requiring every host to invent entity encoding from scratch. This is where
the engine starts catching up to ARCHITECTURE.md's Geometry Engine.

Landed in [`crates/shilpi-drawing`](crates/shilpi-drawing):

- ✅ Entity types: point, polyline, arc, text label — each as `Bbox` + payload
  encoded with the existing codec. (Bezier/spline still to come.)
- ✅ Layers: named containers with visibility and simple style attributes
  (stroke color, width); entities reference a layer.
- ✅ Units (millimetre default, matching AORMS).
- ✅ A documented, versioned `.vdb` layout: header (`MAGIC` + format version) →
  units → layers → entity payload sections. (The spatial grid is currently
  *derived* on load rather than stored; persisting it is a follow-up.)
- ✅ Round-trip tests: build a drawing, serialize, reload, verify entities; plus
  a `to_records()`/`decode_payload()` bridge so a whole drawing pushes into
  `shilpid` and reconstructs from a stored record.

Still open: bezier/spline entities, an optional persisted `SpatialGrid` section,
and per-entity metadata (which flows into Phase 3).

## Phase 2 — Import / export interchange

Goal: get real drawings in and out, not just synthetic test data.

- SVG import/export (closest match to the vector model; good first target).
- DXF import (read-only to start) for CAD interop, since AAD already targets
  that space.
- A small conformance test suite: known SVG/DXF fixtures → ShilpiDB entities
  → re-export → diff.

## Phase 3 — Metadata, topology, and the knowledge graph

Goal: move from "geometry with an id" to the engineering objects and
relationships ARCHITECTURE.md describes (Metadata Engine, Topology Engine,
semantic classes like Wall/Door/Window/Column).

- Per-entity typed metadata (layer, material, dimensions, discipline, …).
- Relationship edges between entities (`contains`, `opens_to`,
  `supported_by`, `belongs_to`) and a queryable graph over them.
- Semantic recognition as an explicit, separate pass over raw geometry
  (e.g. "4 connected lines → Wall") rather than something importers guess at
  inline — keeps the raw-geometry and semantic layers independently testable.

## Phase 4 — AI integration layer

Goal: make the vector data usable by ML tooling, not just human tools.

- Embedding storage: an optional per-entity or per-drawing vector field
  (float array) alongside the existing `Bbox`, encoded with the same codec.
- A second index structure for nearest-neighbor / similarity search over
  those embeddings (approximate NN — e.g. a simple IVF or HNSW-lite —
  mirroring how `SpatialGrid` sits next to the geometric data today).
- A stable extraction path: turn a drawing or entity into whatever
  representation an embedding model expects — kept outside the core engine
  as a separate, optional crate/module so the dependency-free core is
  unaffected.
- Hooks for external embedding generation (bring-your-own-model) rather than
  bundling a specific ML runtime.

## Phase 5 — Hardening and beyond

Goal: production-readiness once the shape of the format and API has settled,
then the further-out items from ARCHITECTURE.md (multi-user transactions,
distributed clustering, autonomous engineering AI) as real demand shows up.

- Crash-safe writes (append-only log or WAL) so a killed process can't
  corrupt a `.vdb` file.
- Benchmarks for spatial + similarity query paths at realistic drawing sizes
  (thousands–millions of entities).
- Backward-compatibility policy once the file format leaves 0.x.
- Everything past this point (ACID/MVCC transactions, RBAC/TLS/auth,
  replication, plugin SDK, REST/gRPC/multi-language clients) stays scoped to
  ARCHITECTURE.md until a concrete need pulls a specific piece forward.

## Sequencing notes

With the server/shell/GUI/installer track shipped, Phase 1 (drawing data
model) is next and unblocked — it's data-model work, independent of the
product shell already in place. Phase 3 onward (metadata, topology,
embeddings) does depend on Phase 1 landing first, since there's nothing to
attach metadata or embeddings to until entities are a real concept in the
engine.
