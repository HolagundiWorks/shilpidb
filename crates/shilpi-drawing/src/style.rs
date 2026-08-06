//! Layers and their display style. Entities reference a layer by index; styling
//! (like CAD) is layer-driven rather than per-entity for now.

use shilpidb::{Error, Reader, Writer};

/// An 8-bit-per-channel RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    /// Opaque black.
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);

    pub(crate) fn write(self, w: &mut Writer) {
        w.u8(self.r);
        w.u8(self.g);
        w.u8(self.b);
        w.u8(self.a);
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Self, Error> {
        Ok(Rgba::new(r.u8()?, r.u8()?, r.u8()?, r.u8()?))
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::BLACK
    }
}

/// A named, styleable container of entities (entities point at it by index).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub color: Rgba,
    pub stroke_width: f64,
    pub visible: bool,
}

impl Layer {
    /// A layer with the given name and sensible defaults (black, hairline,
    /// visible).
    pub fn new(name: impl Into<String>) -> Self {
        Layer {
            name: name.into(),
            color: Rgba::BLACK,
            stroke_width: 0.0,
            visible: true,
        }
    }

    /// Builder: set the stroke color.
    pub fn with_color(mut self, color: Rgba) -> Self {
        self.color = color;
        self
    }

    /// Builder: set the stroke width (in drawing units).
    pub fn with_stroke_width(mut self, width: f64) -> Self {
        self.stroke_width = width;
        self
    }

    pub(crate) fn write(&self, w: &mut Writer) {
        w.str(&self.name);
        self.color.write(w);
        w.f64(self.stroke_width);
        w.bool(self.visible);
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Self, Error> {
        Ok(Layer {
            name: r.str()?,
            color: Rgba::read(r)?,
            stroke_width: r.f64()?,
            visible: r.bool()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_round_trips() {
        let layer = Layer::new("walls")
            .with_color(Rgba::new(10, 20, 30, 255))
            .with_stroke_width(0.5);
        let mut w = Writer::new();
        layer.write(&mut w);
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(Layer::read(&mut r).unwrap(), layer);
    }
}
