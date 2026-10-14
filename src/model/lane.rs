//! Lanes: the cross-section of a road.
//!
//! A road's width used to be two scalars — a lane count and a lane width.
//! That's fine for the common "4 lanes of the same thing" case, but real
//! streets have parking, bike lanes, shoulders, medians, turn pockets, and
//! bus lanes, each with their own width and paint rules. A cross-section is
//! therefore a `Vec<Lane>`, ordered from one side of the road to the other.
//!
//! Ordering is arbitrary but fixed: `lanes[0]` is on the negative-offset
//! side of the road centerline. When a road is drawn left-to-right on
//! screen, that means `lanes[0]` appears at the top.

use super::Unit;

/// What a lane carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneKind {
    /// Ordinary through lane.
    Travel,
    /// Turn pocket. Painted like a travel lane for now; separate kind so
    /// future features (turn arrows, storage-bay tapers) can target it.
    Turn,
    /// Bus lane. Painted like a travel lane for now; separate for future
    /// queue-jump / signal-priority features.
    Bus,
    /// Parallel parking. Directionless. No edge line on the curb side.
    Parking,
    /// Bike lane. Gets a green tint.
    Bike,
    /// Shoulder. Slightly lighter grey; no travel traffic.
    Shoulder,
    /// Median. Filled with a tan tint and *replaces* the center line — two
    /// lanes separated by a median have no painted line between them, the
    /// median body does the separating.
    Median,
}

impl LaneKind {
    pub fn label(self) -> &'static str {
        match self {
            LaneKind::Travel => "Travel",
            LaneKind::Turn => "Turn",
            LaneKind::Bus => "Bus",
            LaneKind::Parking => "Parking",
            LaneKind::Bike => "Bike",
            LaneKind::Shoulder => "Shoulder",
            LaneKind::Median => "Median",
        }
    }

    /// Every kind, in the order shown in the picker. Grouped loosely:
    /// travel-like kinds first, then the special-purpose ones.
    pub fn all() -> &'static [LaneKind] {
        &[
            LaneKind::Travel,
            LaneKind::Turn,
            LaneKind::Bus,
            LaneKind::Parking,
            LaneKind::Bike,
            LaneKind::Shoulder,
            LaneKind::Median,
        ]
    }

    /// Travel-like lanes carry moving traffic. They're the ones the center
    /// line separates when their directions differ, the ones that get a
    /// direction arrow, and the ones whose direction field is editable.
    pub fn is_travel_like(self) -> bool {
        matches!(self, LaneKind::Travel | LaneKind::Turn | LaneKind::Bus)
    }

    /// Whether this kind uses `direction` at all. Parking and median lanes
    /// are directionless; the field still exists on every `Lane` (it's
    /// cheaper than an `Option`), but the UI hides the toggle for these.
    pub fn uses_direction(self) -> bool {
        !matches!(self, LaneKind::Parking | LaneKind::Median)
    }

    /// Default width in meters for a freshly-added lane of this kind.
    /// Reasonable US urban defaults; the user can tune per lane.
    pub fn default_width_m(self) -> f32 {
        match self {
            LaneKind::Travel => 3.35,
            LaneKind::Turn => 3.05,
            LaneKind::Bus => 3.5,
            LaneKind::Parking => 2.4,
            LaneKind::Bike => 1.8,
            LaneKind::Shoulder => 1.2,
            LaneKind::Median => 1.2,
        }
    }

    /// Fill tint applied over the asphalt. `None` means leave the asphalt
    /// showing through untouched — travel lanes are the default surface.
    ///
    /// Colours are deliberately muted so they read as paving materials, not
    /// as UI chrome. They also have to sit under white and yellow markings
    /// without washing them out.
    pub fn tint(self) -> Option<[u8; 3]> {
        match self {
            LaneKind::Travel | LaneKind::Turn | LaneKind::Bus => None,
            LaneKind::Parking => None,
            // Muted greens, greys, and tans. RGB values chosen by eye.
            LaneKind::Bike => Some([62, 118, 82]),
            LaneKind::Shoulder => Some([86, 86, 92]),
            LaneKind::Median => Some([122, 108, 86]),
        }
    }

    /// Whether the outer edge of a road whose endmost lane is this kind
    /// gets a white edge line. Parking and median butt directly against a
    /// curb or against the rest of the median body, so an edge line there
    /// would be wrong.
    pub fn has_edge_line(self) -> bool {
        !matches!(self, LaneKind::Parking | LaneKind::Median)
    }
}

/// Which way a lane's traffic flows, relative to the direction the road was
/// drawn (first anchor → last anchor).
///
/// Draw-order semantics rather than compass-direction semantics: if you
/// draw a road eastward, eastbound is `Forward`. Draw the same road
/// westward and the same physical lanes become `Backward`. That's how most
/// road-editing tools work, and it means flipping a road's draw direction
/// flips all its arrows at once, which is usually what you want.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Forward,
    Backward,
}

/// One lane in a road's cross-section.
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub kind: LaneKind,
    pub width_m: f32,
    pub direction: Direction,
}

impl Lane {
    pub fn new(kind: LaneKind, direction: Direction) -> Self {
        Self {
            kind,
            width_m: kind.default_width_m(),
            direction,
        }
    }

    pub fn travel_forward() -> Self {
        Self::new(LaneKind::Travel, Direction::Forward)
    }

    pub fn travel_backward() -> Self {
        Self::new(LaneKind::Travel, Direction::Backward)
    }
}

/// The cross-section every new road starts with: two travel lanes, one
/// each way, matching the old fixed default of "2 lanes, 11 ft each".
pub fn default_two_way() -> Vec<Lane> {
    vec![Lane::travel_forward(), Lane::travel_backward()]
}

/// Total cross-section width in meters.
pub fn total_width_m(lanes: &[Lane]) -> f32 {
    lanes.iter().map(|l| l.width_m).sum()
}

/// Convenience for the UI layer: width in the user's display unit.
pub fn width_in_display_unit(lane: &Lane, unit: Unit) -> f32 {
    lane.width_m / unit.to_m()
}

/// Inverse of `width_in_display_unit`: converts a value the user typed in
/// their display unit back to meters for storage.
pub fn display_unit_to_meters(display: f32, unit: Unit) -> f32 {
    display * unit.to_m()
}