//! Lane anchors: snap points at the ends of road segments.
//!
//! Each road endpoint exposes a small constellation of anchors:
//!
//! * **Main** — the road centerline endpoint. This is an ordinary node
//!   in the graph and can be dragged to move the road.
//! * **LaneCenter(i)** — the center of lane `i` in the endpoint's
//!   cross-section. Offset laterally from the main anchor.
//! * **Boundary(i)** — the paint line between lanes `i-1` and `i`.
//!   `Boundary(0)` and `Boundary(lanes.len())` are the outer edges.
//!
//! Only the main anchor moves the road. The others are pure snap targets:
//! placing a new segment's first point on a lane anchor positions that
//! segment precisely, and (if the user wants) inherits the source's
//! direction so the two continue smoothly.
//!
//! Lane anchors are *derived* — computed fresh from the road's current
//! geometry every frame. They are not stored in the node graph. Only the
//! main anchor (which is the road's endpoint node) exists in the graph.

use eframe::egui::{Pos2, Vec2};

/// Which end of a road an anchor belongs to.
///
/// Direction semantics are draw-order: `Start` is the road's first anchor,
/// `End` is its last. If you draw a road westward, `Start` is the east end
/// and `End` is the west end. That's how the doc's tangents are meant to
/// be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadEnd {
    Start,
    End,
}

impl RoadEnd {
    pub fn is_start(self) -> bool {
        matches!(self, Self::Start)
    }

    pub fn is_end(self) -> bool {
        matches!(self, Self::End)
    }
}

/// What kind of anchor this is.
///
/// The main anchor is the road's own node. Lane centers and boundaries
/// are derived snap targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorKind {
    /// The road centerline endpoint.
    Main,
    /// The center of lane `i` in the endpoint's cross-section.
    LaneCenter(usize),
    /// The paint line between lanes `i-1` and `i`. `Boundary(0)` and
    /// `Boundary(lanes.len())` are the two outer road edges.
    Boundary(usize),
}

/// One computed lane anchor at a road endpoint.
///
/// Positions and tangents are in image-pixel space, matching the rest of
/// the app's internal representation. `outgoing_tangent` is a unit vector
/// pointing in the direction a new road attached here would travel: away
/// from the source road, not toward it.
#[derive(Clone, Copy, Debug)]
pub struct LaneAnchor {
    pub position: Pos2,
    pub kind: AnchorKind,
    pub outgoing_tangent: Vec2,
}