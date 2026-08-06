//! Build a tiny floor plan, push it into a running `shilpid` in one batch, then
//! query it back and decode a stored record into a typed [`Entity`] — an
//! end-to-end demonstration of the drawing model over the real server.
//!
//! ```text
//! # in one terminal:
//! cargo run -p shilpid -- --bind 127.0.0.1:7420 --data ./floorplan.vdb
//! # in another:
//! cargo run -p shilpi-drawing --example floorplan -- --host 127.0.0.1:7420
//! ```
//!
//! `--host` (or `$SHILPI_HOST`) selects the server; default `127.0.0.1:7420`.

use shilpi_client::Client;
use shilpi_drawing::{Drawing, Entity, Geometry, Layer, Rgba, Units};
use shilpidb::{Bbox, Point};

fn host() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--host" {
            return args.next().expect("--host requires an ADDR:PORT value");
        }
    }
    std::env::var("SHILPI_HOST").unwrap_or_else(|_| "127.0.0.1:7420".to_string())
}

/// A 5m × 3m room (mm units): four walls as a closed polyline, a door swing arc,
/// and a room label.
fn floor_plan() -> Drawing {
    let mut d = Drawing::new(Units::Millimeter);
    let walls = d.add_layer(Layer::new("walls").with_color(Rgba::new(30, 60, 200, 255)));
    let doors = d.add_layer(Layer::new("doors").with_color(Rgba::new(200, 120, 0, 255)));
    let text = d.add_layer(Layer::new("text"));

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
        doors,
        Geometry::Arc {
            center: Point::new(0.0, 0.0),
            radius: 900.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        },
    );
    d.push(
        3,
        text,
        Geometry::Text {
            at: Point::new(2000.0, 1400.0),
            height: 250.0,
            content: "Room 101".to_string(),
        },
    );
    d
}

fn describe(entity: &Entity) -> String {
    match &entity.geometry {
        Geometry::Point(p) => format!("Point at ({}, {})", p.x, p.y),
        Geometry::Polyline { points, closed } => format!(
            "Polyline ({} pts, {})",
            points.len(),
            if *closed { "closed" } else { "open" }
        ),
        Geometry::Arc { radius, .. } => format!("Arc (r={radius})"),
        Geometry::Text { content, .. } => format!("Text {content:?}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = host();
    let mut db = Client::connect(&host)?;
    println!("connected to shilpid at {host}\n");

    let drawing = floor_plan();
    println!(
        "built a {} drawing: {} layers, {} entities",
        format_units(drawing.units),
        drawing.layers.len(),
        drawing.entities.len()
    );

    // Push the whole drawing in a single round-trip.
    db.insert_many(drawing.to_records())?;
    println!("pushed {} entities to the server\n", drawing.entities.len());

    // Spatial query: what's near the bottom-left corner of the room?
    let window = Bbox::from_corners(-1000.0, -1000.0, 1000.0, 1000.0);
    let hits = db.query_bbox(window)?;
    println!("query near the door corner -> ids {hits:?}");

    // Fetch a hit back and decode the opaque record into a typed entity.
    if let Some(&id) = hits.first() {
        if let Some(record) = db.get(id)? {
            let entity = Entity::decode_payload(id, &record.payload)?;
            println!(
                "decoded record {id}: {} on layer {}",
                describe(&entity),
                entity.layer
            );
        }
    }

    // Whole-drawing readback.
    let all = db.list()?;
    println!("\nall entity ids on the server: {all:?}");
    db.save()?;
    println!("saved to the server's data file");
    Ok(())
}

fn format_units(u: Units) -> &'static str {
    match u {
        Units::Millimeter => "millimeter",
        Units::Meter => "meter",
        Units::Inch => "inch",
        Units::Foot => "foot",
        Units::Unitless => "unitless",
    }
}
