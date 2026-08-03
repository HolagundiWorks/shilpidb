//! The in-memory, file-backed record store `shilpid` serves.
//!
//! Records are `(Bbox, payload bytes)` keyed by `u64` id — the same
//! primitives `shilpidb::SpatialGrid` already works with. Mutations go
//! straight into a `HashMap` for O(1) point lookups; the `SpatialGrid` is
//! rebuilt lazily from that map the first time a spatial query runs after a
//! mutation, rather than kept incrementally in sync (the grid has no
//! incremental insert/delete). That's the right tradeoff for now: simple,
//! correct, and fine at the record counts this MVP targets — see
//! ROADMAP.md's "hardening" phase for where this stops being good enough.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use shilpidb::{Bbox, Point, Reader, SpatialGrid, Writer, MAGIC};

const FORMAT_VERSION: u32 = 1;

pub struct Database {
    records: HashMap<u64, (Bbox, Vec<u8>)>,
    grid: Option<SpatialGrid>,
}

impl Database {
    pub fn new() -> Self {
        Database {
            records: HashMap::new(),
            grid: None,
        }
    }

    /// Load a database from `path`, or start empty if the file doesn't exist.
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let mut file = BufReader::new(File::open(path)?);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        let mut r = Reader::new(&bytes);
        let magic = r
            .raw(MAGIC.len())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a ShilpiDB data file (bad magic)",
            ));
        }
        let version = r
            .u32()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if version != FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported data file version {version}"),
            ));
        }
        let n = r
            .u32()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
            as usize;
        let mut records = HashMap::with_capacity(n);
        for _ in 0..n {
            let mut decode = || -> Result<(u64, Bbox, Vec<u8>), shilpidb::Error> {
                let id = r.u64()?;
                let min = Point::new(r.f64()?, r.f64()?);
                let max = Point::new(r.f64()?, r.f64()?);
                let len = r.u32()? as usize;
                let payload = r.raw(len)?.to_vec();
                Ok((id, Bbox::new(min, max), payload))
            };
            let (id, bbox, payload) =
                decode().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            records.insert(id, (bbox, payload));
        }
        Ok(Database {
            records,
            grid: None,
        })
    }

    /// Persist the database to `path`, atomically via a temp-file rename.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut w = Writer::new();
        w.raw(MAGIC);
        w.u32(FORMAT_VERSION);
        w.u32(self.records.len() as u32);
        for (id, (bbox, payload)) in &self.records {
            w.u64(*id);
            w.f64(bbox.min.x);
            w.f64(bbox.min.y);
            w.f64(bbox.max.x);
            w.f64(bbox.max.y);
            w.u32(payload.len() as u32);
            w.raw(payload);
        }
        let bytes = w.into_bytes();

        let tmp_path = path.with_extension("vdb.tmp");
        {
            let mut tmp = BufWriter::new(File::create(&tmp_path)?);
            tmp.write_all(&bytes)?;
            tmp.flush()?;
        }
        fs::rename(&tmp_path, path)
    }

    pub fn insert(&mut self, id: u64, bbox: Bbox, payload: Vec<u8>) {
        self.records.insert(id, (bbox, payload));
        self.grid = None;
    }

    pub fn get(&self, id: u64) -> Option<&(Bbox, Vec<u8>)> {
        self.records.get(&id)
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let existed = self.records.remove(&id).is_some();
        if existed {
            self.grid = None;
        }
        existed
    }

    pub fn list_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.records.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// All ids and boxes, sorted by id — no payload bytes, for browsing a
    /// whole database without an N+1 `get` per record.
    pub fn list_boxes(&self) -> Vec<(u64, Bbox)> {
        let mut records: Vec<(u64, Bbox)> = self
            .records
            .iter()
            .map(|(id, (bb, _))| (*id, *bb))
            .collect();
        records.sort_unstable_by_key(|(id, _)| *id);
        records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    fn grid(&mut self) -> &SpatialGrid {
        if self.grid.is_none() {
            let boxes: Vec<(u64, Bbox)> = self
                .records
                .iter()
                .map(|(id, (bb, _))| (*id, *bb))
                .collect();
            self.grid = Some(SpatialGrid::build(boxes));
        }
        self.grid.as_ref().unwrap()
    }

    pub fn query_bbox(&mut self, rect: Bbox) -> Vec<u64> {
        self.grid().query_bbox(rect)
    }

    pub fn query_point(&mut self, p: Point, tol: f64) -> Vec<u64> {
        self.grid().query_point(p, tol)
    }

    pub fn cell_size(&mut self) -> f64 {
        self.grid().cell_size()
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(x0: f64, y0: f64, x1: f64, y1: f64) -> Bbox {
        Bbox::from_corners(x0, y0, x1, y1)
    }

    #[test]
    fn insert_get_delete_round_trip() {
        let mut db = Database::new();
        db.insert(1, bb(0.0, 0.0, 1.0, 1.0), b"a".to_vec());
        assert_eq!(db.get(1).unwrap().1, b"a");
        assert!(db.delete(1));
        assert!(db.get(1).is_none());
        assert!(!db.delete(1));
    }

    #[test]
    fn query_reflects_latest_mutations() {
        let mut db = Database::new();
        db.insert(1, bb(0.0, 0.0, 1.0, 1.0), vec![]);
        db.insert(2, bb(10.0, 10.0, 11.0, 11.0), vec![]);
        assert_eq!(db.query_bbox(bb(0.5, 0.5, 0.9, 0.9)), vec![1]);
        db.delete(1);
        assert!(db.query_bbox(bb(0.5, 0.5, 0.9, 0.9)).is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp_path = std::env::temp_dir().join(format!(
            "shilpid-test-{}-{:?}.vdb",
            std::process::id(),
            std::thread::current().id()
        ));

        let mut db = Database::new();
        db.insert(1, bb(0.0, 0.0, 1.0, 1.0), b"hello".to_vec());
        db.insert(7, bb(4.0, 4.0, 9.0, 9.0), b"world".to_vec());
        db.save(&tmp_path).unwrap();

        let mut back = Database::load(&tmp_path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.get(1).unwrap().1, b"hello");
        assert_eq!(back.query_bbox(bb(4.5, 4.5, 5.0, 5.0)), vec![7]);

        let _ = fs::remove_file(&tmp_path);
    }
}
