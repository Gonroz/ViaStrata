//! Low-level drawing helpers (no app state).

use eframe::egui::{self, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::model::CenterStyle;
use crate::spline;

// Colour constants. Kept in one place so the whole palette can be tuned
// without hunting through the drawing code.
const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);
const PAINT_WHITE: Color32 = Color32::from_rgb(238, 238, 238);
const PAINT_YELLOW: Color32 = Color32::from_rgb(240, 200, 40);

/// Draws a road with full lane markings.
///
/// `pts` must be in *screen space* and already tessellated — the canvas runs
/// the road's control points through `spline::catmull_rom` before calling
/// this. `width` is the full road width in screen pixels; `lanes` and
/// `center` determine the marking layout.
///
/// Drawing order, bottom to top:
///   1. Asphalt body (thick stroke along the centerline, butt caps)
///   2. Solid white edge lines, inset just inside the asphalt on both sides
///   3. Dashed white lane dividers between adjacent lanes
///   4. Center line, style-dependent, at the middle of the road
///
/// The center line and a lane divider can occupy the same position on an
/// even-lane road (the middle divider *is* the center). When that happens,
/// the divider is skipped so the center line wins.
///
/// Road *ends* are butt-capped, not rounded — the polyline stops dead at the
/// last control point and the markings all terminate on the same longitudinal
/// line, which is what a real road-with-paint looks like from overhead. To
/// get that, the asphalt is drawn as a plain `Shape::line` (which is
/// butt-capped by default) with no end circles.
pub fn paint_road(
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

    // ----- 1. Asphalt ----------------------------------------------------
    //
    // `Shape::line` renders an open polyline with butt caps: the stroke is a
    // rectangle of `width` centred on each segment, and the ends stop flush
    // at the first and last point. No end circles — the road terminates with
    // a straight edge.
    p.add(Shape::line(pts.to_vec(), Stroke::new(width, ASPHALT)));

    // ----- Marking geometry ---------------------------------------------
    //
    // All marking widths scale with the road so they stay proportional at
    // any zoom, but with floors so they never vanish to sub-pixel.
    let line_w = (width * 0.02).max(1.0);
    let center_w = (width * 0.025).max(1.0);

    // ----- 2. Edge lines -------------------------------------------------
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

    // ----- 3. Lane dividers ----------------------------------------------
    //
    // With `lanes` lanes across a road of full width `width`, each lane is
    // `width / lanes` wide, and the internal boundaries sit at
    // `k * lane_w - half` for k in 1..lanes. That puts n-1 dividers evenly
    // spaced between the two edge lines.
    //
    // The center line occupies the middle boundary on even-lane roads, so we
    // skip drawing a divider there when a center line is going to be drawn.
    let has_center_line = lanes >= 2 && lanes % 2 == 0 && center != CenterStyle::None;

    if lanes >= 2 {
        let lane_w = width / lanes as f32;
        // Dashes sized relative to lane width; floors keep them legible when
        // the road is small on screen.
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

    // ----- 4. Center line ------------------------------------------------
    if has_center_line {
        draw_center(p, pts, width, center_w, center);
    }
}

/// Renders a short horizontal road inside `rect` using the given style, for
/// use as a clickable preview thumbnail in the side menu.
///
/// This is just `paint_road` on a two-point straight segment. The point of
/// the function (rather than calling `paint_road` directly from the sidebar)
/// is that the preview is always drawn at a fixed *visual* width regardless
/// of the user's current lane width and zoom — the thumbnail is about the
/// marking layout, not about any particular scale.
///
/// Two lanes are used so that all four center styles are clearly
/// distinguishable: with two lanes, `has_center_line` is true for every style
/// other than `None`, so the middle of the road is either unmarked (dashed
/// white divider) or carries the selected center style. Odd-lane previews
/// would suppress the center line entirely under the current rules and make
/// the styles indistinguishable.
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
    paint_road(p, &pts, width, 2, center);
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