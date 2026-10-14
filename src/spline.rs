//! Curve math and tessellation helpers.
//!
//! Everything on-screen is a cubic Bézier path. The Catmull-Rom behavior
//! that used to live here for roads is now derived from Bézier form: the
//! auto-handle computation in `model::path::recompute_auto_handles`
//! converts a Catmull-Rom tangent into the equivalent Bézier control
//! points, so a smooth road still has the same shape but goes through the
//! same rendering code as pavements.
//!
//! Also here: `offset_polyline`, used by lane-marking rendering, and
//! `ear_clip`, used by pavement fill.

use eframe::egui::{Pos2, Vec2};

/// Evaluates a cubic Bézier at parameter `t` in `[0, 1]`.
///
/// `c1` and `c2` are the two control points; `p0` and `p3` are the endpoints.
/// The curve passes through `p0` at `t=0` and `p3` at `t=1`, and its tangent
/// at `p0` points toward `c1` (scaled by 3×, which is the standard Bézier
/// derivative).
pub fn cubic_bezier_point(p0: Pos2, c1: Pos2, c2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;

    let a = mt2 * mt; // (1-t)^3
    let b = 3.0 * mt2 * t;
    let c = 3.0 * mt * t2;
    let d = t2 * t;

    Pos2::new(
        a * p0.x + b * c1.x + c * c2.x + d * p3.x,
        a * p0.y + b * c1.y + c * c2.y + d * p3.y,
    )
}

/// Offsets a polyline laterally by `offset` units (positive = to the left
/// when walking along the polyline in drawing order, in a y-down coordinate
/// system).
///
/// Used to draw lane markings on a polyline parallel to the road centerline.
///
/// The offset is *naive* — each vertex moves along its own normal, so sharp
/// corners self-intersect. That is fine here because the input is always a
/// densely tessellated spline: consecutive vertices are close enough that
/// the naive offset is visually indistinguishable from a proper miter.
pub fn offset_polyline(pts: &[Pos2], offset: f32) -> Vec<Pos2> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
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
            out.push(pts[i]);
            continue;
        }

        let t = tangent / len;
        let normal = Vec2::new(-t.y, t.x);
        out.push(pts[i] + normal * offset);
    }

    out
}

/// Triangulates a simple polygon into a list of triangle indices.
///
/// Uses the classic ear-clipping algorithm: repeatedly find a convex vertex
/// ("ear") whose triangle contains no other polygon vertex, cut it off, and
/// recurse on the remainder. Works on any simple polygon — convex or
/// concave — but not self-intersecting.
///
/// Returns indices into `points`. If the polygon is degenerate (fewer than
/// three points, all collinear, or self-intersecting), returns an empty or
/// partial list rather than looping forever.
///
/// # Caller contract
///
/// The input must not repeat its first point at the end. A duplicated
/// vertex creates a zero-length edge, which the "is this an ear?" test
/// treats as having zero area and rejects, causing the algorithm to bail
/// partway through. `paint_pavement` trims the duplicate before calling.
pub fn ear_clip(points: &[Pos2]) -> Vec<[u32; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }

    let mut indices: Vec<usize> = (0..n).collect();

    let signed_area: f32 = (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            points[i].x * points[j].y - points[j].x * points[i].y
        })
        .sum();
    let ccw = signed_area > 0.0;

    let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(n.saturating_sub(2));

    // Bound iterations: each successful clip removes one vertex, so a
    // simple polygon needs exactly n-2 clips. If we exceed n * n attempts,
    // something is wrong (self-intersecting input) and we bail.
    let mut safety = n * n;

    while indices.len() > 3 && safety > 0 {
        safety -= 1;
        let m = indices.len();
        let mut clipped = false;

        for i in 0..m {
            let prev = indices[(i + m - 1) % m];
            let cur = indices[i];
            let next = indices[(i + 1) % m];

            let a = points[prev];
            let b = points[cur];
            let c = points[next];

            // Convex corner in the polygon's winding direction?
            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            let convex = if ccw { cross > 0.0 } else { cross < 0.0 };
            if !convex {
                continue;
            }

            // No other vertex may lie inside the ear triangle.
            let mut contains_other = false;
            for (k, &other) in indices.iter().enumerate() {
                if k == i || other == prev || other == cur || other == next {
                    continue;
                }
                if point_in_triangle(points[other], a, b, c) {
                    contains_other = true;
                    break;
                }
            }
            if contains_other {
                continue;
            }

            triangles.push([prev as u32, cur as u32, next as u32]);
            indices.remove(i);
            clipped = true;
            break;
        }

        if !clipped {
            // No ear found. Degenerate or self-intersecting; stop rather
            // than loop forever.
            break;
        }
    }

    if indices.len() == 3 {
        triangles.push([indices[0] as u32, indices[1] as u32, indices[2] as u32]);
    }

    triangles
}

/// Tests whether `p` lies inside triangle `(a, b, c)`.
///
/// Uses the sign of the cross product on each edge. Works for either
/// winding because we accept a point as inside if all three edge tests
/// agree, regardless of whether they are all positive or all negative.
fn point_in_triangle(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);

    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;

    !(has_neg && has_pos)
}