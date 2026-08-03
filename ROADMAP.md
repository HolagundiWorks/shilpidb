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
- **`shilpi`** — a `mongosh`-style CLI shell that connects to `shilpid` and
  runs the same operations interactively or via `-e "<command>"`.
- **ShilpiDB Desktop** ([`gui/`](gui)) — a Tauri GUI (the Compass
  equivalent): connect to a server, browse records in a table and on a
  spatial canvas, run bbox queries, insert/delete, save to disk, or launch
  the bundled `shilpid` sidecar directly from the app.
- **Windows installer** — Tauri's bundler, with `shilpid` and `shilpi`
  staged as sidecar binaries, so one installer sets up the server, the
  shell, and the GUI together (see [`gui/README.md`](gui/README.md)).
- End-to-end integration tests spawn the real `shilpid`/`shilpi` binaries
  (not just in-process calls) to cover the actual process boundary.
- CI on every push/PR: `cargo fmt`, `cargo clippy -D warnings`, build, test
  across the whole workspace.

This is Storage Engine + a first Index Engine from ARCHITECTURE.md's module
list, wrapped in a real server/shell/GUI product shape — and nothing else
from that list yet: no entity/geometry types beyond `Bbox`, no metadata, no
topology, no embeddings, no importers (DWG/DXF/PDF/SVG).

## Phase 1 — Drawing data model

Goal: give hosts a real, reusable schema for 2D vector drawings instead of
requiring every host to invent entity encoding from scratch. This is where
the engine starts catching up to ARCHITECTURE.md's Geometry Engine.

- Entity types: point, polyline, arc, bezier/spline, text label, at minimum —
  each as `Bbox` + payload encoded with the existing codec.
- Layers / groups: named, orderable containers of entities; visibility and
  simple style attributes (stroke, fill, width).
- A documented `.vdb` file layout: header (`MAGIC` + version, already in
  place) → metadata section → entity payload sections → `SpatialGrid`. Version
  the layout so old files stay readable as it evolves.
- Round-trip tests: build a drawing, serialize, reload, verify entities +
  index match.

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
