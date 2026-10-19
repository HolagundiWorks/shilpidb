//! # shilpi-drawing — a typed 2D drawing model for ShilpiDB
//!
//! The `shilpidb` engine is deliberately domain-agnostic: it stores
//! `(u64 id, Bbox, opaque bytes)` and leaves the meaning of the bytes to the
//! host. This crate supplies that meaning for **2D vector drawings** — a
//! reusable schema of [`Geometry`] primitives (point, polyline, arc, text),
//! [`Layer`]s, and [`Units`] — so hosts like AADT and AORMS don't each
//! reinvent entity encoding.
//!
//! It builds only on the dependency-free `shilpidb` codec + spatial index, and
//! bridges to the server two ways:
//!
//! - A whole [`Drawing`] serializes to a versioned `.vdb` layout
//!   ([`Drawing::to_bytes`] / [`Drawing::read`]).
//! - Or it lowers to `shilpid` records ([`Drawing::to_records`]) for
//!   `Client::insert_many`, with [`Entity::decode_payload`] reconstructing an
//!   entity from a stored record.
//!
//! ```
//! use shilpi_drawing::{Drawing, Entity, Geometry, Layer, Units};
//! use shilpidb::Point;
//!
//! let mut d = Drawing::new(Units::Millimeter);
//! let walls = d.add_layer(Layer::new("walls"));
//! d.push(1, walls, Geometry::Polyline {
//!     points: vec![Point::new(0.0, 0.0), Point::new(5000.0, 0.0)],
//!     closed: false,
//! });
//!
//! // Round-trip through the versioned file format.
//! let bytes = d.to_bytes();
//! assert_eq!(Drawing::read(&bytes).unwrap(), d);
//!
//! // Or lower to shilpid records and reconstruct one entity.
//! let (id, _bbox, payload) = &d.to_records()[0];
//! let back = Entity::decode_payload(*id, payload).unwrap();
//! assert_eq!(back.id, 1);
//! ```

mod drawing;
mod geometry;
mod style;
mod units;

pub use drawing::{Drawing, Entity, DRAWING_FORMAT_VERSION};
pub use geometry::Geometry;
pub use style::{Layer, Rgba};
pub use units::Units;
