//! The central canvas: pan/zoom, clicks, drawing, node editing, and
//! node-level snapping.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, pos2, Color32, PointerButton, Pos2, Rect, Stroke, Vec2};

use super::paint::{paint_attribution, paint_road, paint_road_asphalt, paint_road_markings};
use crate::app::App;
use crate::model::{NodeId, Tool};
use crate::spline;

/// Screen-space radius within which the cursor is considered to be over a
/// node. Drives the handle highlight, the drag hit-test, and the "would a
/// new point snap here" question.
const NODE_HIT_RADIUS_PX: f32 = 10.0;

/// Screen-space radius for right-click-to-insert on a road segment. Slightly
/// larger than the node radius so it's easy to hit a segment without
/// accidentally starting a drag on a nearby node.
const INSERT_HIT_RADIUS_PX: f32 = 14.0;

/// Visual radius of a rendered node handle in screen pixels.
const HANDLE_RADIUS: f32 = 4.0;

/// Visual radius of the ring drawn around a *shared* node — one referenced
/// by two or more roads. Bigger than `HANDLE_RADIUS` so the ring reads as a
/// distinct visual layer rather than a thickened outline.
const SHARED_RING_RADIUS: f32 = HANDLE_RADIUS + 3.5;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;

        if app.tex.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Enter coordinates above, or open an image",
                egui::FontId::proportional(20.0),
                Color32::GRAY,
            );
            return;
        }

        // -------- View: fit / zoom / pan ------------------------------

        if app.fit_pending {
            let z = (rect.width() / app.img_size.x).min(rect.height() / app.img_size.y) * 0.95;
            app.zoom = z;
            app.offset = (rect.size() - app.img_size * z) / 2.0;
            app.fit_pending = false;
        }

        // Zoom around the cursor: pin the image point under the cursor.
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(mp) = resp.hover_pos() {
                    let old = app.zoom;
                    let new = (old * (scroll * 0.002).exp()).clamp(0.02, 60.0);
                    let img_pt = (mp - rect.min - app.offset) / old;
                    app.zoom = new;
                    app.offset = mp - rect.min - img_pt * new;
                }
            }
        }

        // Pan: middle drag always, primary drag with the Pan tool. Right
        // drag is *deliberately* not a pan gesture — right-click has real
        // actions (finish road / delete node / insert node) and mixing those
        // with a pan would produce accidental hits. WASD also pans, from
        // `App::handle_view_keys`.
        let pan = resp.dragged_by(PointerButton::Middle)
            || (app.tool == Tool::Pan && resp.dragged_by(PointerButton::Primary));
        if pan {
            app.offset += resp.drag_delta();
        }

        // Snapshot transforms by value so `app` stays mutable inside.
        let zoom = app.zoom;
        let offset = app.offset;
        let to_s = move |p: Pos2| rect.min + offset + p.to_vec2() * zoom;
        let to_i = move |s: Pos2| ((s - rect.min - offset) / zoom).to_pos2();

        if app.tool != Tool::Pan && resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // -------- Hover detection -------------------------------------
        //
        // While a drag is in flight we pin hover to the dragged node so its
        // highlight stays lit even if the cursor slips off the handle.
        if let Some(d) = app.dragging {
            app.hover_node = Some(d);
        } else if app.tool == Tool::Road {
            app.hover_node = resp
                .hover_pos()
                .map(to_i)
                .and_then(|p| find_node(app, p, NODE_HIT_RADIUS_PX / zoom));
        } else {
            app.hover_node = None;
        }

        // -------- Drag start ------------------------------------------
        //
        // Latch on *press* rather than on egui's `drag_started_by`, because
        // by the time the drag threshold is crossed the cursor has typically
        // moved off the 10px handle. `hover_node` was computed above, so we
        // know exactly what the press landed on.
        if app.tool == Tool::Road
            && ctx.input(|i| i.pointer.primary_pressed())
            && resp.hovered()
            && app.hover_node.is_some()
        {
            app.dragging = app.hover_node;
        }

        // -------- Drag motion -----------------------------------------
        //
        // Gated on `dragged_by` so a click that doesn't cross the drag
        // threshold doesn't nudge the node by a pixel or two.
        if let Some(id) = app.dragging {
            if resp.dragged_by(PointerButton::Primary) {
                let delta = ctx.input(|i| i.pointer.delta());
                if delta != Vec2::ZERO {
                    app.move_node(id, delta / zoom);
                }
            }
        }

        // -------- Drag end --------------------------------------------
        //
        // Runs before click handling so the click handler never sees a
        // stale drag state.
        if app.dragging.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            app.dragging = None;
        }

        // -------- Primary click: place / start / terminate -------------
        //
        // If the click landed on a node, `road_click` decides whether to
        // start a draft there, terminate at it, or ignore the click. If it
        // landed on empty canvas, a fresh node is created at the click point.
        if resp.clicked_by(PointerButton::Primary) && app.tool == Tool::Road {
            if let Some(screen) = resp.interact_pointer_pos() {
                let img = to_i(screen);
                if app.px_per_m.is_some() {
                    let snap = find_node(app, img, NODE_HIT_RADIUS_PX / zoom);
                    app.road_click(img, snap);
                } else {
                    app.status =
                        "Set the scale first (fetch satellite or calibrate).".into();
                }
            }
        }

        // Double-click also finishes the draft road. Fires *after* the
        // single-click handler would place a point, but egui suppresses the
        // single click when a double is detected, so no duplicate point.
        if resp.double_clicked_by(PointerButton::Primary) && app.tool == Tool::Road {
            app.finish_road();
        }

        // -------- Secondary click --------------------------------------
        //
        // Three cases, in priority order:
        //
        // 1. A draft is in progress -> right-click finishes the road. This
        //    is the "I'm done drawing" gesture, and it takes precedence over
        //    deletion so you can't accidentally nuke a node while drawing.
        // 2. No draft, cursor over a node -> delete that node. Splits any
        //    road where the node sat in the middle; orphans are cleaned up
        //    inside `App::delete_node`.
        // 3. No draft, cursor over a road segment -> insert a new node at
        //    the projected point, giving the user something to grab without
        //    changing the road's shape.
        //
        // Right-drag is not a pan gesture (see the pan condition above), so
        // there's no way to confuse a pan with a right-click. Movement
        // threshold still applies: a right *click* is a press-and-release
        // with no movement, which is the only thing that fires these.
        if resp.clicked_by(PointerButton::Secondary) && app.tool == Tool::Road {
            if !app.current.is_empty() {
                app.finish_road();
            } else if let Some(screen) = resp.interact_pointer_pos() {
                let img = to_i(screen);
                if let Some(id) = find_node(app, img, NODE_HIT_RADIUS_PX / zoom) {
                    app.delete_node(id);
                } else if let Some((ri, seg_i, proj)) =
                    closest_road_segment(app, img, INSERT_HIT_RADIUS_PX / zoom)
                {
                    let new_id = app.add_node(proj);
                    app.roads[ri].nodes.insert(seg_i + 1, new_id);
                }
            }
        }

        // -------- Calibration clicks -----------------------------------
        if resp.clicked_by(PointerButton::Primary) && app.tool == Tool::Calibrate {
            if let Some(screen) = resp.interact_pointer_pos() {
                let ip = to_i(screen);
                if app.calib_pts.len() >= 2 {
                    app.calib_pts.clear();
                }
                app.calib_pts.push(ip);
            }
        }

        // -------- Delete hovered node via keyboard ---------------------
        //
        // Delete/Backspace still work as a keyboard alternative. Same effect
        // as the right-click path above.
        if app.tool == Tool::Road
            && app.hover_node.is_some()
            && !ctx.wants_keyboard_input()
            && ctx.input(|i| {
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
            })
        {
            if let Some(id) = app.hover_node {
                app.delete_node(id);
            }
        }

        // -------- Draw ------------------------------------------------

        if let Some(tex) = &app.tex {
            let r = Rect::from_min_size(to_s(Pos2::ZERO), app.img_size * zoom);
            painter.image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let ppm = app.px_per_m.unwrap_or(1.0);

        // Nodes shared by two or more roads (or by the draft and a committed
        // road) get a joint fill and a ring around their handle. Computed
        // once per frame.
        let shared = collect_shared_nodes(app);

        // Pass 1: all asphalt, including joint fills. Doing all asphalt
        // before any markings means a joint fill never covers another road's
        // lane lines.
        for road in &app.roads {
            let pos = app.road_positions(road);
            let smooth = spline::catmull_rom(&pos);
            let screen: Vec<Pos2> = smooth.iter().map(|&p| to_s(p)).collect();
            let w = road.lane_m * road.lanes as f32 * ppm * zoom;
            let joints: Vec<Pos2> = road
                .nodes
                .iter()
                .filter(|id| shared.contains(id))
                .filter_map(|id| app.node_pos(*id))
                .map(to_s)
                .collect();
            paint_road_asphalt(&painter, &screen, w, &joints);
        }

        // Pass 2: all markings.
        for road in &app.roads {
            let pos = app.road_positions(road);
            let smooth = spline::catmull_rom(&pos);
            let screen: Vec<Pos2> = smooth.iter().map(|&p| to_s(p)).collect();
            let w = road.lane_m * road.lanes as f32 * ppm * zoom;
            paint_road_markings(&painter, &screen, w, road.lanes, road.center);
        }

        // Draft road, on top of committed roads. Preview segment to the
        // cursor goes to the snapped node if the cursor is over one, or to
        // the raw cursor position otherwise — so the preview shows exactly
        // what will happen if you click.
        if !app.current.is_empty() {
            let mut ctrl = app.draft_positions();
            if let Some(h) = resp.hover_pos() {
                let img = to_i(h);
                let preview_target = find_node(app, img, NODE_HIT_RADIUS_PX / zoom)
                    .and_then(|id| app.node_pos(id))
                    .unwrap_or(img);
                ctrl.push(preview_target);
            }
            let smooth = spline::catmull_rom(&ctrl);
            let screen: Vec<Pos2> = smooth.iter().map(|&p| to_s(p)).collect();
            let w = app.lane_m * app.lanes as f32 * ppm * zoom;
            let joints: Vec<Pos2> = app
                .current
                .iter()
                .filter(|id| shared.contains(id))
                .filter_map(|id| app.node_pos(*id))
                .map(to_s)
                .collect();
            paint_road(
                &painter,
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
            paint_handles(app, &painter, &to_s, &shared);
        }

        // Calibration markers.
        if !app.calib_pts.is_empty() {
            let red = Color32::from_rgb(235, 60, 60);
            let pts: Vec<Pos2> = app.calib_pts.iter().map(|&p| to_s(p)).collect();
            if pts.len() == 2 {
                painter.line_segment([pts[0], pts[1]], Stroke::new(2.0_f32, red));
            }
            for q in pts {
                painter.circle_stroke(q, 6.0, Stroke::new(2.0_f32, red));
            }
        }

        if !app.attribution.is_empty() {
            paint_attribution(&painter, rect, &app.attribution);
        }
    });
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
/// The hovered node's fill (and therefore its ring, which tracks the fill)
/// turns yellow. Because the ring uses the fill colour rather than a fixed
/// one, a hovered shared node is unambiguously "shared and under the
/// cursor" — the two rings pulse together.
///
/// Deduplicates by `NodeId` so a shared node's handle is painted exactly
/// once even if many roads reference it.
fn paint_handles(
    app: &App,
    painter: &egui::Painter,
    to_s: &impl Fn(Pos2) -> Pos2,
    shared: &HashSet<NodeId>,
) {
    let base = Color32::from_rgb(220, 220, 220);
    let hover = Color32::from_rgb(255, 210, 80);
    let outline = Color32::from_black_alpha(160);

    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut paint_one = |id: NodeId| {
        if !seen.insert(id) {
            return;
        }
        let Some(p) = app.node_pos(id) else {
            return;
        };
        let fill = if app.hover_node == Some(id) { hover } else { base };
        let s = to_s(p);

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

/// Finds the nearest node to `img_pt` within `radius` image pixels.
///
/// The draft's nodes are checked first so that, on a tie, a point the user
/// just placed wins over one buried under an older road.
fn find_node(app: &App, img_pt: Pos2, radius: f32) -> Option<NodeId> {
    let r2 = radius * radius;
    let mut best: Option<(NodeId, f32)> = None;
    let mut seen: HashSet<NodeId> = HashSet::new();

    let mut consider = |id: NodeId| {
        if !seen.insert(id) {
            return;
        }
        let Some(p) = app.node_pos(id) else {
            return;
        };
        let d2 = (p - img_pt).length_sq();
        if d2 <= r2 {
            match best {
                Some((_, bd2)) if bd2 <= d2 => {}
                _ => best = Some((id, d2)),
            }
        }
    };

    for &id in &app.current {
        consider(id);
    }
    for road in &app.roads {
        for &id in &road.nodes {
            consider(id);
        }
    }
    best.map(|(id, _)| id)
}

/// Returns the set of nodes referenced by two or more roads (or by the
/// draft and any road). These are the nodes that get a joint fill and a
/// ring around their handle.
fn collect_shared_nodes(app: &App) -> HashSet<NodeId> {
    let mut counts: HashMap<NodeId, usize> = HashMap::new();
    for road in &app.roads {
        for &id in &road.nodes {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    for &id in &app.current {
        *counts.entry(id).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .map(|(id, _)| id)
        .collect()
}

/// Finds the closest committed-road segment to `pos`, returning
/// `(road_index, segment_start_index, projected_point)`.
///
/// The segment spans node indices `segment_start_index` and
/// `segment_start_index + 1`. The caller inserts the new node at
/// `segment_start_index + 1` to place it *between* the two existing nodes,
/// preserving the road's shape.
///
/// Uses the raw control points, not the tessellated spline. The spline
/// passes through every control point, so the approximation is close enough
/// for a click-to-insert gesture — you're aiming at a visible piece of road,
/// not at an exact curve parameter.
fn closest_road_segment(
    app: &App,
    pos: Pos2,
    max_dist: f32,
) -> Option<(usize, usize, Pos2)> {
    let max_sq = max_dist * max_dist;
    let mut best: Option<(usize, usize, Pos2, f32)> = None;

    for (ri, road) in app.roads.iter().enumerate() {
        let pts = app.road_positions(road);
        // Only iterate segments whose endpoints both resolved. `pts` should
        // have the same length as `road.nodes` under normal operation, but
        // we bound by `pts.len()` to be safe.
        for i in 0..pts.len().saturating_sub(1) {
            let a = pts[i];
            let b = pts[i + 1];
            let proj = project_point_to_segment(pos, a, b);
            let d2 = (proj - pos).length_sq();
            if d2 <= max_sq {
                match best {
                    Some((_, _, _, bd2)) if bd2 <= d2 => {}
                    _ => best = Some((ri, i, proj, d2)),
                }
            }
        }
    }

    best.map(|(ri, i, p, _)| (ri, i, p))
}

/// Projects `p` onto the line segment `a..b`, clamped to the segment's
/// endpoints so the result is always on the segment.
fn project_point_to_segment(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}