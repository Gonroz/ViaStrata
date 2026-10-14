//! Catmull-Rom spline tessellation for smooth road rendering.
//!
//! Roads are stored as *sparse* control points — the exact pixels the user
//! clicked. To render them we need a smooth curve that still passes through
//! every one of those points. Catmull-Rom is ideal here because it is
//! *interpolating* (unlike Bézier, where the middle control points sit off
//! the curve), so the user's clicks remain the literal truth of the road.
//!
//! For a segment P1→P2 with neighbours P0 (before) and P3 (after), uniform
//! Catmull-Rom evaluates to:
//!
//!     C(t) = 0.5 * (
//!           2*P1
//!         + (-P0 + P2)                    * t
//!         + (2*P0 - 5*P1 + 4*P2 - P3)     * t^2
//!         + (-P0 + 3*P1 - 3*P2 + P3)      * t^3
//!     )
//!
//! The curve is C1-continuous, which is why adjoining segments meet without
//! a visible corner even if the user draws a sharp turn.
//!
//! Endpoints are handled by *clamping* the out-of-range neighbours to the
//! nearest real point. That is the standard trick for making the curve start
//! and end with a sensible tangent without asking the user for handle points.

use eframe::egui::{Pos2, Vec2};

/// Straight-line samples per control-point span. Higher = smoother geometry
/// but more vertices pushed to the painter every frame. 12 is a good balance
/// for road widths that rarely exceed a few hundred screen pixels: the corner
/// cut at a typical join is well under a pixel.
const SAMPLES_PER_SEGMENT: usize = 12;

/// Tessellates a Catmull-Rom spline through `pts` into a dense polyline.
///
/// * 0 or 1 point  -> returned unchanged (nothing to interpolate).
/// * 2 points      -> returned unchanged; two control points cannot define a
///                    curve, so the caller gets a straight segment.
/// * 3+ points     -> `(pts.len() - 1) * SAMPLES_PER_SEGMENT + 1` points,
///                    including both original endpoints.
///
/// The output is in the same coordinate space as the input (image pixels or
/// screen pixels — the caller decides), so this function has no opinion about
/// transforms.
pub fn catmull_rom(pts: &[Pos2]) -> Vec<Pos2> {
    // Below three points there is no curve to draw.
    if pts.len() < 3 {
        return pts.to_vec();
    }

    let n = pts.len() as isize;
    let mut out = Vec::with_capacity((pts.len() - 1) * SAMPLES_PER_SEGMENT + 1);

    // Index helper that clamps to the valid range. This gives the first and
    // last segments a "phantom" neighbour equal to the endpoint, which is
    // what makes the tangents behave correctly at the ends of the road.
    let get = |i: isize| -> Pos2 { pts[i.clamp(0, n - 1) as usize] };

    // Emit the first point once, then sample each segment without re-emitting
    // its start (that's the previous segment's t=1).
    out.push(pts[0]);

    for i in 0..pts.len() - 1 {
        let p0 = get(i as isize - 1);
        let p1 = get(i as isize);
        let p2 = get(i as isize + 1);
        let p3 = get(i as isize + 2);

        for s in 1..=SAMPLES_PER_SEGMENT {
            let t = s as f32 / SAMPLES_PER_SEGMENT as f32;
            out.push(catmull_rom_point(p0, p1, p2, p3, t));
        }
    }

    out
}

/// Evaluates the uniform Catmull-Rom basis at `t` in `[0, 1]`. The curve
/// passes through `p1` at `t = 0` and `p2` at `t = 1`.
///
/// The four basis polynomials below come from expanding the matrix form of
/// the Catmull-Rom basis given in the module docs. They are applied
/// component-wise to x and y.
fn catmull_rom_point(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let t2 = t * t;
    let t3 = t2 * t;

    let b0 = -0.5 * t3 + t2 - 0.5 * t; // weight of P0
    let b1 = 1.5 * t3 - 2.5 * t2 + 1.0; // weight of P1
    let b2 = -1.5 * t3 + 2.0 * t2 + 0.5 * t; // weight of P2
    let b3 = 0.5 * t3 - 0.5 * t2; // weight of P3

    Pos2::new(
        b0 * p0.x + b1 * p1.x + b2 * p2.x + b3 * p3.x,
        b0 * p0.y + b1 * p1.y + b2 * p2.y + b3 * p3.y,
    )
}

/// Offsets a polyline laterally by `offset` units (positive = to the left
/// when walking along the polyline in drawing order, in a y-down coordinate
/// system).
///
/// Used to draw lane markings: the asphalt is a thick stroke on the
/// centerline, and each lane divider or edge line is a thin stroke on a
/// parallel polyline offset by a fixed distance.
///
/// The offset is *naive* — it moves each vertex along its own normal, which
/// means sharp corners self-intersect. That's fine here because the input is
/// always a densely tessellated spline, where consecutive vertices are close
/// enough that the naive offset is visually indistinguishable from a proper
/// miter join.
///
/// Vertices with no well-defined tangent (isolated points, or duplicated
/// positions) are passed through unmodified rather than being moved to NaN.
pub fn offset_polyline(pts: &[Pos2], offset: f32) -> Vec<Pos2> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
        // Tangent direction: forward difference at the start, backward
        // difference at the end, and the average of the two neighbouring
        // unit vectors elsewhere (which gives a smoother direction around
        // bends than either single segment would).
        let tangent: Vec2 = if i == 0 {
            pts[1] - pts[0]
        } else if i == n - 1 {
            pts[n - 1] - pts[n - 2]
        } else {
            let a = pts[i] - pts[i - 1];
            let b = pts[i + 1] - pts[i];
            let al = a.length();
            let bl = b.length();
            if al > 1e-6 && bl > 1e-6 {
                a / al + b / bl
            } else if al > 1e-6 {
                a
            } else {
                b
            }
        };

        let len = tangent.length();
        if len < 1e-6 {
            // Degenerate tangent; leave the vertex where it is rather than
            // producing a NaN by dividing by zero.
            out.push(pts[i]);
            continue;
        }

        let t = tangent / len;
        // Rotate 90° to get the normal. In egui's y-down coordinate system
        // this points to the right of travel, which is why the caller's
        // positive offset ends up on the "left" side when read top-to-bottom.
        let normal = Vec2::new(-t.y, t.x);
        out.push(pts[i] + normal * offset);
    }

    out
}