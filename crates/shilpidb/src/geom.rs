//! Minimal 2D geometry for the spatial index — a point and an axis-aligned box.
//!
//! Kept tiny and dependency-free so the engine stays domain-agnostic: a host
//! with its own richer geometry converts to these at the index boundary.

/// A 2D point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned bounding box. `min` should be componentwise ≤ `max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub min: Point,
    pub max: Point,
}

impl Bbox {
    pub const fn new(min: Point, max: Point) -> Self {
        Self { min, max }
    }

    /// The box spanning `(x0,y0)`–`(x1,y1)`, corners in any order.
    pub fn from_corners(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            min: Point::new(x0.min(x1), y0.min(y1)),
            max: Point::new(x0.max(x1), y0.max(y1)),
        }
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// True when this box and `other` overlap (touching edges count as overlap).
    pub fn intersects(&self, other: Bbox) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}
