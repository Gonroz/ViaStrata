//! Roads, their cross-sections, and per-segment overrides.
//!
//! A road's default cross-section (lanes + center style) applies to every
//! segment it's made of. Individual segments can override that — a
//! segment might have a bike lane, a turn pocket, or an extra lane that
//! its neighbours don't. Overrides are keyed by segment index, which runs
//! `0..anchors.len()-1`.

use std::borrow::Cow;
use std::collections::HashMap;

use super::lane::{total_width_m, Lane};
use super::path::PathAnchor;

/// How the center of a two-way road is marked.
///
/// Applied at every boundary in a segment's cross-section where adjacent
/// travel lanes face opposite directions. For the common "one row forward,
/// one row backward" layout, that's exactly one boundary.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CenterStyle {
    /// No center marking.
    None,
    /// Single solid yellow — passing not permitted in either direction.
    YellowSolid,
    /// Two parallel solid yellow lines.
    YellowDouble,
    /// Dashed yellow — passing permitted where sight distance allows.
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
///
/// This is the unit of style that a road carries by default and that an
/// individual segment can override.
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
///
/// Width is derived from lane widths scaled by `px_per_m × zoom`. Segments
/// whose neighbouring segments have different widths taper between them —
/// see `anchor_widths_m` for the exact rule.
pub struct Road {
    pub anchors: Vec<PathAnchor>,
    /// The default cross-section. Applied to every segment that doesn't
    /// have an override.
    pub lanes: Vec<Lane>,
    pub center: CenterStyle,
    /// Per-segment overrides, keyed by segment index.
    pub segment_overrides: HashMap<usize, SegmentStyle>,
}

impl Road {
    /// Number of segments (spans between adjacent anchors).
    pub fn num_segments(&self) -> usize {
        self.anchors.len().saturating_sub(1)
    }

    /// The effective cross-section for segment `i`. Borrows the override
    /// when one exists, otherwise synthesizes the default. `Cow` avoids
    /// cloning the default on every access.
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

    /// Ensures segment `i` has an override and returns it mutably. On first
    /// call the override is initialized from the current effective style,
    /// so nothing visibly changes until the caller writes to it.
    pub fn segment_override_mut(&mut self, i: usize) -> &mut SegmentStyle {
        if !self.segment_overrides.contains_key(&i) {
            let style = self.style_for_segment(i).into_owned();
            self.segment_overrides.insert(i, style);
        }
        self.segment_overrides.get_mut(&i).unwrap()
    }

    /// Removes any override for segment `i`, so it falls back to the road
    /// default.
    pub fn reset_segment(&mut self, i: usize) {
        self.segment_overrides.remove(&i);
    }

    /// Whether segment `i`'s override (if any) is currently identical to
    /// the road default. Used to clean up no-op overrides on deselect.
    pub fn segment_equals_default(&self, i: usize) -> bool {
        match self.segment_overrides.get(&i) {
            Some(over) => over.lanes == self.lanes && over.center == self.center,
            None => true,
        }
    }

    /// Width at each anchor, in meters.
    ///
    /// * For the first and last anchors: the single adjacent segment's width.
    /// * For interior anchors: the **max** of the two adjacent segments'
    ///   widths.
    ///
    /// The max rule means a width transition happens in the *narrower* of
    /// the two segments, which matches how real roads behave — a road that
    /// gains a lane widens before the new paint appears, then stays wide.
    /// The alternative (using the narrower width) would compress the wider
    /// segment, which reads as a bug.
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
}