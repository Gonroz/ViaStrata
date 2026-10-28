//! Curve math and tessellation helpers.
//!
//! Everything on-screen is a cubic Bézier path. The Catmull-Rom behavior
//! that used to live here for roads is now derived from Bézier form: the
//! auto-handle computation in `model::path::recompute_auto_handles`
//! converts a Catmull-Rom tangent into the equivalent Bézier control
//! points.
//!
//! Also here: `offset_polyline` (constant-offset, used by lane markings),
//! `offset_polyline_variable` (variable-offset, used by tapered roads),
//! and `ear_clip` (used by pavement fill).

use eframe::egui::{Pos2, Vec2};

/// Evaluates a cubic Bézier at parameter `t` in `[0, 1]`.
pub fn cubic_bezier_point(p0: Pos2, c1: Pos2, c2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;

    let a = mt2 * mt;
    let b = 3.0 * mt2 * t;
    let c = 3.0 * mt * t2;
    let d = t2 * t;

    Pos2::new(
        a * p0.x + b * c1.x + c * c2.x + d * p3.x,
        a * p0.y + b * c1.y + c * c2.y + d * p3.y,
    )
}

/// Tessellates a single Bézier segment into a polyline with `samples + 1`
/// points, including both endpoints.
///
/// Separate from the anchor-based `tessellate` because tapered rendering
/// needs per-segment polylines and per-segment widths, not one merged
/// polyline with a single width.
pub fn tessellate_bezier(
    p0: Pos2,
    c1: Pos2,
    c2: Pos2,
    p3: Pos2,
    samples: usize,
) -> Vec<Pos2> {
    let samples = samples.max(1);
    let mut out = Vec::with_capacity(samples + 1);
    out.push(p0);
    for s in 1..=samples {
        let t = s as f32 / samples as f32;
        out.push(cubic_bezier_point(p0, c1, c2, p3, t));
    }
    out
}

/// Computes the per-vertex tangent of a polyline.
///
/// Forward difference at the start, backward difference at the end,
/// average of the two neighbouring unit vectors elsewhere. Returns a
/// zero-length vector at any vertex whose neighbours coincide.
fn tangent_at(pts: &[Pos2], i: usize) -> Vec2 {
    let n = pts.len();
    if n < 2 {
        return Vec2::ZERO;
    }
    if i == 0 {
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
    }
}

/// Offsets a polyline laterally by a constant `offset`.
pub fn offset_polyline(pts: &[Pos2], offset: f32) -> Vec<Pos2> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len());
    for i in 0..pts.len() {
        let tangent = tangent_at(pts, i);
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

/// Offsets a polyline laterally by a per-sample distance.
///
/// `offsets[i]` is the lateral offset at `pts[i]`. Used by tapered roads,
/// where the offset of a lane boundary grows or shrinks along the segment.
/// `offsets` must be the same length as `pts`.
pub fn offset_polyline_variable(pts: &[Pos2], offsets: &[f32]) -> Vec<Pos2> {
    debug_assert_eq!(pts.len(), offsets.len());
    if pts.len() < 2 || offsets.len() != pts.len() {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len());
    for i in 0..pts.len() {
        let tangent = tangent_at(pts, i);
        let len = tangent.length();
        if len < 1e-6 {
            out.push(pts[i]);
            continue;
        }
        let t = tangent / len;
        let normal = Vec2::new(-t.y, t.x);
        out.push(pts[i] + normal * offsets[i]);
    }
    out
}

/// Triangulates a simple polygon into a list of triangle indices.
///
/// Uses ear clipping. Works on simple polygons — convex or concave, but
/// not self-intersecting. `points` must not repeat its first point at the
/// end; `paint_pavement` trims the duplicate before calling.
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

            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            let convex = if ccw { cross > 0.0 } else { cross < 0.0 };
            if !convex {
                continue;
            }

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
            break;
        }
    }

    if indices.len() == 3 {
        triangles.push([indices[0] as u32, indices[1] as u32, indices[2] as u32]);
    }

    triangles
}

fn point_in_triangle(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);

    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;

    !(has_neg && has_pos)
}