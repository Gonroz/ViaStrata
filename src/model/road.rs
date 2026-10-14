//! Roads and their marking styles.

use super::path::PathAnchor;

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

/// A road is a sequence of path anchors plus real-world width and marking
/// style. Width is derived per frame from `lane_m × lanes × px_per_m × zoom`,
/// so a road keeps its physical size at any zoom.
pub struct Road {
    pub anchors: Vec<PathAnchor>,
    pub lanes: u32,
    pub lane_m: f32,
    pub center: CenterStyle,
}