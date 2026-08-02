//! # shilpidb — a small, dependency-free spatial vector-store engine
//!
//! ShilpiDB is the storage core behind a *custom* on-disk vector database: a
//! hand-rolled little-endian [binary codec](codec) and a uniform-grid
//! [spatial index](SpatialGrid) over record bounding boxes. It is deliberately
//! **domain-agnostic** — records are opaque `u64` ids with a [`Bbox`], and the
//! payload bytes are whatever the host encodes with the [`Writer`] / [`Reader`].
//! No `serde`, no external crate, `unsafe`-free.
//!
//! A host (for example a CAD app storing a drawing) composes a database file by
//! writing the [`MAGIC`] header + a version, its own payload sections with the
//! codec, and a [`SpatialGrid`] built from its records' boxes — then queries
//! windows and points against the index in `O(cells + hits)`.
//!
//! ```
//! use shilpidb::{Bbox, SpatialGrid, Writer, Reader, MAGIC};
//!
//! // Build an index over two records and persist it after the magic header.
//! let grid = SpatialGrid::build(vec![
//!     (1, Bbox::from_corners(0.0, 0.0, 1.0, 1.0)),
//!     (2, Bbox::from_corners(5.0, 5.0, 6.0, 6.0)),
//! ]);
//! let mut w = Writer::new();
//! w.raw(MAGIC);
//! w.u32(1); // host format version
//! grid.write(&mut w);
//! let bytes = w.into_bytes();
//!
//! // Reopen: check the magic, then rebuild the index and query a window.
//! let mut r = Reader::new(&bytes);
//! assert_eq!(r.raw(MAGIC.len()).unwrap(), MAGIC);
//! let _version = r.u32().unwrap();
//! let grid = SpatialGrid::read(&mut r).unwrap();
//! assert_eq!(grid.query_bbox(Bbox::from_corners(0.5, 0.5, 0.9, 0.9)), vec![1]);
//! ```

mod codec;
mod geom;
mod index;

pub use codec::{Error, Reader, Writer};
pub use geom::{Bbox, Point};
pub use index::SpatialGrid;

/// File magic (8 bytes) identifying a ShilpiDB database: `b"SHILPIDB"`.
///
/// A host writes this first, then its own `u32` format version and payload. The
/// engine does not mandate a version scheme — that belongs to the host format.
pub const MAGIC: &[u8; 8] = b"SHILPIDB";
