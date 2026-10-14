//! Pavements: filled asphalt footprints.
//!
//! A pavement is a closed path drawn as a filled shape rather than as a
//! stroked centerline. That makes it the right tool for anything whose
//! outline isn't expressible as "road of constant width" — a curved curb,
//! a corner apron, a median with its own shape, a widened intersection.
//!
//! Pavements and roads share the same anchor representation
//! (`PathAnchor`), so a pavement corner can be snapped to a road endpoint
//! and the two stay glued together when that shared node is dragged.

use super::path::PathAnchor;

/// A closed, filled asphalt shape.
pub struct Pavement {
    pub anchors: Vec<PathAnchor>,
}