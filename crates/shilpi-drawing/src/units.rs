//! Drawing units. AORMS works in millimetres, so that's the default.

use shilpidb::{Error, Reader, Writer};

/// The unit that a drawing's coordinates are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Units {
    #[default]
    Millimeter,
    Meter,
    Inch,
    Foot,
    /// No physical unit (raw model coordinates).
    Unitless,
}

impl Units {
    fn tag(self) -> u8 {
        match self {
            Units::Millimeter => 0,
            Units::Meter => 1,
            Units::Inch => 2,
            Units::Foot => 3,
            Units::Unitless => 4,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, Error> {
        match tag {
            0 => Ok(Units::Millimeter),
            1 => Ok(Units::Meter),
            2 => Ok(Units::Inch),
            3 => Ok(Units::Foot),
            4 => Ok(Units::Unitless),
            other => Err(Error::Format(format!("unknown units tag {other}"))),
        }
    }

    pub(crate) fn write(self, w: &mut Writer) {
        w.u8(self.tag());
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Self, Error> {
        Self::from_tag(r.u8()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip() {
        for u in [
            Units::Millimeter,
            Units::Meter,
            Units::Inch,
            Units::Foot,
            Units::Unitless,
        ] {
            let mut w = Writer::new();
            u.write(&mut w);
            let bytes = w.into_bytes();
            let mut r = Reader::new(&bytes);
            assert_eq!(Units::read(&mut r).unwrap(), u);
        }
    }

    #[test]
    fn default_is_millimeter() {
        assert_eq!(Units::default(), Units::Millimeter);
    }

    #[test]
    fn unknown_tag_errors() {
        let mut r = Reader::new(&[200u8]);
        assert!(Units::read(&mut r).is_err());
    }
}
