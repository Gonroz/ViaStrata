//! Plain data types shared across the app.

use eframe::egui::Pos2;

/// Which editing tool is currently active.
///
/// Tools are mutually exclusive: only one owns the primary mouse button at a
/// time, and the toolbar's bottom row changes to match.
#[derive(PartialEq, Clone, Copy)]
pub enum Tool {
    /// Drag to pan. No point placement.
    Pan,
    /// Click two points, then enter a real distance to derive pixels-per-meter.
    Calibrate,
    /// Click to append control points; Enter/double-click commits the road.
    Road,
}

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

/// How the center of a two-way road is marked.
///
/// Only applied to *even* lane counts. Odd-lane roads (e.g. a 3-lane with a
/// center turn lane) are asymmetric enough that a single "center line style"
/// can't capture the intent; the center line is suppressed on those and the
/// lane dividers alone convey the structure.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CenterStyle {
    /// No center marking. Use this for one-way roads and unmarked residential
    /// streets, even if `lanes >= 2`.
    None,
    /// Single solid yellow — passing not permitted in either direction.
    YellowSolid,
    /// Two parallel solid yellow lines — the most restrictive marking.
    YellowDouble,
    /// Dashed yellow — passing permitted where sight distance allows.
    YellowDashed,
}

impl CenterStyle {
    /// Short label for the toolbar picker.
    pub fn label(self) -> &'static str {
        match self {
            CenterStyle::None => "None",
            CenterStyle::YellowSolid => "Solid yellow",
            CenterStyle::YellowDouble => "Double yellow",
            CenterStyle::YellowDashed => "Dashed yellow",
        }
    }
}

/// A road is a centerline in *image pixel* coordinates plus a real-world
/// width and a marking style. Width is derived at draw time:
/// `lane_m * lanes * px_per_m`, then scaled by zoom. That way a road stays
/// the correct physical width no matter how far in or out the user zooms.
pub struct Road {
    pub pts: Vec<Pos2>,
    pub lanes: u32,
    pub lane_m: f32,
    pub center: CenterStyle,
}

/// Identifies one editable control point.
///
/// Roads live in two places — committed roads in `App::roads`, and the
/// one-in-progress in `App::current` — and both need to expose their
/// vertices to the canvas's hit-testing. This enum distinguishes them so a
/// single vertex reference can be passed around (drag state, hover state,
/// delete command) without ambiguity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VertexRef {
    /// A vertex on a committed road: `(road index, vertex index)`.
    Committed(usize, usize),
    /// A vertex on the in-progress road: `vertex index` into `App::current`.
    Draft(usize),
}