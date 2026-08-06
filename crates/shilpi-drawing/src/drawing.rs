//! A whole drawing: units, layers, and entities — with a versioned `.vdb`
//! layout and a bridge to `shilpid` records.

use shilpidb::{Bbox, Error, Reader, SpatialGrid, Writer, MAGIC};

use crate::geometry::Geometry;
use crate::style::Layer;
use crate::units::Units;

/// On-disk format version for a drawing `.vdb`. Distinct from `shilpid`'s own
/// record-store version — this is the *host* format written after [`MAGIC`],
/// exactly the layering the engine's `MAGIC` doc describes.
pub const DRAWING_FORMAT_VERSION: u32 = 1;

/// One placed entity: a stable `id`, the `layer` it belongs to (an index into
/// [`Drawing::layers`]), and its [`Geometry`].
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: u64,
    pub layer: u32,
    pub geometry: Geometry,
}

impl Entity {
    pub fn new(id: u64, layer: u32, geometry: Geometry) -> Self {
        Entity {
            id,
            layer,
            geometry,
        }
    }

    /// The record payload for `shilpid`: `[u32 layer][geometry bytes]`. Paired
    /// with the entity's [`Geometry::bbox`], this is everything needed to
    /// reconstruct the entity from a stored record.
    pub fn encode_payload(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.layer);
        self.geometry.encode(&mut w);
        w.into_bytes()
    }

    /// Rebuild an entity from an `id` and the payload produced by
    /// [`encode_payload`](Self::encode_payload) (e.g. a `shilpid` `get`).
    pub fn decode_payload(id: u64, payload: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(payload);
        let layer = r.u32()?;
        let geometry = Geometry::decode(&mut r)?;
        Ok(Entity {
            id,
            layer,
            geometry,
        })
    }
}

/// A 2D vector drawing: a reusable schema for hosts (AADT, AORMS, …) instead of
/// each inventing its own entity encoding.
///
/// The on-disk layout is `MAGIC` → format version → units → layers → entities.
/// The spatial index is *derived* (rebuilt from entity boxes via
/// [`spatial_index`](Self::spatial_index)) rather than stored, so the file has
/// a single source of truth.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub units: Units,
    pub layers: Vec<Layer>,
    pub entities: Vec<Entity>,
}

impl Drawing {
    /// A new drawing in `units` with a single default layer (`"0"`, index 0),
    /// mirroring CAD's layer 0.
    pub fn new(units: Units) -> Self {
        Drawing {
            units,
            layers: vec![Layer::new("0")],
            entities: Vec::new(),
        }
    }

    /// Add a layer, returning its index (use it as an entity's `layer`).
    pub fn add_layer(&mut self, layer: Layer) -> u32 {
        self.layers.push(layer);
        (self.layers.len() - 1) as u32
    }

    /// Append an entity.
    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }

    /// Convenience: build and append an entity, returning its `id`.
    pub fn push(&mut self, id: u64, layer: u32, geometry: Geometry) -> u64 {
        self.add_entity(Entity::new(id, layer, geometry));
        id
    }

    /// The bounding box enclosing every entity, or `None` if empty.
    pub fn bbox(&self) -> Option<Bbox> {
        let mut it = self.entities.iter();
        let first = it.next()?.geometry.bbox();
        let (mut min_x, mut min_y, mut max_x, mut max_y) =
            (first.min.x, first.min.y, first.max.x, first.max.y);
        for e in it {
            let b = e.geometry.bbox();
            min_x = min_x.min(b.min.x);
            min_y = min_y.min(b.min.y);
            max_x = max_x.max(b.max.x);
            max_y = max_y.max(b.max.y);
        }
        Some(Bbox::from_corners(min_x, min_y, max_x, max_y))
    }

    /// Build a spatial index over the entities' bounding boxes (keyed by id).
    /// O(n); callers that query repeatedly should keep the returned grid.
    pub fn spatial_index(&self) -> SpatialGrid {
        SpatialGrid::build(
            self.entities
                .iter()
                .map(|e| (e.id, e.geometry.bbox()))
                .collect(),
        )
    }

    /// The entities as `shilpid` records `(id, bbox, payload)` — ready for
    /// `Client::insert_many`, so a whole drawing pushes in one round-trip.
    pub fn to_records(&self) -> Vec<(u64, Bbox, Vec<u8>)> {
        self.entities
            .iter()
            .map(|e| (e.id, e.geometry.bbox(), e.encode_payload()))
            .collect()
    }

    /// Serialize the drawing (see the type-level layout note).
    pub fn write(&self, w: &mut Writer) {
        w.raw(MAGIC);
        w.u32(DRAWING_FORMAT_VERSION);
        self.units.write(w);
        w.u32(self.layers.len() as u32);
        for layer in &self.layers {
            layer.write(w);
        }
        w.u32(self.entities.len() as u32);
        for e in &self.entities {
            w.u64(e.id);
            w.u32(e.layer);
            e.geometry.encode(w);
        }
    }

    /// Serialize to a fresh byte vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.write(&mut w);
        w.into_bytes()
    }

    /// Parse a drawing previously produced by [`write`](Self::write) /
    /// [`to_bytes`](Self::to_bytes).
    pub fn read(bytes: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);
        if r.raw(MAGIC.len())? != MAGIC {
            return Err(Error::Format("not a ShilpiDB file (bad magic)".to_string()));
        }
        let version = r.u32()?;
        if version != DRAWING_FORMAT_VERSION {
            return Err(Error::Format(format!(
                "unsupported drawing format version {version}"
            )));
        }
        let units = Units::read(&mut r)?;

        let layer_count = r.u32()? as usize;
        let mut layers = Vec::with_capacity(layer_count);
        for _ in 0..layer_count {
            layers.push(Layer::read(&mut r)?);
        }

        let entity_count = r.u32()? as usize;
        let mut entities = Vec::with_capacity(entity_count);
        for _ in 0..entity_count {
            let id = r.u64()?;
            let layer = r.u32()?;
            let geometry = Geometry::decode(&mut r)?;
            entities.push(Entity {
                id,
                layer,
                geometry,
            });
        }

        Ok(Drawing {
            units,
            layers,
            entities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Rgba;
    use shilpidb::Point;

    fn sample() -> Drawing {
        let mut d = Drawing::new(Units::Millimeter);
        let walls = d.add_layer(Layer::new("walls").with_color(Rgba::new(200, 0, 0, 255)));
        d.push(
            1,
            walls,
            Geometry::Polyline {
                points: vec![
                    Point::new(0.0, 0.0),
                    Point::new(5000.0, 0.0),
                    Point::new(5000.0, 3000.0),
                    Point::new(0.0, 3000.0),
                ],
                closed: true,
            },
        );
        d.push(
            2,
            walls,
            Geometry::Arc {
                center: Point::new(5000.0, 0.0),
                radius: 900.0,
                start_angle: std::f64::consts::FRAC_PI_2,
                end_angle: std::f64::consts::PI,
            },
        );
        d.push(
            3,
            0,
            Geometry::Text {
                at: Point::new(2000.0, 1500.0),
                height: 200.0,
                content: "Room 101".to_string(),
            },
        );
        d
    }

    #[test]
    fn file_round_trips() {
        let d = sample();
        let bytes = d.to_bytes();
        assert_eq!(Drawing::read(&bytes).unwrap(), d);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = sample().to_bytes();
        bytes[0] ^= 0xff;
        assert!(matches!(Drawing::read(&bytes), Err(Error::Format(_))));
    }

    #[test]
    fn records_bridge_round_trips_each_entity() {
        let d = sample();
        let records = d.to_records();
        assert_eq!(records.len(), 3);
        for (entity, (id, bbox, payload)) in d.entities.iter().zip(&records) {
            assert_eq!(*id, entity.id);
            assert_eq!(*bbox, entity.geometry.bbox());
            assert_eq!(&Entity::decode_payload(*id, payload).unwrap(), entity);
        }
    }

    #[test]
    fn spatial_index_finds_the_wall_but_not_far_queries() {
        let d = sample();
        let grid = d.spatial_index();
        // A window over the bottom-left wall corner hits the polyline (id 1).
        assert!(grid
            .query_bbox(Bbox::from_corners(-1.0, -1.0, 1.0, 1.0))
            .contains(&1));
        // Far away: nothing.
        assert!(grid
            .query_bbox(Bbox::from_corners(1e6, 1e6, 1e6 + 1.0, 1e6 + 1.0))
            .is_empty());
    }

    #[test]
    fn empty_drawing_has_no_bbox() {
        assert!(Drawing::new(Units::Millimeter).bbox().is_none());
    }
}
