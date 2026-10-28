//! Drawing the scene with the current camera.

use std::collections::HashSet;

use eframe::egui::{self, pos2, Color32, Pos2, Rect, Stroke};

use super::input::NODE_HIT_RADIUS_PX;
use super::Xform;
use crate::app::App;
use crate::model::{
    tessellate, AnchorRef, EditMode, HandleRef, HandleSide, NodeId, PathAnchor, PathOwner, Tool,
};
use crate::spline;
use crate::ui::paint::{
    paint_attribution, paint_lane_tints_tapered, paint_pavement, paint_road_body,
    paint_road_markings_tapered, paint_segment_highlight,
};

const SAMPLES_PER_SEGMENT: usize = 12;
const HANDLE_RADIUS: f32 = 4.0;
const SHARED_RING_RADIUS: f32 = HANDLE_RADIUS + 3.5;
const DIR_HANDLE_RADIUS: f32 = 3.5;

const ASPHALT: Color32 = Color32::from_rgb(58, 58, 62);

pub fn draw(
    app: &App,
    painter: &egui::Painter,
    resp: &egui::Response,
    x: &Xform,
    rect: Rect,
) {
    if let Some(tex) = &app.view.tex {
        let r = Rect::from_min_size(x.to_screen(Pos2::ZERO), app.view.img_size * x.zoom());
        painter.image(
            tex.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let ppm = app.px_per_m.unwrap_or(1.0);
    let shared = app.scene.shared_nodes();

    // --- Pavements (under everything) ---------------------------------
    for pa in &app.scene.pavements {
        let polyline_img = tessellate(&pa.anchors, &app.scene.graph, true, SAMPLES_PER_SEGMENT);
        let polyline: Vec<Pos2> = polyline_img.iter().map(|&p| x.to_screen(p)).collect();
        paint_pavement(painter, &polyline, pa.material.color());
    }

    // --- Road bodies (pass 1) -----------------------------------------
    for road in &app.scene.roads {
        paint_road_whole_body(app, painter, x, road, ppm, &shared);
    }

    // --- Tints + markings (pass 2) ------------------------------------
    for road in &app.scene.roads {
        paint_road_whole_markings(app, painter, x, road, ppm);
    }

    // --- Selection highlight ------------------------------------------
    if let Some(seg) = app.scene.selected_segment {
        if let Some(road) = app.scene.roads.get(seg.road) {
            if seg.segment < road.num_segments() {
                let (pts_screen, widths_screen) =
                    segment_screen_samples(app, x, road, seg.segment, ppm);
                paint_segment_highlight(painter, &pts_screen, &widths_screen);
            }
        }
    }

    // --- Draft pavement ------------------------------------------------
    //
    // Only drawn in Sublayer mode, since the Pavement tool isn't available
    // anywhere else. The draft is filled with the current material color
    // (at reduced alpha) so the user can see what they're about to commit.
    if app.mode == EditMode::Sublayer && !app.scene.current_pavement.is_empty() {
        let anchors = app.scene.current_pavement.clone();
        let polyline_img = tessellate(&anchors, &app.scene.graph, false, SAMPLES_PER_SEGMENT);
        let mut screen: Vec<Pos2> = polyline_img.iter().map(|&p| x.to_screen(p)).collect();
        if let Some(h) = resp.hover_pos() {
            let img = x.to_image(h);
            let preview = app
                .scene
                .find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX))
                .and_then(|id| app.scene.graph.pos(id))
                .unwrap_or(img);
            screen.push(x.to_screen(preview));
            if let Some(first) = app
                .scene
                .current_pavement
                .first()
                .and_then(|a| app.scene.graph.pos(a.node))
            {
                screen.push(x.to_screen(first));
            }
        }
        // Fill at reduced opacity so the user sees the material color
        // without it feeling committed.
        if screen.len() >= 3 {
            paint_pavement(painter, &screen, app.scene.draft_material.preview_color());
        }
        // Blue outline on top so the in-progress state is obvious.
        painter.add(egui::Shape::line(
            screen,
            Stroke::new(2.0_f32, Color32::from_rgb(120, 170, 230)),
        ));
    }

    // --- Draft road ----------------------------------------------------
    if app.mode == EditMode::Road && !app.scene.current_road.is_empty() {
        let anchors = app.scene.current_road.clone();
        let polyline_img = tessellate(&anchors, &app.scene.graph, false, SAMPLES_PER_SEGMENT);
        let mut screen: Vec<Pos2> = polyline_img.iter().map(|&p| x.to_screen(p)).collect();
        if let Some(h) = resp.hover_pos() {
            let img = x.to_image(h);
            let preview = app
                .scene
                .find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX))
                .and_then(|id| app.scene.graph.pos(id))
                .unwrap_or(img);
            screen.push(x.to_screen(preview));
        }
        let w = crate::model::total_width_m(&app.scene.draft_lanes) * ppm * x.zoom();
        let widths = vec![w; screen.len()];
        let style = crate::model::SegmentStyle {
            lanes: app.scene.draft_lanes.clone(),
            center: app.scene.center_style,
        };
        paint_road_body(painter, &screen, &widths);
        paint_lane_tints_tapered(painter, &screen, &widths, &style);
        paint_road_markings_tapered(painter, &screen, &widths, &style);
    }

    // --- Node handles + direction handles -----------------------------
    if matches!(app.tool, Tool::Road | Tool::Pavement) {
        paint_handles(app, painter, x, &shared);
    }

    // --- Calibration --------------------------------------------------
    if !app.calib_pts.is_empty() {
        let red = Color32::from_rgb(235, 60, 60);
        let pts: Vec<Pos2> = app.calib_pts.iter().map(|&p| x.to_screen(p)).collect();
        if pts.len() == 2 {
            painter.line_segment([pts[0], pts[1]], Stroke::new(2.0_f32, red));
        }
        for q in pts {
            painter.circle_stroke(q, 6.0, Stroke::new(2.0_f32, red));
        }
    }

    if !app.attribution.is_empty() {
        paint_attribution(painter, rect, &app.attribution);
    }
}

pub fn segment_screen_samples(
    app: &App,
    x: &Xform,
    road: &crate::model::Road,
    seg_i: usize,
    ppm: f32,
) -> (Vec<Pos2>, Vec<f32>) {
    let anchor_widths = road.anchor_widths_m();
    let Some(a) = road.anchors.get(seg_i) else {
        return (Vec::new(), Vec::new());
    };
    let Some(b) = road.anchors.get(seg_i + 1) else {
        return (Vec::new(), Vec::new());
    };
    let Some(p_a) = app.scene.graph.pos(a.node) else {
        return (Vec::new(), Vec::new());
    };
    let Some(p_b) = app.scene.graph.pos(b.node) else {
        return (Vec::new(), Vec::new());
    };

    let c1 = p_a + a.out_handle;
    let c2 = p_b + b.in_handle;
    let samples_img = spline::tessellate_bezier(p_a, c1, c2, p_b, SAMPLES_PER_SEGMENT);
    let samples_screen: Vec<Pos2> = samples_img.iter().map(|&p| x.to_screen(p)).collect();

    let w_start_m = anchor_widths.get(seg_i).copied().unwrap_or(0.0);
    let w_end_m = anchor_widths.get(seg_i + 1).copied().unwrap_or(0.0);
    let n = samples_screen.len();
    let denom = n.saturating_sub(1).max(1) as f32;
    let widths_screen: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / denom;
            let w_m = w_start_m + (w_end_m - w_start_m) * t;
            w_m * ppm * x.zoom()
        })
        .collect();

    (samples_screen, widths_screen)
}

fn paint_road_whole_body(
    app: &App,
    painter: &egui::Painter,
    x: &Xform,
    road: &crate::model::Road,
    ppm: f32,
    shared: &HashSet<NodeId>,
) {
    let anchor_widths = road.anchor_widths_m();

    for seg_i in 0..road.num_segments() {
        let (pts_screen, widths_screen) = segment_screen_samples(app, x, road, seg_i, ppm);
        if pts_screen.len() < 2 {
            continue;
        }
        paint_road_body(painter, &pts_screen, &widths_screen);
    }

    let last = road.anchors.len().saturating_sub(1);
    for (k, anchor) in road.anchors.iter().enumerate() {
        let is_endpoint = k == 0 || k == last;
        if is_endpoint && !shared.contains(&anchor.node) {
            continue;
        }
        let Some(p_img) = app.scene.graph.pos(anchor.node) else {
            continue;
        };
        let w_m = anchor_widths.get(k).copied().unwrap_or(0.0);
        let w_px = w_m * ppm * x.zoom();
        if w_px <= 0.0 {
            continue;
        }
        painter.circle_filled(x.to_screen(p_img), w_px / 2.0, ASPHALT);
    }
}

fn paint_road_whole_markings(
    app: &App,
    painter: &egui::Painter,
    x: &Xform,
    road: &crate::model::Road,
    ppm: f32,
) {
    for seg_i in 0..road.num_segments() {
        let style = road.style_for_segment(seg_i);
        let (pts_screen, widths_screen) = segment_screen_samples(app, x, road, seg_i, ppm);
        if pts_screen.len() < 2 {
            continue;
        }
        paint_lane_tints_tapered(painter, &pts_screen, &widths_screen, &style);
        paint_road_markings_tapered(painter, &pts_screen, &widths_screen, &style);
    }
}

fn paint_handles(app: &App, painter: &egui::Painter, x: &Xform, shared: &HashSet<NodeId>) {
    let base = Color32::from_rgb(220, 220, 220);
    let hover = Color32::from_rgb(255, 210, 80);
    let outline = Color32::from_black_alpha(160);
    let handle_line = Color32::from_rgb(140, 190, 255);
    let handle_dot = Color32::from_rgb(180, 220, 255);

    let active_anchor = app
        .scene
        .dragging_handle
        .map(|h| h.anchor_ref())
        .or_else(|| app.scene.hover_handle.map(|h| h.anchor_ref()));

    let hover_anchor_owner: Option<(PathOwner, usize)> = app
        .scene
        .hover_node
        .and_then(|id| anchor_owner_for_node(app, id))
        .or_else(|| active_anchor.map(|r| (r.owner, r.index)));

    if let Some((owner, idx)) = hover_anchor_owner {
        let href = AnchorRef { owner, index: idx };
        if let Some(anchor) = app.scene.anchor(href) {
            if let Some(pos) = app.scene.anchor_position(href) {
                let s = x.to_screen(pos);
                draw_direction_handles(
                    painter,
                    x,
                    anchor,
                    s,
                    handle_line,
                    handle_dot,
                    app.scene.hover_handle,
                    owner,
                    idx,
                );
            }
        }
    }

    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut paint_one = |id: NodeId| {
        if !seen.insert(id) {
            return;
        }
        let Some(p) = app.scene.graph.pos(id) else { return };
        let fill = if app.scene.hover_node == Some(id) { hover } else { base };
        let s = x.to_screen(p);

        if shared.contains(&id) {
            painter.circle_stroke(s, SHARED_RING_RADIUS, Stroke::new(2.0_f32, fill));
        }
        painter.circle_filled(s, HANDLE_RADIUS, fill);
        painter.circle_stroke(s, HANDLE_RADIUS, Stroke::new(1.0_f32, outline));
    };

    for a in &app.scene.current_road {
        paint_one(a.node);
    }
    for a in &app.scene.current_pavement {
        paint_one(a.node);
    }
    for road in &app.scene.roads {
        for a in &road.anchors {
            paint_one(a.node);
        }
    }
    for pa in &app.scene.pavements {
        for a in &pa.anchors {
            paint_one(a.node);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_direction_handles(
    painter: &egui::Painter,
    x: &Xform,
    anchor: &PathAnchor,
    anchor_screen: Pos2,
    line_color: Color32,
    dot_color: Color32,
    hover_handle: Option<HandleRef>,
    owner: PathOwner,
    anchor_index: usize,
) {
    let draw_one = |offset: eframe::egui::Vec2, side: HandleSide| {
        if offset.length_sq() < 1e-6 {
            return;
        }
        let end = anchor_screen + offset * x.zoom();
        painter.line_segment([anchor_screen, end], Stroke::new(1.0_f32, line_color));

        let is_hovered = hover_handle
            .map(|h| h.owner == owner && h.anchor_index == anchor_index && h.side == side)
            .unwrap_or(false);
        let r = if is_hovered {
            DIR_HANDLE_RADIUS + 1.0
        } else {
            DIR_HANDLE_RADIUS
        };
        painter.circle_filled(end, r, dot_color);
        painter.circle_stroke(
            end,
            r,
            Stroke::new(1.0_f32, Color32::from_black_alpha(160)),
        );
    };

    draw_one(anchor.in_handle, HandleSide::In);
    draw_one(anchor.out_handle, HandleSide::Out);
}

fn anchor_owner_for_node(app: &App, id: NodeId) -> Option<(PathOwner, usize)> {
    for (i, road) in app.scene.roads.iter().enumerate() {
        for (ai, a) in road.anchors.iter().enumerate() {
            if a.node == id {
                return Some((PathOwner::Road(i), ai));
            }
        }
    }
    for (i, pa) in app.scene.pavements.iter().enumerate() {
        for (ai, a) in pa.anchors.iter().enumerate() {
            if a.node == id {
                return Some((PathOwner::Pavement(i), ai));
            }
        }
    }
    None
}