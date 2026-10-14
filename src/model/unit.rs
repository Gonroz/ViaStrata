//! Display units.

/// Display unit for the user. This is a *presentation* concern only —
/// `lane_m`, `px_per_m`, and the calibration distance's metric value are all
/// stored internally in meters, and converted at the boundary.
#[derive(PartialEq, Clone, Copy)]
pub enum Unit {
    Feet,
    Meters,
}

impl Unit {
    /// Meters per one of this unit. Multiply a value in this unit to get
    /// meters; divide a meter value to get this unit.
    pub fn to_m(self) -> f32 {
        match self {
            Unit::Feet => 0.3048,
            Unit::Meters => 1.0,
        }
    }

    /// Short suffix used in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Feet => "ft",
            Unit::Meters => "m",
        }
    }
}