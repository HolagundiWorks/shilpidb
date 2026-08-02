//! A built-in **spatial index**: a uniform grid over record bounding boxes.
//!
//! Records are identified by an opaque `u64` id (the host maps its own key to a
//! `u64`). The index's spatial data (cell size + per-record boxes) serializes
//! with the binary codec so it persists inside a database file; the bucket
//! acceleration is rebuilt from it on load.
//!
//! Queries return candidate ids narrowed by the grid, then filtered to a true
//! box overlap — `O(cells + hits)` instead of a linear scan.

use std::collections::{HashMap, HashSet};

use crate::codec::{Error, Reader, Writer};
use crate::geom::{Bbox, Point};

/// A record spanning more grid cells than this is held in an overflow list
/// instead of being smeared across every cell it touches.
const MAX_CELLS_PER_ENTITY: i64 = 256;
/// A query rectangle covering more cells than this falls back to scanning the
/// (already small) box list rather than walking a huge cell range.
const MAX_QUERY_CELLS: i64 = 8192;

/// A uniform-grid spatial index over `(u64, Bbox)` records.
#[derive(Debug, Clone)]
pub struct SpatialGrid {
    cell: f64,
    boxes: Vec<(u64, Bbox)>,
    buckets: HashMap<(i64, i64), Vec<usize>>,
    oversized: Vec<usize>,
}

fn cell_of(v: f64, cell: f64) -> i64 {
    (v / cell).floor() as i64
}

impl SpatialGrid {
    /// Build the index over record bounding boxes, choosing a sensible cell size.
    pub fn build(boxes: Vec<(u64, Bbox)>) -> Self {
        let cell = choose_cell(&boxes);
        let mut grid = SpatialGrid {
            cell,
            boxes,
            buckets: HashMap::new(),
            oversized: Vec::new(),
        };
        grid.reindex();
        grid
    }

    /// Rebuild the bucket structure from `cell` + `boxes`.
    fn reindex(&mut self) {
        self.buckets.clear();
        self.oversized.clear();
        for (idx, (_, bb)) in self.boxes.iter().enumerate() {
            let (cx0, cy0, cx1, cy1) = cell_bounds(*bb, self.cell);
            let span = (cx1 - cx0 + 1).saturating_mul(cy1 - cy0 + 1);
            if span > MAX_CELLS_PER_ENTITY {
                self.oversized.push(idx);
                continue;
            }
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    self.buckets.entry((cx, cy)).or_default().push(idx);
                }
            }
        }
    }

    /// Number of indexed records.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Whether the index holds no records.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// The grid cell size.
    pub fn cell_size(&self) -> f64 {
        self.cell
    }

    /// Record ids whose bounding box intersects `rect`, ascending and deduped.
    pub fn query_bbox(&self, rect: Bbox) -> Vec<u64> {
        let mut seen: HashSet<usize> = HashSet::new();
        for &idx in &self.oversized {
            seen.insert(idx);
        }
        let (cx0, cy0, cx1, cy1) = cell_bounds(rect, self.cell);
        let span = (cx1 - cx0 + 1).saturating_mul(cy1 - cy0 + 1);
        if span > MAX_QUERY_CELLS {
            // Degenerate huge query — consider every box (still correct).
            for idx in 0..self.boxes.len() {
                seen.insert(idx);
            }
        } else {
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    if let Some(v) = self.buckets.get(&(cx, cy)) {
                        for &idx in v {
                            seen.insert(idx);
                        }
                    }
                }
            }
        }
        let mut ids: Vec<u64> = seen
            .into_iter()
            .filter(|&idx| self.boxes[idx].1.intersects(rect))
            .map(|idx| self.boxes[idx].0)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Record ids whose box is within `tol` of point `p` (a small-box query).
    pub fn query_point(&self, p: Point, tol: f64) -> Vec<u64> {
        let t = tol.abs();
        self.query_bbox(Bbox {
            min: Point::new(p.x - t, p.y - t),
            max: Point::new(p.x + t, p.y + t),
        })
    }

    /// Serialize the index's spatial data (cell size + record boxes).
    pub fn write(&self, w: &mut Writer) {
        w.f64(self.cell);
        w.u32(self.boxes.len() as u32);
        for (id, bb) in &self.boxes {
            w.u64(*id);
            w.f64(bb.min.x);
            w.f64(bb.min.y);
            w.f64(bb.max.x);
            w.f64(bb.max.y);
        }
    }

    /// Read an index written by [`write`](Self::write) and rebuild its buckets.
    pub fn read(r: &mut Reader) -> Result<Self, Error> {
        let mut cell = r.f64()?;
        if !(cell.is_finite() && cell > 0.0) {
            cell = 1.0;
        }
        let n = r.u32()? as usize;
        let mut boxes = Vec::with_capacity(n);
        for _ in 0..n {
            let id = r.u64()?;
            let min = Point::new(r.f64()?, r.f64()?);
            let max = Point::new(r.f64()?, r.f64()?);
            boxes.push((id, Bbox { min, max }));
        }
        let mut grid = SpatialGrid {
            cell,
            boxes,
            buckets: HashMap::new(),
            oversized: Vec::new(),
        };
        grid.reindex();
        Ok(grid)
    }
}

fn cell_bounds(bb: Bbox, cell: f64) -> (i64, i64, i64, i64) {
    (
        cell_of(bb.min.x, cell),
        cell_of(bb.min.y, cell),
        cell_of(bb.max.x, cell),
        cell_of(bb.max.y, cell),
    )
}

/// Pick a cell size so a typical record spans roughly one cell.
fn choose_cell(boxes: &[(u64, Bbox)]) -> f64 {
    if boxes.is_empty() {
        return 1.0;
    }
    let mut sum = 0.0;
    let mut overall: Option<Bbox> = None;
    for (_, bb) in boxes {
        sum += bb.width().abs().max(bb.height().abs());
        overall = Some(match overall {
            None => *bb,
            Some(o) => Bbox {
                min: Point::new(o.min.x.min(bb.min.x), o.min.y.min(bb.min.y)),
                max: Point::new(o.max.x.max(bb.max.x), o.max.y.max(bb.max.y)),
            },
        });
    }
    let avg = sum / boxes.len() as f64;
    let diag = overall
        .map(|o| (o.width().abs().powi(2) + o.height().abs().powi(2)).sqrt())
        .unwrap_or(0.0);
    // Prefer average record size; fall back to a fraction of the extent; never 0.
    let by_extent = diag / (boxes.len() as f64).sqrt().max(1.0);
    avg.max(by_extent).max(1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(id: u64, x0: f64, y0: f64, x1: f64, y1: f64) -> (u64, Bbox) {
        (id, Bbox::from_corners(x0, y0, x1, y1))
    }

    fn brute(boxes: &[(u64, Bbox)], rect: Bbox) -> Vec<u64> {
        let mut v: Vec<u64> = boxes
            .iter()
            .filter(|(_, b)| b.intersects(rect))
            .map(|(id, _)| *id)
            .collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn query_matches_brute_force() {
        let boxes = vec![
            bx(1, 0.0, 0.0, 1.0, 1.0),
            bx(2, 5.0, 5.0, 6.0, 6.0),
            bx(3, 2.0, 2.0, 3.0, 3.0),
            bx(4, -10.0, -10.0, 20.0, 20.0), // spans many cells
            bx(5, 100.0, 100.0, 100.5, 100.5),
        ];
        let grid = SpatialGrid::build(boxes.clone());
        for rect in [
            Bbox::from_corners(0.5, 0.5, 2.5, 2.5),
            Bbox::from_corners(-1.0, -1.0, 0.1, 0.1),
            Bbox::from_corners(99.0, 99.0, 101.0, 101.0),
            Bbox::from_corners(-1000.0, -1000.0, 1000.0, 1000.0),
        ] {
            assert_eq!(grid.query_bbox(rect), brute(&boxes, rect), "rect {rect:?}");
        }
    }

    #[test]
    fn oversized_record_is_always_a_candidate() {
        let boxes = vec![
            bx(1, -1000.0, -1000.0, 1000.0, 1000.0),
            bx(2, 0.0, 0.0, 1.0, 1.0),
        ];
        let grid = SpatialGrid::build(boxes);
        let hit = grid.query_bbox(Bbox::from_corners(0.2, 0.2, 0.3, 0.3));
        assert_eq!(hit, vec![1, 2]);
    }

    #[test]
    fn round_trips_through_bytes() {
        let boxes = vec![bx(1, 0.0, 0.0, 1.0, 1.0), bx(7, 4.0, 4.0, 9.0, 9.0)];
        let grid = SpatialGrid::build(boxes);
        let mut w = Writer::new();
        grid.write(&mut w);
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        let back = SpatialGrid::read(&mut r).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(
            back.query_bbox(Bbox::from_corners(4.5, 4.5, 5.0, 5.0)),
            vec![7]
        );
    }

    #[test]
    fn point_query_finds_enclosing_box() {
        let boxes = vec![bx(1, 0.0, 0.0, 2.0, 2.0), bx(2, 10.0, 10.0, 12.0, 12.0)];
        let grid = SpatialGrid::build(boxes);
        assert_eq!(grid.query_point(Point::new(1.0, 1.0), 0.01), vec![1]);
        assert!(grid.query_point(Point::new(50.0, 50.0), 0.01).is_empty());
    }
}
