//! Mouse and keyboard handling that mutates the scene.

use eframe::egui::{self, PointerButton, Vec2};

use super::Xform;
use crate::app::App;
use crate::model::{EditMode, PathAnchor, Tool};

pub(crate) const NODE_HIT_RADIUS_PX: f32 = 10.0;
const HANDLE_HIT_RADIUS_PX: f32 = 8.0;
const INSERT_HIT_RADIUS_PX: f32 = 14.0;
const SECONDARY_CLICK_MAX_MOVE_PX: f32 = 8.0;

pub fn handle(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    handle_hover(app, resp, x);
    handle_drag(app, resp, x, ctx);
    handle_primary_click(app, resp, x, ctx);
    handle_secondary_click(app, resp, x, ctx);
    handle_calibration_click(app, resp, x);
    handle_delete_key(app, ctx);
}

fn is_path_tool(tool: Tool) -> bool {
    matches!(tool, Tool::Road | Tool::Pavement)
}

fn handle_hover(app: &mut App, resp: &egui::Response, x: &Xform) {
    if let Some(h) = app.scene.dragging_handle {
        app.scene.hover_handle = Some(h);
        app.scene.hover_node = None;
        return;
    }
    if let Some(n) = app.scene.dragging {
        app.scene.hover_node = Some(n);
        app.scene.hover_handle = None;
        return;
    }

    if !is_path_tool(app.tool) {
        app.scene.hover_node = None;
        app.scene.hover_handle = None;
        return;
    }

    let Some(screen) = resp.hover_pos() else {
        app.scene.hover_node = None;
        app.scene.hover_handle = None;
        return;
    };
    let img = x.to_image(screen);

    let handle_r = x.screen_dist_to_image(HANDLE_HIT_RADIUS_PX);
    let node_r = x.screen_dist_to_image(NODE_HIT_RADIUS_PX);

    app.scene.hover_handle = app.scene.find_handle(app.mode, img, handle_r);
    app.scene.hover_node = if app.scene.hover_handle.is_some() {
        None
    } else {
        app.scene.find_node(app.mode, img, node_r)
    };
}

fn handle_drag(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    if is_path_tool(app.tool)
        && ctx.input(|i| i.pointer.primary_pressed())
        && resp.hovered()
    {
        if app.scene.hover_handle.is_some() {
            app.scene.dragging_handle = app.scene.hover_handle;
            app.scene.dragging = None;
        } else if app.scene.hover_node.is_some() {
            app.scene.dragging = app.scene.hover_node;
            app.scene.dragging_handle = None;
        }
    }

    if resp.dragged_by(PointerButton::Primary) {
        let delta = ctx.input(|i| i.pointer.delta());
        if delta != Vec2::ZERO {
            let img_delta = delta / x.zoom();
            let alt = ctx.input(|i| i.modifiers.alt);

            if let Some(href) = app.scene.dragging_handle {
                app.scene.move_handle(href, img_delta, alt);
            } else if let Some(id) = app.scene.dragging {
                if alt {
                    app.scene.pull_out_handles(id, img_delta);
                } else {
                    app.scene.move_node(id, img_delta);
                }
            }
        }
    }

    if !ctx.input(|i| i.pointer.primary_down()) {
        app.scene.dragging = None;
        app.scene.dragging_handle = None;
    }
}

/// Primary click with a path tool.
///
/// Road tool priority, high to low:
///
/// 1. Mid-draft → place the next point (node snap only, existing behavior).
/// 2. Empty draft, on a lane anchor → start a new road there, inheriting
///    the source's direction (Shift held = place at the anchor position
///    but don't inherit).
/// 3. Empty draft, on a node → start a new road there (existing behavior).
/// 4. Empty draft, on a road body → select that segment.
/// 5. Empty draft, on empty canvas → start a new road freely.
///
/// Pavement tool is unchanged: node snap or free placement.
fn handle_primary_click(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    if !is_path_tool(app.tool) {
        return;
    }
    if resp.double_clicked_by(PointerButton::Primary) {
        match (app.mode, app.tool) {
            (EditMode::Road, Tool::Road) => app.scene.finish_road(),
            (EditMode::Sublayer, Tool::Pavement) => app.scene.finish_pavement(),
            _ => {}
        }
        return;
    }
    if !resp.clicked_by(PointerButton::Primary) {
        return;
    }
    let Some(screen) = resp.interact_pointer_pos() else {
        return;
    };
    let img = x.to_image(screen);
    if app.px_per_m.is_none() {
        app.status = "Set the scale first (fetch satellite or calibrate).".into();
        return;
    }
    let ppm = app.px_per_m.unwrap();
    let node_r = x.screen_dist_to_image(NODE_HIT_RADIUS_PX);
    let snap = app.scene.find_node(app.mode, img, node_r);

    match (app.mode, app.tool) {
        (EditMode::Road, Tool::Road) => {
            if !app.scene.current_road.is_empty() {
                // Mid-drawing: existing behavior, node snap only.
                app.scene.road_click(img, snap);
            } else if let Some(hit) = app.scene.nearest_lane_anchor(app.mode, img, node_r, ppm) {
                let inherit = !ctx.input(|i| i.modifiers.shift);
                app.scene.road_anchor_click(hit, inherit);
            } else if snap.is_some() {
                app.scene.deselect_segment();
                app.scene.road_click(img, snap);
            } else if let Some(seg) = road_hit(app, img) {
                app.scene.select_segment(seg.road, seg.segment);
            } else {
                app.scene.deselect_segment();
                app.scene.road_click(img, None);
            }
        }
        (EditMode::Sublayer, Tool::Pavement) => {
            app.scene.pavement_click(img, snap);
        }
        _ => {}
    }
}

fn road_hit(app: &App, img_pos: eframe::egui::Pos2) -> Option<crate::scene::SegmentRef> {
    let ppm = app.px_per_m.unwrap_or(1.0);
    app.scene.segment_at(img_pos, ppm)
}

fn handle_secondary_click(
    app: &mut App,
    resp: &egui::Response,
    x: &Xform,
    ctx: &egui::Context,
) {
    if !is_path_tool(app.tool) {
        app.secondary_press = None;
        return;
    }

    let (pressed, released, cursor) = ctx.input(|i| {
        (
            i.pointer.secondary_pressed(),
            i.pointer.secondary_released(),
            i.pointer.interact_pos(),
        )
    });

    if pressed && resp.hovered() {
        if let Some(pos) = cursor {
            app.secondary_press = Some(pos);
        }
    }

    if !released {
        return;
    }
    let Some(press_pos) = app.secondary_press.take() else {
        return;
    };
    let Some(release_pos) = cursor else {
        return;
    };
    if (release_pos - press_pos).length() > SECONDARY_CLICK_MAX_MOVE_PX {
        return;
    }

    let img = x.to_image(press_pos);

    if !app.scene.current_road.is_empty() {
        app.scene.finish_road();
        return;
    }
    if !app.scene.current_pavement.is_empty() {
        app.scene.finish_pavement();
        return;
    }
    if app.scene.selected_segment.is_some() {
        app.scene.deselect_segment();
        return;
    }

    if let Some(id) = app
        .scene
        .find_node(app.mode, img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX))
    {
        app.scene.delete_node(id);
        return;
    }

    if app.mode == EditMode::Road && app.tool == Tool::Road {
        if let Some((ri, seg_i, proj)) = app
            .scene
            .closest_road_segment(img, x.screen_dist_to_image(INSERT_HIT_RADIUS_PX))
        {
            let new_id = app.scene.add_node(proj);
            app.scene.roads[ri]
                .anchors
                .insert(seg_i + 1, PathAnchor::new(new_id));
            if let Some(a) = app.scene.roads[ri].anchors.get_mut(seg_i) {
                a.auto = true;
            }
            if let Some(a) = app.scene.roads[ri].anchors.get_mut(seg_i + 2) {
                a.auto = true;
            }
            app.scene.refresh_auto_handles();
        }
    }
}

fn handle_calibration_click(app: &mut App, resp: &egui::Response, x: &Xform) {
    if !resp.clicked_by(PointerButton::Primary) || app.tool != Tool::Calibrate {
        return;
    }
    let Some(screen) = resp.interact_pointer_pos() else {
        return;
    };
    let ip = x.to_image(screen);
    if app.calib_pts.len() >= 2 {
        app.calib_pts.clear();
    }
    app.calib_pts.push(ip);
}

fn handle_delete_key(app: &mut App, ctx: &egui::Context) {
    if !is_path_tool(app.tool) || app.scene.hover_node.is_none() || ctx.wants_keyboard_input() {
        return;
    }
    let pressed = ctx.input(|i| {
        i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
    });
    if pressed {
        if let Some(id) = app.scene.hover_node {
            app.scene.delete_node(id);
        }
    }
}