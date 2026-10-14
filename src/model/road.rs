//! Roads and their marking styles.

use super::NodeId;

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

/// A road is a sequence of *node references* plus real-world width and
/// marking style. The actual positions live in `NodeGraph`, which is what
/// makes two roads that share a node stay glued together when that node is
/// dragged.
pub struct Road {
    pub nodes: Vec<NodeId>,
    pub lanes: u32,
    pub lane_m: f32,
    pub center: CenterStyle,
}