//! The active editing tool.

/// Which editing tool is currently active.
///
/// Tools are mutually exclusive: only one owns the primary mouse button at a
/// time, and the toolbar's bottom row changes to match.
#[derive(PartialEq, Clone, Copy)]
pub enum Tool {
    /// Drag to pan. No point placement.
    Pan,
    /// Click two points, then enter a real distance to derive pixels-per-meter.
    Calibrate,
    /// Click to append control points; Enter/double-click/right-click commits.
    Road,
}