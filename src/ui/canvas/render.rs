//! Drawing the scene with the current camera.

use std::collections::HashSet;

use eframe::egui::{self, pos2, Color32, Pos2, Rect, Stroke, Vec2};

use super::input::NODE_HIT_RADIUS_PX;
use super::Xform;
use crate::app::App;
use crate::model::{
    recompute_auto_handles, tessellate, AnchorKind, AnchorRef, EditMode, HandleRef, HandleSide,
    NodeGraph, NodeId, PathAnchor, PathOwner, Tool,
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

    // Pavements render regardless of mode — the design is always visible.
    for pa in &app.scene.pavements {
        let polyline_img = tessellate(&pa.anchors, &app.scene.graph, true, SAMPLES_PER_SEGMENT);
        let polyline: Vec<Pos2> = polyline_img.iter().map(|&p| x.to_screen(p)).collect();
        paint_pavement(painter, &polyline, pa.material.color());
    }

    for road in &app.scene.roads {
        paint_road_whole_body(app, painter, x, road, ppm, &shared);
    }

    for road in &app.scene.roads {
        paint_road_whole_markings(app, painter, x, road, ppm);
    }

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
    if app.mode == EditMode::Sublayer && !app.scene.current_pavement.is_empty() {
        let anchors = app.scene.current_pavement.clone();
        let polyline_img = tessellate(&anchors, &app.scene.graph, false, SAMPLES_PER_SEGMENT);
        let mut screen: Vec<Pos2> = polyline_img.iter().map(|&p| x.to_screen(p)).collect();
        if let Some(h) = resp.hover_pos() {
            let img = x.to_image(h);
            let preview = app
                .scene
                .find_node(
                    app.mode,
                    img,
                    x.screen_dist_to_image(NODE_HIT_RADIUS_PX),
                )
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
        if screen.len() >= 3 {
            paint_pavement(painter, &screen, app.scene.draft_material.preview_color());
        }
        painter.add(egui::Shape::line(
            screen,
            Stroke::new(2.0_f32, Color32::from_rgb(120, 170, 230)),
        ));
    }

    // --- Draft road ----------------------------------------------------
    if app.mode == EditMode::Road && !app.scene.current_road.is_empty() {
        let hover_img = resp.hover_pos().map(|h| {
            let img = x.to_image(h);
            app.scene
                .find_node(
                    app.mode,
                    img,
                    x.screen_dist_to_image(NODE_HIT_RADIUS_PX),
                )
                .and_then(|id| app.scene.graph.pos(id))
                .unwrap_or(img)
        });
        let screen_polyline =
            draft_preview_polyline(app, x, hover_img, ppm);
        if screen_polyline.len() >= 2 {
            let w = crate::model::total_width_m(&app.scene.draft_lanes) * ppm * x.zoom();
            let widths = vec![w; screen_polyline.len()];
            let style = crate::model::SegmentStyle {
                lanes: app.scene.draft_lanes.clone(),
                center: app.scene.center_style,
            };
            paint_road_body(painter, &screen_polyline, &widths);
            paint_lane_tints_tapered(painter, &screen_polyline, &widths, &style);
            paint_road_markings_tapered(painter, &screen_polyline, &widths, &style);
        }
    }

    // --- Lane anchors --------------------------------------------------
    //
    // Shown whenever the Road tool is active with an empty draft. These
    // are the snap targets for starting a new road on a lane.
    if app.mode == EditMode::Road
        && app.tool == Tool::Road
        && app.scene.current_road.is_empty()
    {
        paint_lane_anchor_dots(app, painter, resp, x, rect, ppm);
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

/// Builds the preview polyline for the current road draft, including an
/// optional hover point as a virtual last anchor.
///
/// Uses a temporary graph so the normal `recompute_auto_handles` and
/// `tessellate` pipelines can be reused without special-casing. This means
/// the preview reflects the effect of any `preferred_tangent` on the first
/// anchor — the curve visibly inherits the source road's direction.
fn draft_preview_polyline(
    app: &App,
    x: &Xform,
    hover_img: Option<Pos2>,
    _ppm: f32,
) -> Vec<Pos2> {
    // Snapshot the draft anchors and their positions.
    let mut anchors = app.scene.current_road.clone();
    let mut positions: Vec<Pos2> = anchors
        .iter()
        .filter_map(|a| app.scene.graph.pos(a.node))
        .collect();

    // If a hover point is present, treat it as a virtual last anchor.
    if let Some(h) = hover_img {
        if !anchors.is_empty() {
            positions.push(h);
            anchors.push(PathAnchor::new(NodeId(u32::MAX)));
        }
    }

    if anchors.len() < 2 {
        return Vec::new();
    }

    // Build a temporary graph from the positions we have, then run the
    // normal auto-handle computation and tessellation on it. Cheap, and
    // keeps the preview logic faithful to the real pipeline.
    let mut temp_graph = NodeGraph::new();
    let mut temp_anchors: Vec<PathAnchor> = anchors;
    for (i, a) in temp_anchors.iter_mut().enumerate() {
        a.node = temp_graph.add(positions[i]);
    }

    recompute_auto_handles(&mut temp_anchors, &temp_graph, false);
    let img_polyline = tessellate(&temp_anchors, &temp_graph, false, SAMPLES_PER_SEGMENT);
    img_polyline.iter().map(|&p| x.to_screen(p)).collect()
}

/// Draws lane anchors at every road endpoint when the user is starting a
/// new road. The nearest one to the cursor is highlighted.
fn paint_lane_anchor_dots(
    app: &App,
    painter: &egui::Painter,
    resp: &egui::Response,
    x: &Xform,
    rect: Rect,
    px_per_m: f32,
) {
    if px_per_m <= 0.0 {
        return;
    }

    // Precompute which anchor is closest to the cursor within snap radius,
    // so we can highlight it.
    let cursor_img = resp.hover_pos().map(|h| x.to_image(h));
    let snap_r = x.screen_dist_to_image(NODE_HIT_RADIUS_PX);
    let hovered = cursor_img
        .and_then(|c| app.scene.nearest_lane_anchor(app.mode, c, snap_r, px_per_m));

    for road in &app.scene.roads {
        for end in [crate::model::RoadEnd::Start, crate::model::RoadEnd::End] {
            for anchor in road.lane_anchors(end, &app.scene.graph, px_per_m) {
                if matches!(anchor.kind, AnchorKind::Main) {
                    continue;
                }
                let screen_pos = x.to_screen(anchor.position);
                if !rect.contains(screen_pos) {
                    continue;
                }

                let is_hovered = hovered
                    .map(|h| {
                        h.position.distance(anchor.position) < 0.5
                    })
                    .unwrap_or(false);

                let (r, color) = match anchor.kind {
                    AnchorKind::Main => continue,
                    AnchorKind::LaneCenter(_) => {
                        if is_hovered {
                            (4.5, Color32::from_rgb(255, 220, 90))
                        } else {
                            (3.0, Color32::from_rgb(90, 200, 255))
                        }
                    }
                    AnchorKind::Boundary(_) => {
                        if is_hovered {
                            (3.5, Color32::from_rgb(255, 220, 90))
                        } else {
                            (2.0, Color32::from_rgb(170, 170, 170))
                        }
                    }
                };

                painter.circle_filled(screen_pos, r, color);
                painter.circle_stroke(
                    screen_pos,
                    r,
                    Stroke::new(1.0_f32, Color32::from_black_alpha(140)),
                );
            }
        }
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
    let show_roads = app.mode == EditMode::Road;
    let show_pavements = app.mode == EditMode::Sublayer;
    if !show_roads && !show_pavements {
        return;
    }

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
        let owner_visible = match owner {
            PathOwner::Road(_) => show_roads,
            PathOwner::Pavement(_) => show_pavements,
        };
        if owner_visible {
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

    if show_roads {
        for a in &app.scene.current_road {
            paint_one(a.node);
        }
        for road in &app.scene.roads {
            for a in &road.anchors {
                paint_one(a.node);
            }
        }
    }
    if show_pavements {
        for a in &app.scene.current_pavement {
            paint_one(a.node);
        }
        for pa in &app.scene.pavements {
            for a in &pa.anchors {
                paint_one(a.node);
            }
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
    let draw_one = |offset: Vec2, side: HandleSide| {
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
    match app.mode {
        EditMode::Road => {
            for (i, road) in app.scene.roads.iter().enumerate() {
                for (ai, a) in road.anchors.iter().enumerate() {
                    if a.node == id {
                        return Some((PathOwner::Road(i), ai));
                    }
                }
            }
            None
        }
        EditMode::Sublayer => {
            for (i, pa) in app.scene.pavements.iter().enumerate() {
                for (ai, a) in pa.anchors.iter().enumerate() {
                    if a.node == id {
                        return Some((PathOwner::Pavement(i), ai));
                    }
                }
            }
            None
        }
        EditMode::Details | EditMode::Decorations => None,
    }
}