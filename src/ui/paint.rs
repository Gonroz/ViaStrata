//! Low-level drawing helpers (no app state).

use eframe::egui::{self, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::model::{CenterStyle, Direction, Lane, LaneKind};
use crate::spline;

const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);
const PAINT_WHITE: Color32 = Color32::from_rgb(238, 238, 238);
const PAINT_YELLOW: Color32 = Color32::from_rgb(240, 200, 40);

/// How a boundary between two adjacent lanes is marked.
#[derive(Clone, Copy)]
enum BoundaryMark {
    /// Nothing painted at this boundary.
    None,
    /// The yellow center line, styled by the road's `CenterStyle`.
    CenterLine(CenterStyle),
    /// Short white dashes — between same-direction travel lanes.
    DashedWhite,
    /// A continuous white line.
    SolidWhite,
}

/// Decides what separates two adjacent lanes.
///
/// The rules, in order:
///
/// 1. **Median on either side** → nothing painted. The median body itself
///    is the separator.
/// 2. **Two travel-like lanes facing opposite directions** → center line,
///    with whatever style the road carries. This is where the concept of
///    "the center line" now lives — it's not a special case, it's just one
///    possible boundary marking.
/// 3. **Two travel-like lanes facing the same direction** → dashed white.
/// 4. **Anything else** (parking, bike, shoulder against any neighbour) →
///    solid white.
fn classify_boundary(a: &Lane, b: &Lane, center: CenterStyle) -> BoundaryMark {
    if a.kind == LaneKind::Median || b.kind == LaneKind::Median {
        return BoundaryMark::None;
    }

    if a.kind.is_travel_like() && b.kind.is_travel_like() {
        if a.direction != b.direction {
            return if center == CenterStyle::None {
                BoundaryMark::None
            } else {
                BoundaryMark::CenterLine(center)
            };
        } else {
            return BoundaryMark::DashedWhite;
        }
    }

    BoundaryMark::SolidWhite
}

/// Fills a closed pavement polygon with asphalt.
///
/// Same contract as before: `polygon` may or may not repeat its first point
/// at the end; this function trims a duplicate. Skips the fill entirely if
/// ear-clipping can't fully triangulate the shape.
pub fn paint_pavement(painter: &Painter, polygon: &[Pos2]) {
    let poly: &[Pos2] = if polygon.len() >= 2 && polygon[0] == polygon[polygon.len() - 1] {
        &polygon[..polygon.len() - 1]
    } else {
        polygon
    };

    if poly.len() < 3 {
        return;
    }

    let tris = spline::ear_clip(poly);
    let expected = poly.len() - 2;

    if tris.len() == expected {
        let mut mesh = egui::Mesh::default();
        mesh.vertices.reserve(tris.len() * 3);
        for [a, b, c] in &tris {
            let base = mesh.vertices.len() as u32;
            for &idx in &[*a, *b, *c] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: poly[idx as usize],
                    uv: egui::epaint::WHITE_UV,
                    color: ASPHALT,
                });
            }
            mesh.indices.push(base);
            mesh.indices.push(base + 1);
            mesh.indices.push(base + 2);
        }
        painter.add(Shape::Mesh(mesh));
    }

    let mut outline: Vec<Pos2> = poly.to_vec();
    outline.push(poly[0]);
    painter.add(Shape::line(outline, Stroke::new(1.0_f32, ASPHALT)));
}

/// Asphalt body plus joint fills. No markings, no lane tints.
pub fn paint_road_asphalt(painter: &Painter, pts: &[Pos2], width: f32, joints: &[Pos2]) {
    if pts.len() < 2 || width <= 0.0 {
        return;
    }
    painter.add(Shape::line(pts.to_vec(), Stroke::new(width, ASPHALT)));
    for &j in joints {
        painter.circle_filled(j, width / 2.0, ASPHALT);
    }
}

/// Paints coloured fills for lanes whose kind defines a tint.
///
/// Travel-like lanes and parking have no tint (the asphalt shows through),
/// so this only draws bike lanes, shoulders, and medians. Drawn *between*
/// asphalt and markings, so it never covers a line.
pub fn paint_lane_tints(painter: &Painter, pts: &[Pos2], width: f32, lanes: &[Lane]) {
    if pts.len() < 2 || width <= 0.0 || lanes.is_empty() {
        return;
    }
    let total_m = crate::model::total_width_m(lanes);
    if total_m <= 0.0 {
        return;
    }

    let px_per_m = width / total_m;
    let half = width / 2.0;
    let mut cursor = -half;

    for lane in lanes {
        let lane_w = lane.width_m * px_per_m;
        let center_offset = cursor + lane_w / 2.0;
        cursor += lane_w;

        if let Some([r, g, b]) = lane.kind.tint() {
            let color = Color32::from_rgb(r, g, b);
            let line = spline::offset_polyline(pts, center_offset);
            painter.add(Shape::line(line, Stroke::new(lane_w, color)));
        }
    }
}

/// Edge lines, lane dividers, and center line for a road.
///
/// `width` is the road's full cross-section width in screen pixels; `lanes`
/// gives the internal layout. Marking positions are computed by walking the
/// lane list and accumulating widths, so they follow whatever the user has
/// configured rather than assuming a uniform grid.
pub fn paint_road_markings(
    painter: &Painter,
    pts: &[Pos2],
    width: f32,
    lanes: &[Lane],
    center: CenterStyle,
) {
    if pts.len() < 2 || width <= 0.0 || lanes.is_empty() {
        return;
    }
    let total_m = crate::model::total_width_m(lanes);
    if total_m <= 0.0 {
        return;
    }

    let half = width / 2.0;
    let px_per_m = width / total_m;
    let line_w = (width * 0.02).max(1.0);
    let center_w = (width * 0.025).max(1.0);

    // ----- Edge lines --------------------------------------------------
    //
    // Drawn on each side of the road unless the endmost lane is curb-
    // adjacent (parking or median), in which case there is no paint
    // between the lane and whatever abuts it.
    let edge_offset = half - line_w;
    if edge_offset > 0.5 {
        let draw_left = lanes
            .first()
            .map(|l| l.kind.has_edge_line())
            .unwrap_or(false);
        let draw_right = lanes
            .last()
            .map(|l| l.kind.has_edge_line())
            .unwrap_or(false);

        if draw_left {
            let line = spline::offset_polyline(pts, -edge_offset);
            painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
        }
        if draw_right {
            let line = spline::offset_polyline(pts, edge_offset);
            painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
        }
    }

    // ----- Internal boundaries ----------------------------------------
    //
    // Walk from the negative-offset edge toward the positive-offset edge.
    // At each internal boundary, classify and draw.
    let mut cursor = -half;
    for i in 0..lanes.len().saturating_sub(1) {
        cursor += lanes[i].width_m * px_per_m;
        let mark = classify_boundary(&lanes[i], &lanes[i + 1], center);
        let lane_px = lanes[i].width_m * px_per_m;

        match mark {
            BoundaryMark::None => {}
            BoundaryMark::DashedWhite => {
                let line = spline::offset_polyline(pts, cursor);
                let dash = (lane_px * 0.5).max(6.0);
                let gap = (lane_px * 0.4).max(4.0);
                painter.extend(Shape::dashed_line(
                    &line,
                    Stroke::new(line_w, PAINT_WHITE),
                    dash,
                    gap,
                ));
            }
            BoundaryMark::SolidWhite => {
                let line = spline::offset_polyline(pts, cursor);
                painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
            }
            BoundaryMark::CenterLine(style) => {
                draw_center_at(painter, pts, cursor, width, center_w, style);
            }
        }
    }
}

/// Preview thumbnail for the center-line style pickers. A two-lane straight
/// road with the given center style applied. Direction arrows omitted — the
/// thumbnail is about the marking, not the cross-section.
pub fn paint_road_preview(painter: &Painter, rect: Rect, center: CenterStyle) {
    if rect.width() < 12.0 || rect.height() < 8.0 {
        return;
    }
    let y = rect.center().y;
    let pts = vec![
        Pos2::new(rect.left() + 2.0, y),
        Pos2::new(rect.right() - 2.0, y),
    ];
    let width = (rect.height() * 0.7).max(8.0);
    painter.add(Shape::line(pts.clone(), Stroke::new(width, ASPHALT)));

    let half = width / 2.0;
    let line_w = (width * 0.02).max(1.0);
    let center_w = (width * 0.025).max(1.0);
    let edge_offset = half - line_w;
    if edge_offset > 0.5 {
        for side in [-1.0_f32, 1.0] {
            let line = spline::offset_polyline(&pts, side * edge_offset);
            painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
        }
    }
    if center != CenterStyle::None {
        draw_center_at(painter, &pts, 0.0, width, center_w, center);
    } else {
        let line = spline::offset_polyline(&pts, 0.0);
        painter.extend(Shape::dashed_line(
            &line,
            Stroke::new(line_w, PAINT_WHITE),
            (width * 0.3).max(6.0),
            (width * 0.25).max(4.0),
        ));
    }
}

/// Cross-section preview used in the sidebar. Draws the given lanes as a
/// horizontal strip, including tints, markings, and direction arrows, so
/// the user sees exactly what the next road will look like.
pub fn paint_cross_section(
    painter: &Painter,
    rect: Rect,
    lanes: &[Lane],
    center: CenterStyle,
) {
    if lanes.is_empty() || rect.width() < 20.0 || rect.height() < 20.0 {
        return;
    }
    let y = rect.center().y;
    let pts = vec![Pos2::new(rect.left() + 2.0, y), Pos2::new(rect.right() - 2.0, y)];
    let width = (rect.height() * 0.7).max(20.0);
    let half = width / 2.0;

    // Asphalt body.
    painter.add(Shape::line(pts.clone(), Stroke::new(width, ASPHALT)));

    // Coloured lane fills.
    paint_lane_tints(painter, &pts, width, lanes);

    // Markings.
    paint_road_markings(painter, &pts, width, lanes, center);

    // Direction arrows on travel-like lanes.
    let total_m = crate::model::total_width_m(lanes);
    if total_m <= 0.0 {
        return;
    }
    let px_per_m = width / total_m;
    let mut cursor = -half;
    for lane in lanes {
        let lane_w = lane.width_m * px_per_m;
        let cx = rect.center().x + cursor + lane_w / 2.0;
        cursor += lane_w;
        if lane.kind.is_travel_like() {
            let arrow: &str = match lane.direction {
                Direction::Forward => "→",
                Direction::Backward => "←",
            };
            painter.text(
                Pos2::new(cx, y),
                egui::Align2::CENTER_CENTER,
                arrow,
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
    }
}

/// Draws a center line at a given lateral offset (screen pixels from the
/// road centerline). Used by both the road renderer and the cross-section
/// preview, since the offset there is nonzero.
fn draw_center_at(
    painter: &Painter,
    pts: &[Pos2],
    offset: f32,
    road_width: f32,
    line_width: f32,
    style: CenterStyle,
) {
    let stroke = Stroke::new(line_width, PAINT_YELLOW);
    match style {
        CenterStyle::None => {}
        CenterStyle::YellowSolid => {
            let line = spline::offset_polyline(pts, offset);
            painter.add(Shape::line(line, stroke));
        }
        CenterStyle::YellowDouble => {
            let gap = (road_width * 0.015).max(line_width + 1.0);
            for side in [-1.0_f32, 1.0] {
                let line = spline::offset_polyline(pts, offset + side * gap);
                painter.add(Shape::line(line, stroke));
            }
        }
        CenterStyle::YellowDashed => {
            let dash = (road_width * 0.4).max(10.0);
            let gap = (road_width * 0.3).max(8.0);
            let line = spline::offset_polyline(pts, offset);
            painter.extend(Shape::dashed_line(&line, stroke, dash, gap));
        }
    }
}

/// Imagery credit in the bottom-right corner of the canvas.
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