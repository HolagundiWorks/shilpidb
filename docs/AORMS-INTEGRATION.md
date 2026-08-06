# ShilpiDB in the AORMS ecosystem — integration roadmap

This document explains where ShilpiDB fits in the **AORMS** platform and lays
out the concrete, buildable path to making it a first-class part of that
ecosystem. It complements the engine's own [ROADMAP.md](../ROADMAP.md) (which
is about the storage engine itself) and [ARCHITECTURE.md](../ARCHITECTURE.md)
(the long-term geometry-database vision).

## The players

| Project | Role |
|---|---|
| **ShilpiDB** (this repo) | The geometry-native storage substrate: the `.vdb` codec, the `SpatialGrid`, and the `shilpid` server / `shilpi` shell / desktop GUI around it. |
| **AADT CAD** ([`HolagundiWorks/AADT`](https://github.com/HolagundiWorks/AADT)) | The reference CAD consumer — embeds the codec + grid and/or talks to `shilpid`. |
| **AORMS** ([`HolagundiWorks/aorms`](https://github.com/HolagundiWorks/aorms)) | Architecture Office Resources Management System — the platform for AEC consulting firms (TypeScript/Fastify/tRPC + PostgreSQL, React SPA, a Python worker). ESTI is its embedded-intelligence layer. |

ShilpiDB is the **geometry floor**; AORMS is the **business/ops platform** whose
plan-measurement and quantity-takeoff pillar is the natural consumer of that
floor.

## Where things stand today (honest baseline)

- **No live integration exists yet.** AORMS currently does its drawing work
  without ShilpiDB: a Python worker converts **DXF→SVG**, PDF.js renders plans,
  and plan markup + measurements live as **PostgreSQL rows**
  (`esti_plan_markup_*`, `esti_measurement_*`, `esti_building_level`,
  `esti_sheet_calibration`).
- **The former CAD bridge was retired.** AORMS's native desktop CAD companion,
  **ESTICAD**, was dropped on 2026-07-19 (the `companion` tRPC namespace, CAD AI
  drafts, drawing-scale calibration, and `esticad://` deep links went with it).
  Per AORMS's own docs, "no quantity takeoff surface exists today."

So ShilpiDB's role is **latent but strategic**: it is positioned to be the
geometry/measurement engine behind a revived takeoff/CAD-extraction surface —
storing drawing entities as structured, spatially-queryable records instead of
Postgres rows and flattened SVG.

## The core obstacle

`shilpid` speaks a compact binary wire protocol with a **Rust-only** client
(`shilpi-client`). AORMS is **TypeScript + Python**. Until ShilpiDB is reachable
from a non-Rust runtime, no AORMS integration is possible at all. Everything
else (entity model, importers, auth) is downstream of that.

## Roadmap

### Milestone 0 — HTTP/JSON gateway (started ✅)

A dependency-free HTTP gateway, [`crates/shilpi-http`](../crates/shilpi-http),
that fronts a `shilpid` backend and exposes JSON over HTTP, so AORMS's
TypeScript/Python code can reach the store with a plain `fetch`/`requests` call.

Shipped in this milestone:

- `GET /health`, `GET /stats`, `GET /records`, `GET /records/{id}`,
  `PUT /records/{id}`, `DELETE /records/{id}`, `GET /query/bbox`,
  `GET /query/point`, `POST /save`.
- Inputs via path + query params + raw body (no JSON parsing required yet);
  JSON out. Unit + end-to-end integration tests.

Still open (follow-ups within this milestone): a JSON **request-body** variant
of insert (and a batch/`InsertMany` endpoint), CORS headers for browser callers,
and structured error codes.

### Milestone 1 — a typed drawing entity model (engine [Phase 1]) (started ✅)

The gateway moves opaque bytes; AORMS needs *meaning*. This is the
[`crates/shilpi-drawing`](../crates/shilpi-drawing) crate: typed entities
(point, polyline, arc, text), layers with style, units (millimetres by default,
matching AORMS), and a versioned `.vdb` layout, so a drawing round-trips as real
entities rather than host-specific payloads. It builds only on the
dependency-free `shilpidb` codec + spatial index and bridges to the server via
`Drawing::to_records()` / `Entity::decode_payload()`. See the worked
`floorplan` example for a whole drawing pushed to a live `shilpid`. Maps onto
[ROADMAP.md](../ROADMAP.md) Phase 1.

Still open (follow-ups within this milestone): bezier/spline entities, per-entity
metadata (material/discipline — really the start of milestone 3), and an
optional persisted spatial-grid section in the file (today the grid is derived
on load).

### Milestone 2 — import/export + calibration (engine [Phase 2])

SVG first, then read-only **DXF** — the formats AORMS already ingests — plus
per-drawing scale/units metadata mirroring AORMS's `esti_sheet_calibration`. A
fixture-based conformance suite (source → entities → re-export → diff).

### Milestone 3 — measurement mapping

Map ShilpiDB entities onto AORMS's measurement model (markers → library items →
measurement rows; `deriveMeasurementQuantity()` semantics). Prove a plan stored
in `.vdb` can drive an AORMS abstract sheet.

### Milestone 4 — multi-tenant safety

AORMS is multi-firm SaaS with device-token auth and an audit-first posture.
Before shared/server deployment ShilpiDB needs tenant scoping and at minimum a
bearer token on the protocol/gateway (engine [Phase 5] security, pulled forward
by this use case).

### Milestone 5 — hardening for shared use

Crash-safe writes (WAL/append log) and a `.vdb` checksum so a killed process
can't corrupt data, `RwLock`/read-concurrency in the server, and benchmarks at
realistic drawing sizes. Maps onto [ROADMAP.md](../ROADMAP.md) Phase 5.

## Deployment shapes

Two viable integration modes, not mutually exclusive:

1. **Networked** — AORMS's backend/worker talks to a shared `shilpid` through
   `shilpi-http` (this milestone). Good for the multi-client path.
2. **Embedded, local-first** — ShilpiDB (dependency-free, file-based `.vdb`)
   embedded in AORMS's Tauri desktop shell as a local geometry store. A strong
   fit for AORMS's stated local-first direction.
