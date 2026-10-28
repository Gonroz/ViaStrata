//! Low-level drawing helpers (no app state).

use eframe::egui::{self, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::model::{CenterStyle, Direction, Lane, LaneKind, SegmentStyle};
use crate::spline;

const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);
const PAINT_WHITE: Color32 = Color32::from_rgb(238, 238, 238);
const PAINT_YELLOW: Color32 = Color32::from_rgb(240, 200, 40);
const SELECTION_BLUE: Color32 = Color32::from_rgba_premultiplied(80, 160, 255, 90);

#[derive(Clone, Copy)]
enum BoundaryMark {
    None,
    CenterLine(CenterStyle),
    DashedWhite,
    SolidWhite,
}

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
        }
        return BoundaryMark::DashedWhite;
    }
    BoundaryMark::SolidWhite
}

/// Fills a closed polygon with `color` using ear-clip triangulation.
///
/// `polygon` may or may not repeat its first point at the end; this
/// function trims a duplicate. Skips the fill entirely if ear-clipping
/// can't fully triangulate the shape.
pub fn paint_pavement(painter: &Painter, polygon: &[Pos2], color: Color32) {
    let poly: &[Pos2] = if polygon.len() >= 2 && polygon[0] == polygon[polygon.len() - 1] {
        &polygon[..polygon.len() - 1]
    } else {
        polygon
    };
    if poly.len() < 3 {
        return;
    }
    let tris = spline::ear_clip(poly);
    if tris.len() == poly.len() - 2 {
        let mut mesh = egui::Mesh::default();
        mesh.vertices.reserve(tris.len() * 3);
        for [a, b, c] in &tris {
            let base = mesh.vertices.len() as u32;
            for &idx in &[*a, *b, *c] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: poly[idx as usize],
                    uv: egui::epaint::WHITE_UV,
                    color,
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
    painter.add(Shape::line(outline, Stroke::new(1.0_f32, color)));
}

/// Draws a road segment body as a triangle-strip mesh, using per-sample
/// widths.
pub fn paint_road_body(painter: &Painter, pts: &[Pos2], widths: &[f32]) {
    if pts.len() < 2 || widths.len() != pts.len() {
        return;
    }

    let mut mesh = egui::Mesh::default();
    let n = pts.len();

    for i in 0..n - 1 {
        let tangent_i = tangent(pts, i);
        let tangent_next = tangent(pts, i + 1);
        let len_i = tangent_i.length();
        let len_next = tangent_next.length();
        if len_i < 1e-6 || len_next < 1e-6 {
            continue;
        }
        let n_i = egui::vec2(-tangent_i.y, tangent_i.x) / len_i;
        let n_next = egui::vec2(-tangent_next.y, tangent_next.x) / len_next;

        let h_i = widths[i] / 2.0;
        let h_next = widths[i + 1] / 2.0;

        let left_i = pts[i] + n_i * h_i;
        let right_i = pts[i] - n_i * h_i;
        let left_next = pts[i + 1] + n_next * h_next;
        let right_next = pts[i + 1] - n_next * h_next;

        let base = mesh.vertices.len() as u32;
        for p in [left_i, right_i, left_next, right_next] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: egui::epaint::WHITE_UV,
                color: ASPHALT,
            });
        }
        mesh.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
        ]);
    }

    painter.add(Shape::Mesh(mesh));
}

pub fn paint_lane_tints_tapered(
    painter: &Painter,
    pts: &[Pos2],
    widths: &[f32],
    style: &SegmentStyle,
) {
    if pts.len() < 2 || widths.len() != pts.len() || style.lanes.is_empty() {
        return;
    }
    let native = style.total_width_m();
    if native <= 0.0 {
        return;
    }

    let mut fractions = Vec::with_capacity(style.lanes.len() + 1);
    let mut acc = 0.0;
    fractions.push(0.0);
    for lane in &style.lanes {
        acc += lane.width_m;
        fractions.push(acc / native);
    }

    for (i, lane) in style.lanes.iter().enumerate() {
        let Some([r, g, b]) = lane.kind.tint() else {
            continue;
        };
        let color = Color32::from_rgb(r, g, b);
        let f_left = fractions[i];
        let f_right = fractions[i + 1];
        let mut left_offsets = Vec::with_capacity(pts.len());
        let mut right_offsets = Vec::with_capacity(pts.len());
        for w in widths {
            left_offsets.push((f_left - 0.5) * w);
            right_offsets.push((f_right - 0.5) * w);
        }
        let left_line = spline::offset_polyline_variable(pts, &left_offsets);
        let right_line = spline::offset_polyline_variable(pts, &right_offsets);
        paint_strip(painter, &left_line, &right_line, color);
    }
}

fn paint_strip(painter: &Painter, left: &[Pos2], right: &[Pos2], color: Color32) {
    if left.len() < 2 || left.len() != right.len() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for i in 0..left.len() - 1 {
        let base = mesh.vertices.len() as u32;
        for p in [left[i], right[i], left[i + 1], right[i + 1]] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
        ]);
    }
    painter.add(Shape::Mesh(mesh));
}

pub fn paint_road_markings_tapered(
    painter: &Painter,
    pts: &[Pos2],
    widths: &[f32],
    style: &SegmentStyle,
) {
    if pts.len() < 2 || widths.len() != pts.len() || style.lanes.is_empty() {
        return;
    }
    let native = style.total_width_m();
    if native <= 0.0 {
        return;
    }

    let avg_w = widths.iter().copied().sum::<f32>() / widths.len() as f32;
    let line_w = (avg_w * 0.02).max(1.0);
    let center_w = (avg_w * 0.025).max(1.0);

    let mut fractions = Vec::with_capacity(style.lanes.len() + 1);
    let mut acc = 0.0;
    fractions.push(0.0);
    for lane in &style.lanes {
        acc += lane.width_m;
        fractions.push(acc / native);
    }

    let offsets_for = |f: f32| -> Vec<f32> {
        widths.iter().map(|w| (f - 0.5) * w).collect()
    };

    let left_ok = style.lanes.first().map(|l| l.kind.has_edge_line()).unwrap_or(false);
    let right_ok = style.lanes.last().map(|l| l.kind.has_edge_line()).unwrap_or(false);
    if left_ok {
        let offs = offsets_for(0.0);
        let line = spline::offset_polyline_variable(pts, &offs);
        painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
    }
    if right_ok {
        let offs = offsets_for(1.0);
        let line = spline::offset_polyline_variable(pts, &offs);
        painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
    }

    for i in 0..style.lanes.len().saturating_sub(1) {
        let f = fractions[i + 1];
        let mark = classify_boundary(&style.lanes[i], &style.lanes[i + 1], style.center);
        let offs = offsets_for(f);
        let line = spline::offset_polyline_variable(pts, &offs);
        match mark {
            BoundaryMark::None => {}
            BoundaryMark::DashedWhite => {
                let dash = (avg_w * 0.1).max(6.0);
                let gap = (avg_w * 0.08).max(4.0);
                painter.extend(Shape::dashed_line(
                    &line,
                    Stroke::new(line_w, PAINT_WHITE),
                    dash,
                    gap,
                ));
            }
            BoundaryMark::SolidWhite => {
                painter.add(Shape::line(line, Stroke::new(line_w, PAINT_WHITE)));
            }
            BoundaryMark::CenterLine(style_c) => {
                draw_center_at(painter, pts, widths, f, center_w, style_c);
            }
        }
    }
}

fn draw_center_at(
    painter: &Painter,
    pts: &[Pos2],
    widths: &[f32],
    f: f32,
    line_width: f32,
    style: CenterStyle,
) {
    let stroke = Stroke::new(line_width, PAINT_YELLOW);
    let avg_w = widths.iter().copied().sum::<f32>() / widths.len() as f32;
    let offset_at = |extra: f32| -> Vec<f32> {
        widths.iter().map(|w| (f - 0.5) * w + extra).collect()
    };

    match style {
        CenterStyle::None => {}
        CenterStyle::YellowSolid => {
            let offs = offset_at(0.0);
            let line = spline::offset_polyline_variable(pts, &offs);
            painter.add(Shape::line(line, stroke));
        }
        CenterStyle::YellowDouble => {
            let gap = (avg_w * 0.015).max(line_width + 1.0);
            for side in [-1.0_f32, 1.0] {
                let offs = offset_at(side * gap);
                let line = spline::offset_polyline_variable(pts, &offs);
                painter.add(Shape::line(line, stroke));
            }
        }
        CenterStyle::YellowDashed => {
            let dash = (avg_w * 0.4).max(10.0);
            let gap = (avg_w * 0.3).max(8.0);
            let offs = offset_at(0.0);
            let line = spline::offset_polyline_variable(pts, &offs);
            painter.extend(Shape::dashed_line(&line, stroke, dash, gap));
        }
    }
}

pub fn paint_segment_highlight(painter: &Painter, pts: &[Pos2], widths: &[f32]) {
    if pts.len() < 2 || widths.len() != pts.len() {
        return;
    }
    paint_road_body_with_color(painter, pts, widths, SELECTION_BLUE);
}

fn paint_road_body_with_color(
    painter: &Painter,
    pts: &[Pos2],
    widths: &[f32],
    color: Color32,
) {
    if pts.len() < 2 || widths.len() != pts.len() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for i in 0..pts.len() - 1 {
        let ti = tangent(pts, i);
        let tn = tangent(pts, i + 1);
        let li = ti.length();
        let ln = tn.length();
        if li < 1e-6 || ln < 1e-6 {
            continue;
        }
        let ni = egui::vec2(-ti.y, ti.x) / li;
        let nn = egui::vec2(-tn.y, tn.x) / ln;
        let hi = widths[i] / 2.0;
        let hn = widths[i + 1] / 2.0;

        let base = mesh.vertices.len() as u32;
        for p in [
            pts[i] + ni * hi,
            pts[i] - ni * hi,
            pts[i + 1] + nn * hn,
            pts[i + 1] - nn * hn,
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
        ]);
    }
    painter.add(Shape::Mesh(mesh));
}

fn tangent(pts: &[Pos2], i: usize) -> eframe::egui::Vec2 {
    let n = pts.len();
    if n < 2 {
        return eframe::egui::Vec2::ZERO;
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
        draw_center_at(painter, &pts, &[width, width], 0.5, center_w, center);
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

    painter.add(Shape::line(pts.clone(), Stroke::new(width, ASPHALT)));
    let style = SegmentStyle {
        lanes: lanes.to_vec(),
        center,
    };
    let widths = vec![width, width];
    paint_lane_tints_tapered(painter, &pts, &widths, &style);
    paint_road_markings_tapered(painter, &pts, &widths, &style);

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
            let arrow = match lane.direction {
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