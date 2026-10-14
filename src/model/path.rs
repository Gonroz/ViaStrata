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
//!   The right default for roads, whose centerline should curve through the
//!   user's clicks.
//! * **Corner** (`new_corner`) — `auto: false`, both handles zero. The right
//!   default for pavement edges, which are straight unless the user pulls a
//!   handle out.

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
        }
    }

    /// Creates a *corner* anchor. Handles are zero and won't be recomputed —
    /// consecutive corners produce straight edges. Use for pavements. To
    /// curve a corner, the user Alt-drags it (`App::pull_out_handles`).
    pub fn new_corner(node: NodeId) -> Self {
        Self {
            node,
            in_handle: Vec2::ZERO,
            out_handle: Vec2::ZERO,
            auto: false,
        }
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
/// # Straight-segment optimization
///
/// If both handles on a segment are zero, the cubic Bézier degenerates to
/// the straight line between the two anchor positions — but the *naive*
/// subdivision into `samples_per_segment` points along that line produces a
/// run of nearly-colinear vertices. Those vertices are a problem for
/// downstream triangulation (ear-clipping, in particular, struggles with
/// polygons that have many near-degenerate corners), so we detect the case
/// and emit just the endpoint.
///
/// The result for a corner-only polygon is that the polyline has exactly
/// one vertex per anchor — a clean, minimal polygon.
///
/// If `closed`, an additional segment connects the last anchor back to the
/// first. The returned polyline *includes* the closing endpoint (equal to
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

        // Straight segment: no curvature to sample. Emit just the endpoint.
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

/// Recomputes handles for every anchor with `auto == true`, based on the
/// current positions of its neighbours in the same path.
///
/// The derivation is the standard Catmull-Rom to Bézier conversion:
///
/// * Interior anchor: `tangent = (next − prev) / 6`
/// * Endpoint (open path): `tangent = (neighbour − self) / 3`
/// * Closed path: every anchor is interior
///
/// Anchors with `auto == false` are left completely untouched — that's what
/// keeps a corner-only pavement straight no matter how its nodes move.
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

        let tangent = match (has_prev, has_next) {
            (true, true) => (positions[next] - positions[prev]) / 6.0,
            (false, true) => (positions[next] - positions[i]) / 3.0,
            (true, false) => (positions[i] - positions[prev]) / 3.0,
            (false, false) => Vec2::ZERO,
        };

        anchors[i].out_handle = tangent;
        anchors[i].in_handle = -tangent;
    }
}