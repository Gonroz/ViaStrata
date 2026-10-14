//! Low-level drawing helpers (no app state).

use eframe::egui::{self, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::model::CenterStyle;
use crate::spline;

// Colour constants. Kept in one place so the whole palette can be tuned
// without hunting through the drawing code.
const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);
const PAINT_WHITE: Color32 = Color32::from_rgb(238, 238, 238);
const PAINT_YELLOW: Color32 = Color32::from_rgb(240, 200, 40);

/// Draws a complete road: asphalt (with joint fills), then markings.
///
/// `pts` must be in *screen space* and already tessellated. `joint_positions`
/// are extra screen-space points where a filled asphalt circle should be
/// drawn at the road's half-width — used to bridge the corner gaps at
/// vertices shared with another road. The canvas computes these each frame
/// by looking for coincident vertices across all roads.
///
/// The two-phase split (asphalt first, markings second) matters when
/// rendering multiple roads: `paint_road_asphalt` and `paint_road_markings`
/// are exposed separately so the canvas can do all asphalt+joints in one
/// pass and all markings in a second pass. Without that, a road drawn later
/// would have its asphalt painted over an earlier road's markings.
pub fn paint_road(
    p: &Painter,
    pts: &[Pos2],
    width: f32,
    lanes: u32,
    center: CenterStyle,
    joint_positions: &[Pos2],
) {
    paint_road_asphalt(p, pts, width, joint_positions);
    paint_road_markings(p, pts, width, lanes, center);
}

/// Asphalt body plus joint fills. No markings.
///
/// The asphalt is a butt-capped polyline (the default for `Shape::line`), so
/// loose road ends terminate in a straight edge. At a shared vertex,
/// `joint_positions` adds a filled circle the size of the road's half-width,
/// which fills the wedge-shaped gap between the two roads' straight edges.
///
/// Joint circles are only drawn where a vertex is actually shared. A road
/// that ends in open space gets no circle — that's the user-visible
/// difference between "connected" and "loose end".
pub fn paint_road_asphalt(p: &Painter, pts: &[Pos2], width: f32, joints: &[Pos2]) {
    if pts.len() < 2 || width <= 0.0 {
        return;
    }
    p.add(Shape::line(pts.to_vec(), Stroke::new(width, ASPHALT)));
    for &j in joints {
        p.circle_filled(j, width / 2.0, ASPHALT);
    }
}

/// Lane markings: edge lines, lane dividers, and the center line.
///
/// Assumes the asphalt has already been drawn. `width` is the full road
/// width in screen pixels; markings are sized proportionally with floors so
/// they never vanish to sub-pixel at high zoom.
///
/// The center line and a lane divider can occupy the same position on an
/// even-lane road (the middle divider *is* the center). When that happens,
/// the divider is skipped so the center line wins.
pub fn paint_road_markings(
    p: &Painter,
    pts: &[Pos2],
    width: f32,
    lanes: u32,
    center: CenterStyle,
) {
    if pts.len() < 2 || width <= 0.0 {
        return;
    }

    let half = width / 2.0;

    // All marking widths scale with the road so they stay proportional at
    // any zoom, but with floors so they never vanish to sub-pixel.
    let line_w = (width * 0.02).max(1.0);
    let center_w = (width * 0.025).max(1.0);

    // ----- Edge lines ----------------------------------------------------
    //
    // Placed just inside the asphalt edge. `edge_offset` is the distance
    // from the centerline to the *center* of the edge line stroke, which is
    // what `offset_polyline` takes.
    //
    // Because `offset_polyline` offsets each vertex perpendicular to its
    // tangent, the edge lines share the same longitudinal extent as the
    // centerline. Their endpoints line up with the asphalt's endpoints,
    // giving a clean straight edge to the whole road.
    let edge_offset = half - line_w;
    if edge_offset > 0.5 {
        for side in [-1.0_f32, 1.0] {
            let line = spline::offset_polyline(pts, side * edge_offset);
            p.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
        }
    }

    // ----- Lane dividers -------------------------------------------------
    //
    // With `lanes` lanes across a road of full width `width`, each lane is
    // `width / lanes` wide, and the internal boundaries sit at
    // `k * lane_w - half` for k in 1..lanes.
    let has_center_line = lanes >= 2 && lanes % 2 == 0 && center != CenterStyle::None;

    if lanes >= 2 {
        let lane_w = width / lanes as f32;
        let dash = (lane_w * 0.5).max(6.0);
        let gap = (lane_w * 0.4).max(4.0);

        for k in 1..lanes {
            let off = k as f32 * lane_w - half;

            // Skip the middle divider if a center line occupies it.
            if has_center_line && off.abs() < 1.0 {
                continue;
            }

            let line = spline::offset_polyline(pts, off);
            p.extend(Shape::dashed_line(
                &line,
                Stroke::new(line_w, PAINT_WHITE),
                dash,
                gap,
            ));
        }
    }

    // ----- Center line ---------------------------------------------------
    if has_center_line {
        draw_center(p, pts, width, center_w, center);
    }
}

/// Renders a short horizontal road inside `rect` using the given style, for
/// use as a clickable preview thumbnail in the side menu.
///
/// Two lanes are used so that all four center styles are clearly
/// distinguishable — with two lanes, `has_center_line` is true for every
/// style other than `None`. Odd-lane previews would suppress the center line
/// entirely and make the styles indistinguishable.
pub fn paint_road_preview(p: &Painter, rect: Rect, center: CenterStyle) {
    if rect.width() < 12.0 || rect.height() < 8.0 {
        return;
    }
    let y = rect.center().y;
    let pts = vec![
        Pos2::new(rect.left() + 2.0, y),
        Pos2::new(rect.right() - 2.0, y),
    ];
    let width = (rect.height() * 0.7).max(8.0);
    // No joints in the preview: a two-point straight segment has nothing to
    // bridge, and the thumbnail is about marking layout only.
    paint_road(p, &pts, width, 2, center, &[]);
}

/// Paints the center line at offset 0 in the selected style. Called only
/// when the caller has already established that a center line is wanted.
fn draw_center(
    p: &Painter,
    pts: &[Pos2],
    road_width: f32,
    line_width: f32,
    style: CenterStyle,
) {
    let stroke = Stroke::new(line_width, PAINT_YELLOW);

    match style {
        CenterStyle::None => {
            // Caller should not have called us, but be defensive.
        }
        CenterStyle::YellowSolid => {
            let line = spline::offset_polyline(pts, 0.0);
            p.add(Shape::line(line, stroke));
        }
        CenterStyle::YellowDouble => {
            // Two parallel solid yellows, separated by a gap proportional to
            // road width. `gap` here is the offset from the centerline to
            // each line's own center, so the visual gap between them is
            // `2 * gap - line_width`, which is what you want to keep roughly
            // constant regardless of stroke weight.
            let gap = (road_width * 0.015).max(line_width + 1.0);
            for side in [-1.0_f32, 1.0] {
                let line = spline::offset_polyline(pts, side * gap);
                p.add(Shape::line(line, stroke));
            }
        }
        CenterStyle::YellowDashed => {
            let dash = (road_width * 0.4).max(10.0);
            let gap = (road_width * 0.3).max(8.0);
            let line = spline::offset_polyline(pts, 0.0);
            p.extend(Shape::dashed_line(&line, stroke, dash, gap));
        }
    }
}

/// Imagery credit in the bottom-right corner of the canvas.
///
/// This is required by both tile providers when the imagery is displayed.
/// Drawn twice — black offset shadow underneath, white text on top — so it
/// stays readable over both light and dark satellite imagery.
pub fn paint_attribution(painter: &Painter, rect: Rect, text: &str) {
    let pos = rect.right_bottom() - vec2(8.0, 6.0);
    let font = egui::FontId::proportional(12.0);
    painter.text(
        pos + vec2(1.0, 1.0),
        egui::Align2::RIGHT_BOTTOM,
        text,
        font.clone(),
        Color32::BLACK,
    );
    painter.text(pos, egui::Align2::RIGHT_BOTTOM, text, font, Color32::WHITE);
}