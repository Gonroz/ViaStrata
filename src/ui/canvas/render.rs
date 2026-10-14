//! Drawing the scene with the current camera.
//!
//! This module knows nothing about input. It reads App state, transforms
//! through the `Xform`, and paints. Any hit-testing is a *query* on App
//! (see `find_node`, `shared_nodes`), not a mutation.

use std::collections::HashSet;

use eframe::egui::{self, pos2, Color32, Pos2, Rect, Stroke};

use super::input::NODE_HIT_RADIUS_PX;
use super::Xform;
use crate::app::App;
use crate::model::{NodeId, Tool};
use crate::spline;
use crate::ui::paint::{paint_attribution, paint_road, paint_road_asphalt, paint_road_markings};

/// Visual radius of a rendered node handle in screen pixels.
const HANDLE_RADIUS: f32 = 4.0;

/// Visual radius of the ring drawn around a *shared* node — one referenced
/// by two or more roads. Bigger than `HANDLE_RADIUS` so the ring reads as a
/// distinct visual layer rather than a thickened outline.
const SHARED_RING_RADIUS: f32 = HANDLE_RADIUS + 3.5;

pub fn draw(
    app: &App,
    painter: &egui::Painter,
    resp: &egui::Response,
    x: &Xform,
    rect: Rect,
) {
    // Base image.
    if let Some(tex) = &app.view.tex {
        let r = Rect::from_min_size(x.to_screen(Pos2::ZERO), app.view.img_size * x.zoom());
        painter.image(
            tex.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    // Fall back to 1 px/m if scale isn't set yet — only matters for the
    // draft preview, which can't be sized correctly without it.
    let ppm = app.px_per_m.unwrap_or(1.0);

    // Nodes shared by two or more roads (or by the draft and a committed
    // road) get a joint fill and a ring around their handle. Computed once
    // per frame.
    let shared = app.shared_nodes();

    // Pass 1: all asphalt, including joint fills. Doing all asphalt before
    // any markings means a joint fill never covers another road's lane
    // lines, and a later road doesn't erase an earlier one's asphalt at a
    // shared vertex.
    for road in &app.roads {
        let pos = app.road_positions(road);
        let smooth = spline::catmull_rom(&pos);
        let screen: Vec<Pos2> = smooth.iter().map(|&p| x.to_screen(p)).collect();
        let w = road.lane_m * road.lanes as f32 * ppm * x.zoom();
        let joints: Vec<Pos2> = road
            .nodes
            .iter()
            .filter(|id| shared.contains(id))
            .filter_map(|id| app.graph.pos(*id))
            .map(|p| x.to_screen(p))
            .collect();
        paint_road_asphalt(painter, &screen, w, &joints);
    }

    // Pass 2: all markings.
    for road in &app.roads {
        let pos = app.road_positions(road);
        let smooth = spline::catmull_rom(&pos);
        let screen: Vec<Pos2> = smooth.iter().map(|&p| x.to_screen(p)).collect();
        let w = road.lane_m * road.lanes as f32 * ppm * x.zoom();
        paint_road_markings(painter, &screen, w, road.lanes, road.center);
    }

    // Draft road, on top of committed roads. Preview segment to the cursor
    // goes to the snapped node if the cursor is over one, or to the raw
    // cursor position otherwise — so the preview shows exactly what will
    // happen if you click.
    if !app.current.is_empty() {
        let mut ctrl = app.draft_positions();
        if let Some(h) = resp.hover_pos() {
            let img = x.to_image(h);
            let preview_target = app
                .find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX))
                .and_then(|id| app.graph.pos(id))
                .unwrap_or(img);
            ctrl.push(preview_target);
        }
        let smooth = spline::catmull_rom(&ctrl);
        let screen: Vec<Pos2> = smooth.iter().map(|&p| x.to_screen(p)).collect();
        let w = app.lane_m * app.lanes as f32 * ppm * x.zoom();
        let joints: Vec<Pos2> = app
            .current
            .iter()
            .filter(|id| shared.contains(id))
            .filter_map(|id| app.graph.pos(*id))
            .map(|p| x.to_screen(p))
            .collect();
        paint_road(
            painter,
            &screen,
            w,
            app.lanes,
            app.center_style,
            &joints,
        );
    }

    // Node handles. Drawn last so they sit on top, and only for the Road
    // tool so they don't clutter Pan/Calibrate.
    if app.tool == Tool::Road {
        paint_handles(app, painter, x, &shared);
    }

    // Calibration markers.
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

/// Paints a handle dot for every node in the graph.
///
/// Two visual states:
///
/// * **Solo node** (referenced by exactly one road): a small filled dot
///   with a thin black outline. This is a road's own control point.
/// * **Shared node** (referenced by two or more roads): the same dot *plus*
///   a ring at `SHARED_RING_RADIUS`, drawn in the same colour as the fill.
///   The ring reads as a "bullseye" — a visually distinct shape without
///   introducing a new colour that would compete with the yellow hover
///   highlight.
///
/// Deduplicates by `NodeId` so a shared node's handle is painted exactly
/// once even if many roads reference it.
fn paint_handles(app: &App, painter: &egui::Painter, x: &Xform, shared: &HashSet<NodeId>) {
    let base = Color32::from_rgb(220, 220, 220);
    let hover = Color32::from_rgb(255, 210, 80);
    let outline = Color32::from_black_alpha(160);

    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut paint_one = |id: NodeId| {
        if !seen.insert(id) {
            return;
        }
        let Some(p) = app.graph.pos(id) else { return };
        let fill = if app.hover_node == Some(id) { hover } else { base };
        let s = x.to_screen(p);

        // Ring first so the fill sits on top of it. The ring uses the same
        // colour as the fill, so it tracks hover automatically.
        if shared.contains(&id) {
            painter.circle_stroke(s, SHARED_RING_RADIUS, Stroke::new(2.0_f32, fill));
        }

        painter.circle_filled(s, HANDLE_RADIUS, fill);
        painter.circle_stroke(s, HANDLE_RADIUS, Stroke::new(1.0_f32, outline));
    };

    // Draft first so its handle wins the highlight on a tie.
    for &id in &app.current {
        paint_one(id);
    }
    for road in &app.roads {
        for &id in &road.nodes {
            paint_one(id);
        }
    }
}