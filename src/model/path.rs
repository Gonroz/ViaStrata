//! Paths built from anchors and Bézier handles.
//!
//! A *path* is a sequence of anchors. Each anchor references a position in
//! the node graph, plus two optional direction handles that pull the curve
//! on either side. Two paths that share a node share the position but have
//! independent handles — which is what makes a T-junction work.
//!
//! Roads and pavements both use this representation. The only difference at
//! the geometry level is that pavements are closed.
//!
//! # Corner versus smooth defaults
//!
//! * **Smooth** (`new`) — `auto: true`, handles computed from neighbours.
//! * **Corner** (`new_corner`) — `auto: false`, both handles zero.
//!
//! # Preferred tangent
//!
//! An anchor can carry a `preferred_tangent` — a unit direction that
//! `recompute_auto_handles` uses for the outgoing direction instead of the
//! normal Catmull-Rom derivative. This is how "inherit the source road's
//! direction" is expressed: when the user starts a new road on a lane
//! anchor, the new road's first anchor gets a preferred tangent, and the
//! resulting curve leaves the source smoothly in that direction rather
//! than pointing straight at the user's next click.

use eframe::egui::{Pos2, Vec2};

use super::graph::{NodeGraph, NodeId};
use crate::spline;

/// One anchor point in a path.
#[derive(Clone, Copy, Debug)]
pub struct PathAnchor {
    pub node: NodeId,
    pub in_handle: Vec2,
    pub out_handle: Vec2,
    pub auto: bool,
    /// Optional unit direction the anchor prefers to leave in. Only
    /// consulted when `auto` is true; manual handles ignore it. Set when a
    /// road is anchored to a lane anchor on another road.
    pub preferred_tangent: Option<Vec2>,
}

impl PathAnchor {
    /// Creates a *smooth* anchor. Handles will be computed from neighbours
    /// by `recompute_auto_handles`. Use for roads.
    pub fn new(node: NodeId) -> Self {
        Self {
            node,
            in_handle: Vec2::ZERO,
            out_handle: Vec2::ZERO,
            auto: true,
            preferred_tangent: None,
        }
    }

    /// Creates a *corner* anchor. Handles are zero and won't be recomputed —
    /// consecutive corners produce straight edges. Use for pavements.
    pub fn new_corner(node: NodeId) -> Self {
        Self {
            node,
            in_handle: Vec2::ZERO,
            out_handle: Vec2::ZERO,
            auto: false,
            preferred_tangent: None,
        }
    }

    /// Marks this anchor as preferring a specific outgoing direction.
    /// Does not clear `auto` — the preferred tangent is used *by* the auto
    /// computation, it doesn't override it.
    pub fn with_preferred_tangent(mut self, tangent: Vec2) -> Self {
        self.preferred_tangent = Some(tangent);
        self
    }
}

/// Which side of an anchor a handle sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleSide {
    In,
    Out,
}

/// Identifies a specific handle on a specific path.
#[derive(Clone, Copy, Debug)]
pub struct HandleRef {
    pub owner: PathOwner,
    pub anchor_index: usize,
    pub side: HandleSide,
}

impl HandleRef {
    pub fn anchor_ref(self) -> AnchorRef {
        AnchorRef {
            owner: self.owner,
            index: self.anchor_index,
        }
    }
}

/// Identifies a specific anchor on a specific path.
#[derive(Clone, Copy, Debug)]
pub struct AnchorRef {
    pub owner: PathOwner,
    pub index: usize,
}

/// Which path a reference points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathOwner {
    Road(usize),
    Pavement(usize),
}

/// Tessellates a path of anchors into a dense polyline using cubic Béziers.
///
/// Each segment between anchor `i` and anchor `i+1` uses:
///
/// ```text
///     C1 = anchor[i].pos   + anchor[i].out_handle
///     C2 = anchor[i+1].pos + anchor[i+1].in_handle
/// ```
///
/// Straight-segment optimization: if both handles on a segment are zero,
/// emit just the endpoint instead of subdividing a straight line into
/// many nearly-colinear samples. Keeps corner-only polygons clean.
///
/// If `closed`, an additional segment connects the last anchor back to the
/// first. The returned polyline includes the closing endpoint (equal to
/// the first point) so callers can hand it directly to a fill routine;
/// `paint_pavement` trims the duplicate before triangulating.
///
/// Anchors whose node no longer exists in the graph are skipped silently.
pub fn tessellate(
    anchors: &[PathAnchor],
    graph: &NodeGraph,
    closed: bool,
    samples_per_segment: usize,
) -> Vec<Pos2> {
    if anchors.is_empty() {
        return Vec::new();
    }
    if anchors.len() == 1 {
        return match graph.pos(anchors[0].node) {
            Some(p) => vec![p],
            None => Vec::new(),
        };
    }

    let n = anchors.len();
    let segment_count = if closed { n } else { n - 1 };

    let mut out = Vec::new();
    let Some(first) = graph.pos(anchors[0].node) else {
        return out;
    };
    out.push(first);

    for i in 0..segment_count {
        let a = &anchors[i];
        let b = &anchors[(i + 1) % n];
        let Some(pa) = graph.pos(a.node) else { continue };
        let Some(pb) = graph.pos(b.node) else { continue };

        if a.out_handle.length_sq() < 1e-9 && b.in_handle.length_sq() < 1e-9 {
            out.push(pb);
            continue;
        }

        let c1 = pa + a.out_handle;
        let c2 = pb + b.in_handle;

        for s in 1..=samples_per_segment {
            let t = s as f32 / samples_per_segment as f32;
            out.push(spline::cubic_bezier_point(pa, c1, c2, pb, t));
        }
    }

    out
}

/// Recomputes handles for every anchor with `auto == true`.
///
/// Normal path: Catmull-Rom tangent, which is `(next − prev) / 6` for an
/// interior anchor and `(neighbour − self) / 3` for an endpoint.
///
/// Preferred tangent override: if the anchor has a preferred direction and
/// `auto == true`, use that unit vector as the outgoing direction and
/// scale it by `distance_to_next / 3`. This is what makes "inherit the
/// source road's direction" work — the curve leaves in the preferred
/// direction and bends toward the next click.
///
/// Anchors with `auto == false` are left completely untouched.
pub fn recompute_auto_handles(anchors: &mut [PathAnchor], graph: &NodeGraph, closed: bool) {
    let n = anchors.len();
    if n < 2 {
        return;
    }

    let positions: Vec<Pos2> = anchors
        .iter()
        .map(|a| graph.pos(a.node).unwrap_or(Pos2::ZERO))
        .collect();

    for i in 0..n {
        if !anchors[i].auto {
            continue;
        }

        let has_prev = closed || i > 0;
        let has_next = closed || i < n - 1;
        let prev = if i == 0 { n - 1 } else { i - 1 };
        let next = if i == n - 1 { 0 } else { i + 1 };

        let tangent = if let Some(pref) = anchors[i].preferred_tangent {
            let dist = if has_next {
                (positions[next] - positions[i]).length()
            } else if has_prev {
                (positions[i] - positions[prev]).length()
            } else {
                0.0
            };
            let pref_len = pref.length();
            let unit = if pref_len > 1e-6 {
                pref / pref_len
            } else {
                Vec2::ZERO
            };
            unit * dist / 3.0
        } else {
            match (has_prev, has_next) {
                (true, true) => (positions[next] - positions[prev]) / 6.0,
                (false, true) => (positions[next] - positions[i]) / 3.0,
                (true, false) => (positions[i] - positions[prev]) / 3.0,
                (false, false) => Vec2::ZERO,
            }
        };

        anchors[i].out_handle = tangent;
        anchors[i].in_handle = -tangent;
    }
}