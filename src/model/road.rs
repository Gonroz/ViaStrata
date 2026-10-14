//! Roads and their marking styles.

use super::lane::{total_width_m, Lane};
use super::path::PathAnchor;

/// How the center of a two-way road is marked.
///
/// Applied at every boundary in the cross-section where adjacent travel
/// lanes face opposite directions. For the common case — one row of
/// forward lanes next to one row of backward lanes — that's exactly one
/// boundary, and this style draws there.
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

/// A road is a sequence of path anchors plus a cross-section (ordered lanes)
/// and a center-line style.
///
/// Width is derived per frame from the sum of lane widths scaled by
/// `px_per_m × zoom`, so a road keeps its physical size at any zoom.
pub struct Road {
    pub anchors: Vec<PathAnchor>,
    pub lanes: Vec<Lane>,
    pub center: CenterStyle,
}

impl Road {
    /// Total cross-section width in meters.
    pub fn total_width_m(&self) -> f32 {
        total_width_m(&self.lanes)
    }
}