//! The drawing primitives ShilpiDB stores: point, polyline, arc, and text.
//!
//! Each variant carries its 2D geometry in drawing units (see [`Units`]), knows
//! its axis-aligned [`Bbox`] (what the spatial index keys on), and round-trips
//! through the `shilpidb` binary [`codec`]. Coordinates are `f64`; angles are
//! radians, counter-clockwise, measured from the +x axis.
//!
//! [`Units`]: crate::Units
//! [`codec`]: shilpidb::Writer

use std::f64::consts::TAU;

use shilpidb::{Bbox, Error, Point, Reader, Writer};

/// How wide one text character is assumed to be, as a fraction of its height —
/// a coarse advance-width estimate so a text label has a sensible bounding box
/// without font metrics. A real renderer refines this; the index only needs a
/// conservative box.
const TEXT_ADVANCE_FACTOR: f64 = 0.6;

// Geometry opcodes, written as the first byte of an encoded primitive.
const GEO_POINT: u8 = 1;
const GEO_POLYLINE: u8 = 2;
const GEO_ARC: u8 = 3;
const GEO_TEXT: u8 = 4;

/// A single 2D drawing primitive.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A single point.
    Point(Point),
    /// A connected run of vertices; `closed` joins the last back to the first.
    Polyline { points: Vec<Point>, closed: bool },
    /// A circular arc: `start_angle` → `end_angle` (radians, CCW). A full
    /// circle is `start_angle = 0`, `end_angle = 2π`.
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// A text label anchored at `at`, drawn up and to the right at `height`.
    Text {
        at: Point,
        height: f64,
        content: String,
    },
}

impl Geometry {
    /// The primitive's axis-aligned bounding box — what the spatial index uses.
    pub fn bbox(&self) -> Bbox {
        match self {
            Geometry::Point(p) => Bbox::from_corners(p.x, p.y, p.x, p.y),
            Geometry::Polyline { points, .. } => bbox_of_points(points),
            Geometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => arc_bbox(*center, *radius, *start_angle, *end_angle),
            Geometry::Text {
                at,
                height,
                content,
            } => {
                let width = height * TEXT_ADVANCE_FACTOR * content.chars().count() as f64;
                Bbox::from_corners(at.x, at.y, at.x + width, at.y + height)
            }
        }
    }

    /// Append this primitive's encoding (opcode + fields) to `w`.
    pub fn encode(&self, w: &mut Writer) {
        match self {
            Geometry::Point(p) => {
                w.u8(GEO_POINT);
                w.f64(p.x);
                w.f64(p.y);
            }
            Geometry::Polyline { points, closed } => {
                w.u8(GEO_POLYLINE);
                w.bool(*closed);
                w.u32(points.len() as u32);
                for p in points {
                    w.f64(p.x);
                    w.f64(p.y);
                }
            }
            Geometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                w.u8(GEO_ARC);
                w.f64(center.x);
                w.f64(center.y);
                w.f64(*radius);
                w.f64(*start_angle);
                w.f64(*end_angle);
            }
            Geometry::Text {
                at,
                height,
                content,
            } => {
                w.u8(GEO_TEXT);
                w.f64(at.x);
                w.f64(at.y);
                w.f64(*height);
                w.str(content);
            }
        }
    }

    /// Encode to a fresh byte vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.encode(&mut w);
        w.into_bytes()
    }

    /// Decode a primitive previously written by [`encode`](Self::encode).
    pub fn decode(r: &mut Reader) -> Result<Self, Error> {
        let op = r.u8()?;
        match op {
            GEO_POINT => Ok(Geometry::Point(Point::new(r.f64()?, r.f64()?))),
            GEO_POLYLINE => {
                let closed = r.bool()?;
                let n = r.u32()? as usize;
                let mut points = Vec::with_capacity(n);
                for _ in 0..n {
                    points.push(Point::new(r.f64()?, r.f64()?));
                }
                Ok(Geometry::Polyline { points, closed })
            }
            GEO_ARC => Ok(Geometry::Arc {
                center: Point::new(r.f64()?, r.f64()?),
                radius: r.f64()?,
                start_angle: r.f64()?,
                end_angle: r.f64()?,
            }),
            GEO_TEXT => Ok(Geometry::Text {
                at: Point::new(r.f64()?, r.f64()?),
                height: r.f64()?,
                content: r.str()?,
            }),
            other => Err(Error::Format(format!("unknown geometry opcode {other}"))),
        }
    }
}

fn bbox_of_points(points: &[Point]) -> Bbox {
    let mut it = points.iter();
    let Some(first) = it.next() else {
        return Bbox::from_corners(0.0, 0.0, 0.0, 0.0);
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for p in it {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    Bbox::from_corners(min_x, min_y, max_x, max_y)
}

/// Tight bounding box of a CCW arc: its two endpoints, plus whichever of the
/// four cardinal extreme points (angles 0, π/2, π, 3π/2) the sweep passes
/// through.
fn arc_bbox(center: Point, radius: f64, start_angle: f64, end_angle: f64) -> Bbox {
    let at = |angle: f64| {
        Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        )
    };

    // Normalize the sweep to a positive CCW span in (0, 2π].
    let mut sweep = end_angle - start_angle;
    if sweep <= 0.0 {
        sweep += TAU;
    }
    let sweep = sweep.min(TAU);

    let mut pts = vec![at(start_angle), at(end_angle)];
    for k in 0..4 {
        let cardinal = k as f64 * (TAU / 4.0);
        // How far `cardinal` is past `start_angle`, wrapped into [0, 2π).
        let mut rel = (cardinal - start_angle) % TAU;
        if rel < 0.0 {
            rel += TAU;
        }
        if rel <= sweep {
            pts.push(at(cardinal));
        }
    }
    bbox_of_points(&pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn assert_box(bb: Bbox, x0: f64, y0: f64, x1: f64, y1: f64) {
        assert!(approx(bb.min.x, x0), "min.x {} vs {x0}", bb.min.x);
        assert!(approx(bb.min.y, y0), "min.y {} vs {y0}", bb.min.y);
        assert!(approx(bb.max.x, x1), "max.x {} vs {x1}", bb.max.x);
        assert!(approx(bb.max.y, y1), "max.y {} vs {y1}", bb.max.y);
    }

    fn round_trip(g: &Geometry) -> Geometry {
        let bytes = g.to_bytes();
        let mut r = Reader::new(&bytes);
        Geometry::decode(&mut r).unwrap()
    }

    #[test]
    fn all_variants_round_trip() {
        let variants = vec![
            Geometry::Point(Point::new(1.5, -2.0)),
            Geometry::Polyline {
                points: vec![
                    Point::new(0.0, 0.0),
                    Point::new(10.0, 0.0),
                    Point::new(10.0, 5.0),
                ],
                closed: true,
            },
            Geometry::Arc {
                center: Point::new(2.0, 3.0),
                radius: 4.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::PI,
            },
            Geometry::Text {
                at: Point::new(1.0, 1.0),
                height: 2.5,
                content: "Room 101".to_string(),
            },
        ];
        for g in &variants {
            assert_eq!(&round_trip(g), g);
        }
    }

    #[test]
    fn unknown_opcode_is_a_format_error() {
        let mut r = Reader::new(&[99u8]);
        assert!(matches!(Geometry::decode(&mut r), Err(Error::Format(_))));
    }

    #[test]
    fn point_and_polyline_bbox() {
        assert_box(
            Geometry::Point(Point::new(3.0, 4.0)).bbox(),
            3.0,
            4.0,
            3.0,
            4.0,
        );
        let poly = Geometry::Polyline {
            points: vec![
                Point::new(-1.0, 2.0),
                Point::new(5.0, -3.0),
                Point::new(0.0, 0.0),
            ],
            closed: false,
        };
        assert_box(poly.bbox(), -1.0, -3.0, 5.0, 2.0);
    }

    #[test]
    fn quarter_arc_bbox_is_tight() {
        // Unit circle, 0 → 90°: tight box is [0,1] x [0,1].
        let arc = arc_bbox(Point::new(0.0, 0.0), 1.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert_box(arc, 0.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn semicircle_bbox_includes_top_extreme() {
        // 0 → 180°: box is [-1,1] x [0,1] (top of the circle is swept).
        let arc = arc_bbox(Point::new(0.0, 0.0), 1.0, 0.0, std::f64::consts::PI);
        assert_box(arc, -1.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn full_circle_bbox_is_the_whole_disc() {
        let arc = arc_bbox(Point::new(2.0, 3.0), 5.0, 0.0, TAU);
        assert_box(arc, -3.0, -2.0, 7.0, 8.0);
    }

    #[test]
    fn text_bbox_grows_with_content() {
        let short = Geometry::Text {
            at: Point::new(0.0, 0.0),
            height: 10.0,
            content: "A".to_string(),
        }
        .bbox();
        let long = Geometry::Text {
            at: Point::new(0.0, 0.0),
            height: 10.0,
            content: "AAAA".to_string(),
        }
        .bbox();
        assert!(long.width() > short.width());
        assert!(approx(short.height(), 10.0));
    }
}
