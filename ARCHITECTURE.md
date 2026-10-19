# ShilpiDB — Architecture Blueprint

**Purpose:** a geometry-native database for CAD drawings that transforms DWG,
DXF, and PDF files into an AI-readable engineering knowledge graph.

This document is the long-term vision. It is deliberately much bigger than
what exists in the codebase today — see [ROADMAP.md](ROADMAP.md) for what's
actually built, in progress, or realistically next. Treat this file as the
north star everything else is measured against, not a status report.

> **Reference consumer.** [O-Draft](https://github.com/HolagundiWorks/O-DRAFT)
> embeds this engine (`shilpidb` codec + `SpatialGrid`) via its `aad-vdb`
> crate and connects to `shilpid` over `shilpi-client`. Much of the import,
> semantic-recognition, metadata, and knowledge-graph work described below is
> being built on the O-Draft side today, on top of this storage core — see the
> [README's reference-application section](README.md#reference-application-o-draft)
> and O-Draft's
> [CAD + ShilpiDB architecture](https://github.com/HolagundiWorks/O-DRAFT/blob/main/docs/architecture/aadt-shilpidb-architecture.md).
>
> **Ecosystem.** The [AORMS](https://github.com/HolagundiWorks/aorms) platform
> (AEC consulting-firm management) is the broader consumer this engine is
> heading toward powering — its plan-measurement/takeoff surface over ShilpiDB's
> geometry. The integration plan lives in
> [docs/AORMS-INTEGRATION.md](docs/AORMS-INTEGRATION.md).

---

## Vision

ShilpiDB is **not** a traditional database, CAD library, or vector database.

It is a **geometry-native database engine** that stores engineering geometry,
semantics, topology, relationships, embeddings, and revisions as first-class
citizens.

Instead of AI interpreting pixels every time, drawings are parsed once into
structured engineering knowledge, allowing AI to reason over data rather than
images.

---

## High-level architecture

```text
                   ┌─────────────────────────┐
                   │    External Sources     │
                   │─────────────────────────│
                   │ DWG │ DXF │ PDF │ IFC    │
                   └────────────┬────────────┘
                                │
                                ▼
                   ┌─────────────────────────┐
                   │     Import Pipeline     │
                   └────────────┬────────────┘
                                │
          ┌─────────────────────┼──────────────────────┐
          ▼                     ▼                       ▼
     DWG Parser            DXF Parser              PDF Parser
          │                     │                       │
          └─────────────────────┼───────────────────────┘
                                ▼
                 Geometry Normalization Engine
                                │
                                ▼
                 Primitive Extraction Engine
                                │
                                ▼
              Engineering Object Recognition Engine
                                │
                                ▼
                        ShilpiDB
                                │
     ┌───────────────┬───────────────┬───────────────┐
     ▼               ▼               ▼               ▼
 Geometry        Metadata        Topology        AI Embeddings
 Engine          Engine          Engine          Engine
     │               │               │               │
     └───────────────┴───────────────┴───────────────┘
                                │
                                ▼
                    Query & AI Reasoning Engine
                                │
      ┌─────────────┬─────────────┬─────────────┐
      ▼             ▼             ▼             ▼
 Drafting     Quantity Takeoff   Validation   AI Agents
```

---

## Core modules

```
ShilpiDB
├── Storage Engine
├── Geometry Engine
├── Metadata Engine
├── Semantic Engine
├── Topology Engine
├── Embedding Engine
├── Pattern Recognition Engine
├── Query Engine
├── AI Interface
├── Index Engine
├── Transaction Engine
├── Cache Engine
├── Version Control
├── Security Engine
├── Backup Engine
├── Plugin Manager
└── Monitoring
```

Today, only a sliver of this exists: **Storage Engine** (the codec) and
**Index Engine** (the spatial grid), and only over opaque `Bbox` + `u64` id
records — nothing else in this list is implemented. See ROADMAP.md.

---

## Source file layer

Supported formats (target state):

```
DWG
DXF
PDF (vector)
PDF (scanned)
SVG
IFC
DGN
STEP
Revit export
BIM
```

Each source is converted into a common internal representation.

---

## Parsing layer

Responsible for extracting every drawing primitive.

### Geometry

```
Point       Line        Polyline    Arc
Circle      Ellipse     Spline      Region
Solid       Mesh        Block       Attribute
Text        Leader      Dimension   Viewport
Raster      Hatch
```

Every primitive receives a globally unique ObjectID.

---

## Geometry normalization

Converts all imported geometry into a common internal format.

```
DWG Line
DXF Line          →   Internal Geometry Object
PDF Vector
```

Benefits: a single geometry API, independence from the source file format,
faster indexing, and easier AI training.

---

## Semantic recognition layer

Converts geometry into engineering objects.

```
4 connected lines   →   Wall
Rectangle block     →   Window
Circle              →   Column
```

### Semantic classes

```
Wall        Door        Window      Column
Beam        Slab        Footing     Roof
Ceiling     Stair       Ramp        Grid
Axis        Room        Zone        Furniture
Equipment   Electrical  Mechanical  Plumbing
```

---

## Metadata engine

Stores engineering information per object:

```
ObjectID            Layer               Discipline
Material             Thickness           Height
Width                Fire Rating         Structural Type
Construction Phase   Revision            Author
Timestamp            Status              Confidence Score
Company Standard     Project ID
```

---

## Topology engine

Stores engineering relationships, e.g.:

```
Wall   contains      Door
Door   opens_to      Room
Beam   supported_by  Column
Room   belongs_to    Floor
Floor  belongs_to    Building
```

---

## Engineering knowledge graph

```
Building
└── Floor
    ├── Room
    │     ├── Wall
    │     ├── Window
    │     ├── Door
    │     └── Ceiling
    ├── Structural
    │      ├── Column
    │      ├── Beam
    │      └── Slab
    └── Services
           ├── Plumbing
           ├── Electrical
           └── HVAC
```

---

## AI embedding engine

Stores semantic embeddings at multiple levels:

```
Primitive → Object → Room → Floor → Building → Project → Company → Industry
```

Used for: similar-object search, layout similarity, AI retrieval, RAG,
pattern recognition.

---

## Pattern recognition engine

Learns reusable engineering knowledge, e.g. door patterns, window types,
apartment layouts, hospital wards, beam grids, structural frames, toilet
layouts, kitchen modules, MEP layouts.

---

## Spatial indexes

```
RTree       QuadTree     KDTree      Octree
BVH         Morton Index Hilbert Curve
```

Today: a uniform grid (`SpatialGrid`) — see [src](crates/shilpidb/src/index.rs).
The rest are candidates as data scale and query shapes demand them.

---

## AI indexes

```
HNSW        IVF          PQ          DiskANN     Flat Index
```

---

## Graph index

Stores relationships: `Wall → Door → Room → Floor → Building`.

---

## Query engine

**Geometry queries:** find all walls; intersect beam; nearest column;
objects inside polygon.

**Metadata queries:** `Material = Brick`; `Height > 3000`; `FireRating = 2hr`.

**Semantic queries:** find all hospitals; find all staircases; find all
apartment layouts.

**AI queries:** find similar plans; suggest missing columns; predict wall
type; estimate quantities; find drafting mistakes.

---

## AI interface

The AI never reads CAD files directly — it consumes structured knowledge:

```
DWG → Parser → Geometry Database → Knowledge Graph → AI
```

---

## Version control

Every modification is recorded: revision, author, timestamp, delta, rollback
point, merge, conflict resolution. Git-like branching can be supported for
collaborative design.

---

## Storage engine

Responsibilities: page manager, memory manager, compression, checksums,
snapshots, recovery, transactions, garbage collection.

Suggested on-disk layout: header → page table → object table → geometry →
metadata → relationships → embeddings → indexes → binary blob storage.

---

## Cache engine

Geometry cache, metadata cache, embedding cache, query cache, disk cache,
memory cache.

---

## Transaction engine

ACID, MVCC, rollback, commit, savepoints, crash recovery.

---

## Security

Authentication, RBAC, TLS, encryption, audit logs, API keys, SSO, OAuth2.

---

## APIs

REST, gRPC, and client bindings for C++, C#, Python, Rust, Go, JavaScript.

Today: a Rust client (`shilpi-client`) over the binary protocol, plus a
dependency-free JSON-over-HTTP gateway (`shilpi-http`) so non-Rust hosts can
reach the store. This is the first concrete step toward the
[AORMS](https://github.com/HolagundiWorks/aorms) integration — see
[docs/AORMS-INTEGRATION.md](docs/AORMS-INTEGRATION.md).

---

## Plugin SDK

Plugins may provide: importers, exporters, AI models, geometry tools,
validators, estimators, code checkers, visualization, industry extensions.

---

## Database services

Database server, CLI, desktop manager, web dashboard, scheduler, worker
queue, background indexer, AI service, monitoring service, backup service.

---

## Developer tools

Database manager, query console, schema explorer, object browser, graph
viewer, embedding viewer, topology viewer, performance monitor, index
manager, transaction monitor, revision browser, log viewer.

---

## AI workflow

```
CAD Drawing → Import → Geometry Parsing → Primitive Extraction
  → Semantic Recognition → Relationship Mapping → Knowledge Graph
  → Embedding Generation → Indexed Database → LLM / AI Agent
  → Engineering Reasoning → User Response
```

---

## Long-term phases

**Phase 1** — Storage Engine, Geometry Engine, Importers, Metadata, Query
Engine.

**Phase 2** — AI Semantic Recognition, Knowledge Graph, Pattern Recognition,
Embedding Engine.

**Phase 3** — Multi-user Database, Transactions, Collaboration, Version
Control.

**Phase 4** — Distributed Database, Clustering, Replication, Cloud Sync.

**Phase 5** — Autonomous Engineering AI, Automatic Drafting, Quantity
Estimation, Code Compliance, Digital Twin Integration, Construction ERP
Integration.

(ROADMAP.md breaks Phase 1 down further into the concrete, buildable steps
this repo is actually working through.)

---

## End goal

ShilpiDB becomes the **single source of truth** for engineering data.
Instead of storing drawings as disconnected files, every engineering entity
becomes:

- A precise geometric object
- A semantic engineering object
- A node in an engineering knowledge graph
- An AI-searchable embedding
- A version-controlled asset
- A rule-aware construction component

This architecture enables AI to reason over engineering knowledge rather than
performing repeated computer vision on drawings, creating a scalable
foundation for CAD, BIM-lite, estimation, compliance, ERP, and intelligent
design automation.
