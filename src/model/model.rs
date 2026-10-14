//! Plain data types shared across the app.

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
    /// Click to append control points; Enter/double-click/right-click commits.
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
    /// Meters per one of this unit.
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
/// Only applied to *even* lane counts. Odd-lane roads are asymmetric enough
/// that a single "center line style" can't capture the intent, so the center
/// line is suppressed on those and the lane dividers alone convey structure.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CenterStyle {
    /// No center marking. Use for one-way roads and unmarked streets.
    None,
    /// Single solid yellow — passing not permitted in either direction.
    YellowSolid,
    /// Two parallel solid yellow lines — the most restrictive marking.
    YellowDouble,
    /// Dashed yellow — passing permitted where sight distance allows.
    YellowDashed,
}

impl CenterStyle {
    /// Short label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            CenterStyle::None => "None",
            CenterStyle::YellowSolid => "Solid yellow",
            CenterStyle::YellowDouble => "Double yellow",
            CenterStyle::YellowDashed => "Dashed yellow",
        }
    }
}

/// Stable identifier for a point in the node graph.
///
/// Node IDs are allocated once and never reused, so a reference to
/// `NodeId(n)` is always either valid or deliberately removed — it never
/// silently points at a different location. The `u32` is opaque; don't do
/// arithmetic on it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u32);

/// A road is a sequence of *node references* plus real-world width and
/// marking style. The actual positions live in `App::nodes`, which is what
/// makes two roads that share a node stay glued together when that node is
/// dragged.
pub struct Road {
    pub nodes: Vec<NodeId>,
    pub lanes: u32,
    pub lane_m: f32,
    pub center: CenterStyle,
}