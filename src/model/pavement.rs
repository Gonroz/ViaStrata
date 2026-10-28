//! Pavement shapes: filled material footprints.
//!
//! A pavement is a closed path drawn as a filled shape rather than as a
//! stroked centerline. In Sublayer mode this represents a base material
//! (asphalt, concrete, sidewalk, grass). In Road mode it was previously
//! available as a filled shape too, but that's now Sublayer-only — the
//! road itself draws its own asphalt.
//!
//! Pavements and roads share the same anchor representation
//! (`PathAnchor`), so a pavement corner can be snapped to a road endpoint
//! and the two stay glued together when that shared node is dragged.

use super::material::Material;
use super::path::PathAnchor;

/// A closed, filled shape drawn with a base material.
pub struct Pavement {
    pub anchors: Vec<PathAnchor>,
    pub material: Material,
}