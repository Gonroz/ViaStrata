//! Mouse and keyboard handling that mutates the scene.

use eframe::egui::{self, PointerButton, Vec2};

use super::Xform;
use crate::app::App;
use crate::model::{PathAnchor, Tool};

/// Screen-space hit radius for anchors.
pub(crate) const NODE_HIT_RADIUS_PX: f32 = 10.0;

/// Screen-space hit radius for handle dots. Slightly smaller than the
/// anchor radius so an anchor wins over its own handles if they overlap.
const HANDLE_HIT_RADIUS_PX: f32 = 8.0;

/// Screen-space hit radius for right-click-to-insert on a road segment.
const INSERT_HIT_RADIUS_PX: f32 = 14.0;

/// Movement tolerance for our hand-rolled right-click detection.
const SECONDARY_CLICK_MAX_MOVE_PX: f32 = 8.0;

pub fn handle(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    handle_hover(app, resp, x);
    handle_drag(app, resp, x, ctx);
    handle_primary_click(app, resp, x);
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

    app.scene.hover_handle = app.scene.find_handle(img, handle_r);
    app.scene.hover_node = if app.scene.hover_handle.is_some() {
        None
    } else {
        app.scene.find_node(img, node_r)
    };
}

/// Latches drags on primary press. Applies motion per frame and clears on
/// release.
///
/// # Modifier keys
///
/// * **Plain drag on an anchor**: moves the anchor and its handles along
///   with it.
/// * **Alt-drag on an anchor**: pulls out symmetric direction handles
///   instead of moving the anchor. This is how a pavement corner becomes a
///   smooth point.
/// * **Alt-drag on a handle**: breaks the tangent symmetry, letting the two
///   handles point independently.
fn handle_drag(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    // Start
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

    // Continue
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

    // End
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.scene.dragging = None;
        app.scene.dragging_handle = None;
    }
}

fn handle_primary_click(app: &mut App, resp: &egui::Response, x: &Xform) {
    if !is_path_tool(app.tool) {
        return;
    }
    if resp.double_clicked_by(PointerButton::Primary) {
        match app.tool {
            Tool::Road => app.scene.finish_road(),
            Tool::Pavement => app.scene.finish_pavement(),
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
    let snap = app.scene.find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX));
    match app.tool {
        Tool::Road => app.scene.road_click(img, snap),
        Tool::Pavement => app.scene.pavement_click(img, snap),
        _ => {}
    }
}

/// Secondary click with a path tool. Three cases, in priority order:
///
/// 1. A draft is in progress -> finish it.
/// 2. No draft, cursor over a node -> delete that node.
/// 3. No draft, cursor over a road segment (Road tool only) -> insert a new
///    anchor there.
///
/// # Why this doesn't use `Response::clicked_by`
///
/// egui's built-in click detection requires the cursor to be at *exactly*
/// the same position on press and release. Real mouse input jitters by a
/// pixel or two, so we do our own detection: record the press position,
/// compare to the release, accept anything within `SECONDARY_CLICK_MAX_MOVE_PX`.
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

    if let Some(id) = app.scene.find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX)) {
        app.scene.delete_node(id);
        return;
    }

    if app.tool == Tool::Road {
        if let Some((ri, seg_i, proj)) = app
            .scene
            .closest_road_segment(img, x.screen_dist_to_image(INSERT_HIT_RADIUS_PX))
        {
            let new_id = app.scene.add_node(proj);
            app.scene.roads[ri]
                .anchors
                .insert(seg_i + 1, PathAnchor::new(new_id));
            // The old endpoint anchors on either side of the new one now
            // have a neighbour in a different direction; let their auto
            // tangents recompute so the curve smooths through the split.
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