//! Low-level drawing helpers (no app state).

use eframe::egui::{self, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::model::CenterStyle;
use crate::spline;

const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);
const PAINT_WHITE: Color32 = Color32::from_rgb(238, 238, 238);
const PAINT_YELLOW: Color32 = Color32::from_rgb(240, 200, 40);

/// Fills a closed pavement polygon with asphalt.
///
/// # Input expectations
///
/// `polygon` should be a closed loop in screen space. It may or may not
/// repeat the first point at the end — this function trims a duplicated
/// closing point if present. It should have at least three distinct points.
///
/// # Why not `Shape::convex_polygon`
///
/// `Shape::convex_polygon` fills a closed path via a fan from vertex 0,
/// which is only correct for convex shapes. Our polygons can be concave
/// (L-shaped footprints, cut corners), so we ear-clip into triangles
/// ourselves and hand the triangles to egui as an explicit `Mesh`. A
/// `Mesh` carries its own triangle indices, which removes any ambiguity
/// about winding or fill rules.
///
/// If ear-clipping can't fully triangulate the polygon — which happens on
/// self-intersecting input — the fill is skipped and only the outline is
/// drawn. A partial fill on ambiguous geometry is worse than no fill.
pub fn paint_pavement(painter: &Painter, polygon: &[Pos2]) {
    // Trim a repeated closing point. `tessellate` includes the closing
    // endpoint, which equals the first vertex, so the raw polyline has a
    // duplicate. Ear-clipping struggles with duplicated vertices: the
    // shoelace area is unaffected, but the "is this an ear?" test can get
    // confused by the zero-length edge, and the algorithm bails partway.
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
        // Build one Mesh carrying every triangle. `Mesh` is explicit about
        // its vertices and indices, so there is no risk of egui
        // interpreting a 3-point closed path differently from the triangle
        // we intended.
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

    // Outline: gives the fill a clean edge and hides hairline seams between
    // adjacent triangles. Drawn in the same colour as the fill, so it
    // doesn't read as a separate stroke.
    let mut outline: Vec<Pos2> = poly.to_vec();
    outline.push(poly[0]);
    painter.add(Shape::line(outline, Stroke::new(1.0_f32, ASPHALT)));
}

/// Asphalt body plus joint fills. No markings.
pub fn paint_road_asphalt(painter: &Painter, pts: &[Pos2], width: f32, joints: &[Pos2]) {
    if pts.len() < 2 || width <= 0.0 {
        return;
    }
    painter.add(Shape::line(pts.to_vec(), Stroke::new(width, ASPHALT)));
    for &j in joints {
        painter.circle_filled(j, width / 2.0, ASPHALT);
    }
}

/// Lane markings: edge lines, lane dividers, and the center line.
pub fn paint_road_markings(
    painter: &Painter,
    pts: &[Pos2],
    width: f32,
    lanes: u32,
    center: CenterStyle,
) {
    if pts.len() < 2 || width <= 0.0 {
        return;
    }

    let half = width / 2.0;
    let line_w = (width * 0.02).max(1.0);
    let center_w = (width * 0.025).max(1.0);

    let edge_offset = half - line_w;
    if edge_offset > 0.5 {
        for side in [-1.0_f32, 1.0] {
            let line = spline::offset_polyline(pts, side * edge_offset);
            painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
        }
    }

    let has_center_line = lanes >= 2 && lanes % 2 == 0 && center != CenterStyle::None;

    if lanes >= 2 {
        let lane_w = width / lanes as f32;
        let dash = (lane_w * 0.5).max(6.0);
        let gap = (lane_w * 0.4).max(4.0);

        for k in 1..lanes {
            let off = k as f32 * lane_w - half;
            if has_center_line && off.abs() < 1.0 {
                continue;
            }
            let line = spline::offset_polyline(pts, off);
            painter.extend(Shape::dashed_line(
                &line,
                Stroke::new(line_w, PAINT_WHITE),
                dash,
                gap,
            ));
        }
    }

    if has_center_line {
        draw_center(painter, pts, width, center_w, center);
    }
}

/// Preview thumbnail for the side menu pickers.
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
        draw_center(painter, &pts, width, center_w, center);
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

fn draw_center(
    painter: &Painter,
    pts: &[Pos2],
    road_width: f32,
    line_width: f32,
    style: CenterStyle,
) {
    let stroke = Stroke::new(line_width, PAINT_YELLOW);
    match style {
        CenterStyle::None => {}
        CenterStyle::YellowSolid => {
            let line = spline::offset_polyline(pts, 0.0);
            painter.add(Shape::line(line, stroke));
        }
        CenterStyle::YellowDouble => {
            let gap = (road_width * 0.015).max(line_width + 1.0);
            for side in [-1.0_f32, 1.0] {
                let line = spline::offset_polyline(pts, side * gap);
                painter.add(Shape::line(line, stroke));
            }
        }
        CenterStyle::YellowDashed => {
            let dash = (road_width * 0.4).max(10.0);
            let gap = (road_width * 0.3).max(8.0);
            let line = spline::offset_polyline(pts, 0.0);
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