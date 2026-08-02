# ShilpiDB

A small, **dependency-free** spatial vector-store engine in Rust: a hand-rolled
little-endian binary codec plus a uniform-grid **spatial index** over record
bounding boxes. It is the storage core behind [AAD](https://github.com/HolagundiWorks/AADT)'s
`.vdb` databases, extracted so it can be reused — and, eventually, run as a
standalone server.

- **Domain-agnostic.** Records are opaque `u64` ids with a `Bbox`; the payload
  bytes are whatever you encode with the `Writer` / `Reader`.
- **Owned end to end.** No `serde`, no external crate, `#![forbid(unsafe_code)]`.
- **Spatial by design.** `O(cells + hits)` window and point queries via
  `SpatialGrid`, and the index serializes into your file so it persists.

## Use

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
boxes. See the crate docs (`cargo doc --open`) for the codec and a full round
trip.

## Roadmap

ShilpiDB currently ships as an embedded library. The plan is a standalone
`shilpid` server + `shilpi` shell (MongoDB-style) over a length-framed binary
wire protocol that reuses this same codec, so the file format and the wire
format share one encoding.

## License

Apache-2.0.
