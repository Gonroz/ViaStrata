//! Roads, their cross-sections, and per-segment overrides.

use std::borrow::Cow;
use std::collections::HashMap;

use eframe::egui::Pos2;

use super::anchor::{AnchorKind, LaneAnchor, RoadEnd};
use super::graph::NodeGraph;
use super::lane::{total_width_m, Lane};
use super::path::PathAnchor;

/// How the center of a two-way road is marked.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CenterStyle {
    None,
    YellowSolid,
    YellowDouble,
    YellowDashed,
}

impl CenterStyle {
    pub fn label(self) -> &'static str {
        match self {
            CenterStyle::None => "None",
            CenterStyle::YellowSolid => "Solid yellow",
            CenterStyle::YellowDouble => "Double yellow",
            CenterStyle::YellowDashed => "Dashed yellow",
        }
    }
}

/// A cross-section: the ordered lane list plus the center-line style.
#[derive(Clone, PartialEq)]
pub struct SegmentStyle {
    pub lanes: Vec<Lane>,
    pub center: CenterStyle,
}

impl SegmentStyle {
    pub fn total_width_m(&self) -> f32 {
        total_width_m(&self.lanes)
    }
}

/// A road is a sequence of path anchors plus a cross-section, plus any
/// per-segment overrides.
pub struct Road {
    pub anchors: Vec<PathAnchor>,
    pub lanes: Vec<Lane>,
    pub center: CenterStyle,
    pub segment_overrides: HashMap<usize, SegmentStyle>,
}

impl Road {
    pub fn num_segments(&self) -> usize {
        self.anchors.len().saturating_sub(1)
    }

    pub fn style_for_segment(&self, i: usize) -> Cow<'_, SegmentStyle> {
        match self.segment_overrides.get(&i) {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(SegmentStyle {
                lanes: self.lanes.clone(),
                center: self.center,
            }),
        }
    }

    pub fn segment_width_m(&self, i: usize) -> f32 {
        self.style_for_segment(i).total_width_m()
    }

    pub fn segment_override_mut(&mut self, i: usize) -> &mut SegmentStyle {
        if !self.segment_overrides.contains_key(&i) {
            let style = self.style_for_segment(i).into_owned();
            self.segment_overrides.insert(i, style);
        }
        self.segment_overrides.get_mut(&i).unwrap()
    }

    pub fn reset_segment(&mut self, i: usize) {
        self.segment_overrides.remove(&i);
    }

    pub fn segment_equals_default(&self, i: usize) -> bool {
        match self.segment_overrides.get(&i) {
            Some(over) => over.lanes == self.lanes && over.center == self.center,
            None => true,
        }
    }

    pub fn anchor_widths_m(&self) -> Vec<f32> {
        let n = self.anchors.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            return vec![total_width_m(&self.lanes)];
        }
        let mut out = vec![0.0f32; n];
        for k in 0..n {
            let left = if k > 0 { Some(self.segment_width_m(k - 1)) } else { None };
            let right = if k < n - 1 { Some(self.segment_width_m(k)) } else { None };
            out[k] = match (left, right) {
                (Some(l), Some(r)) => l.max(r),
                (Some(w), None) | (None, Some(w)) => w,
                (None, None) => total_width_m(&self.lanes),
            };
        }
        out
    }

    /// Computes the lane anchors at `end`.
    ///
    /// The main anchor (at the road centerline) is included so callers can
    /// draw all anchors in a uniform list; the caller usually filters it out
    /// since it's already shown as an ordinary node.
    ///
    /// `px_per_m` converts lane widths from meters into image pixels. All
    /// returned positions and tangents are in image space. Returns an empty
    /// list if the road has fewer than 2 anchors or the endpoint is
    /// degenerate (zero-length forward tangent).
    pub fn lane_anchors(
        &self,
        end: RoadEnd,
        graph: &NodeGraph,
        px_per_m: f32,
    ) -> Vec<LaneAnchor> {
        if self.anchors.len() < 2 || px_per_m <= 0.0 {
            return Vec::new();
        }

        let (endpoint_anchor, adjacent_anchor, seg_idx) = match end {
            RoadEnd::Start => {
                let a = &self.anchors[0];
                let b = &self.anchors[1];
                (a, b, 0usize)
            }
            RoadEnd::End => {
                let n = self.anchors.len();
                let a = &self.anchors[n - 1];
                let b = &self.anchors[n - 2];
                (a, b, n - 2)
            }
        };

        let Some(p_end) = graph.pos(endpoint_anchor.node) else {
            return Vec::new();
        };
        let Some(p_adj) = graph.pos(adjacent_anchor.node) else {
            return Vec::new();
        };

        // Forward tangent: the direction the road travels as it leaves
        // (at Start) or as it arrives (at End).
        let forward = match end {
            RoadEnd::Start => {
                if endpoint_anchor.out_handle.length_sq() > 1e-6 {
                    endpoint_anchor.out_handle.normalized()
                } else {
                    let d = p_adj - p_end;
                    if d.length_sq() < 1e-9 {
                        return Vec::new();
                    }
                    d.normalized()
                }
            }
            RoadEnd::End => {
                if endpoint_anchor.in_handle.length_sq() > 1e-6 {
                    (-endpoint_anchor.in_handle).normalized()
                } else {
                    let d = p_end - p_adj;
                    if d.length_sq() < 1e-9 {
                        return Vec::new();
                    }
                    d.normalized()
                }
            }
        };

        // Outgoing tangent: a new road attached here travels away from the
        // source, not back along it. At Start that's reversed; at End it's
        // the same as forward.
        let outgoing = match end {
            RoadEnd::Start => -forward,
            RoadEnd::End => forward,
        };

        let style = self.style_for_segment(seg_idx);
        let total_m = style.total_width_m();
        if total_m <= 0.0 {
            return Vec::new();
        }

        // Perpendicular in image-y-down space: rotate forward 90° clockwise.
        let perp = eframe::egui::Vec2::new(-forward.y, forward.x);

        let mut anchors = Vec::with_capacity(style.lanes.len() * 2 + 1);

        // Main anchor.
        anchors.push(LaneAnchor {
            position: p_end,
            kind: AnchorKind::Main,
            outgoing_tangent: outgoing,
        });

        // Boundaries and lane centers, walking left to right.
        let mut cursor_m = -total_m / 2.0;
        for (i, lane) in style.lanes.iter().enumerate() {
            let boundary_pos = p_end + perp * (cursor_m * px_per_m);
            anchors.push(LaneAnchor {
                position: boundary_pos,
                kind: AnchorKind::Boundary(i),
                outgoing_tangent: outgoing,
            });

            let center_m = cursor_m + lane.width_m / 2.0;
            let center_pos = p_end + perp * (center_m * px_per_m);
            anchors.push(LaneAnchor {
                position: center_pos,
                kind: AnchorKind::LaneCenter(i),
                outgoing_tangent: outgoing,
            });

            cursor_m += lane.width_m;
        }
        // Final boundary (right edge).
        let final_pos = p_end + perp * (cursor_m * px_per_m);
        anchors.push(LaneAnchor {
            position: final_pos,
            kind: AnchorKind::Boundary(style.lanes.len()),
            outgoing_tangent: outgoing,
        });

        anchors
    }

    /// The endpoint position of `end`, if the road has one.
    pub fn endpoint_position(&self, end: RoadEnd, graph: &NodeGraph) -> Option<Pos2> {
        let anchor = match end {
            RoadEnd::Start => self.anchors.first()?,
            RoadEnd::End => self.anchors.last()?,
        };
        graph.pos(anchor.node)
    }
}