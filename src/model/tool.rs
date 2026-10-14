//! The active editing tool.

/// Which editing tool is currently active.
///
/// Tools are mutually exclusive: only one owns the primary mouse button at
/// a time, and the toolbar's bottom row changes to match.
#[derive(PartialEq, Clone, Copy)]
pub enum Tool {
    /// Drag to pan. No point placement.
    Pan,
    /// Click two points, then enter a real distance to derive pixels-per-meter.
    Calibrate,
    /// Draw a road: a stroked centerline with lane markings.
    Road,
    /// Draw a pavement: a closed, filled asphalt footprint.
    Pavement,
}