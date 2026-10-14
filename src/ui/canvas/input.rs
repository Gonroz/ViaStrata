//! Mouse and keyboard handling that mutates the scene.
//!
//! All hit-tests are done in *image* space; the `Xform` converts the
//! screen-space mouse position and the constant screen radii into image
//! space, so a threshold that reads "10 pixels" stays 10 screen pixels
//! regardless of zoom.

use eframe::egui::{self, PointerButton, Vec2};

use super::Xform;
use crate::app::App;
use crate::model::Tool;

/// Screen-space radius within which the cursor is considered to be over a
/// node. Drives the handle highlight, the drag hit-test, and the "would a
/// new point snap here" question.
///
/// `pub(crate)` because the draft preview in `render` needs the same radius
/// to decide which node the preview segment should snap to. Keeping them
/// equal means the visual feedback matches the click behavior exactly.
pub(crate) const NODE_HIT_RADIUS_PX: f32 = 10.0;

/// Screen-space radius for right-click-to-insert on a road segment. Slightly
/// larger than the node radius so it's easy to hit a segment without
/// accidentally starting a drag on a nearby node.
const INSERT_HIT_RADIUS_PX: f32 = 14.0;

/// How far the cursor may move between secondary press and release and still
/// count as a "click" rather than a drag. egui's own click detection uses a
/// movement threshold of essentially zero on `click_and_drag` widgets, which
/// makes right-clicks unreliable in practice: a hand tremor of two pixels
/// turns a click into a drag. We implement our own detection with a
/// generously larger tolerance, since there is no right-drag gesture in this
/// app to confuse it with.
const SECONDARY_CLICK_MAX_MOVE_PX: f32 = 8.0;

pub fn handle(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    handle_hover(app, resp, x);
    handle_drag(app, resp, x, ctx);
    handle_primary_click(app, resp, x);
    handle_secondary_click(app, resp, x, ctx);
    handle_calibration_click(app, resp, x);
    handle_delete_key(app, ctx);
}

/// Updates `app.hover_node`. While a drag is in flight we pin hover to the
/// dragged node so its highlight stays lit even if the cursor slips off the
/// handle.
fn handle_hover(app: &mut App, resp: &egui::Response, x: &Xform) {
    if let Some(d) = app.dragging {
        app.hover_node = Some(d);
    } else if app.tool == Tool::Road {
        let r = x.screen_dist_to_image(NODE_HIT_RADIUS_PX);
        app.hover_node = resp
            .hover_pos()
            .map(|s| x.to_image(s))
            .and_then(|p| app.find_node(p, r));
    } else {
        app.hover_node = None;
    }
}

/// Latches drag on primary *press* rather than on egui's `drag_started_by`,
/// because by the time the drag threshold is crossed the cursor has
/// typically moved off the handle. Then applies per-frame motion and clears
/// on release.
fn handle_drag(app: &mut App, resp: &egui::Response, x: &Xform, ctx: &egui::Context) {
    if app.tool == Tool::Road
        && ctx.input(|i| i.pointer.primary_pressed())
        && resp.hovered()
        && app.hover_node.is_some()
    {
        app.dragging = app.hover_node;
    }

    if let Some(id) = app.dragging {
        if resp.dragged_by(PointerButton::Primary) {
            let delta = ctx.input(|i| i.pointer.delta());
            if delta != Vec2::ZERO {
                app.move_node(id, delta / x.zoom());
            }
        }
    }

    // Runs before click handling so the click handler never sees a stale
    // drag state.
    if app.dragging.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
        app.dragging = None;
    }
}

/// Primary click with the Road tool: place / start / terminate.
///
/// # Ordering
///
/// The double-click check comes *first*. egui reports `double_clicked`
/// **instead of** `clicked` on the second click of a double, so a handler
/// that returned early on `!clicked_by(Primary)` would never reach the
/// double-click branch. Checking double first avoids that.
fn handle_primary_click(app: &mut App, resp: &egui::Response, x: &Xform) {
    if app.tool != Tool::Road {
        return;
    }
    if resp.double_clicked_by(PointerButton::Primary) {
        app.finish_road();
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
    let snap = app.find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX));
    app.road_click(img, snap);
}

/// Secondary click with the Road tool. Three cases, in priority order:
///
/// 1. A draft is in progress -> finish the road. This takes precedence over
///    deletion so you can't accidentally nuke a node while drawing.
/// 2. No draft, cursor over a node -> delete that node.
/// 3. No draft, cursor over a road segment -> insert a new node there.
///
/// # Why this doesn't use `Response::clicked_by`
///
/// egui's built-in click detection requires the cursor to be at *exactly*
/// the same position on press and release. On a real mouse that almost never
/// happens — a hand tremor of one or two pixels is enough for egui to
/// classify the gesture as a drag and withhold the click event. The
/// user-visible symptom is that right-click "works sometimes."
///
/// So we implement our own detection:
///
/// * On secondary press, if the cursor is over the canvas, record the
///   press position.
/// * On secondary release, compare the release position to the recorded
///   press position. If they're within `SECONDARY_CLICK_MAX_MOVE_PX`,
///   treat it as a click and act at the *press* position (where the user
///   aimed), not the release position.
///
/// The press-position-recorded-only-when-hovering gate keeps a secondary
/// press that began on a toolbar or side panel from firing an action on the
/// canvas when it happens to be released over it.
fn handle_secondary_click(
    app: &mut App,
    resp: &egui::Response,
    x: &Xform,
    ctx: &egui::Context,
) {
    if app.tool != Tool::Road {
        // Still clear the press record so a tool switch mid-gesture doesn't
        // leave stale state around. (Edge case, but cheap to guard.)
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

    // Record the press, but only if it began over the canvas.
    if pressed && resp.hovered() {
        if let Some(pos) = cursor {
            app.secondary_press = Some(pos);
        }
    }

    // On release, decide whether it was a click.
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

    // We have a click. Act at the press position.
    let img = x.to_image(press_pos);

    if !app.current.is_empty() {
        app.finish_road();
        return;
    }
    if let Some(id) = app.find_node(img, x.screen_dist_to_image(NODE_HIT_RADIUS_PX)) {
        app.delete_node(id);
        return;
    }
    if let Some((ri, seg_i, proj)) =
        app.closest_road_segment(img, x.screen_dist_to_image(INSERT_HIT_RADIUS_PX))
    {
        let new_id = app.add_node(proj);
        app.roads[ri].nodes.insert(seg_i + 1, new_id);
    }
}

/// Calibration: primary click records a measurement point. A third click
/// restarts the measurement.
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

/// Delete / Backspace over a hovered node. Keyboard alternative to
/// right-click delete.
fn handle_delete_key(app: &mut App, ctx: &egui::Context) {
    if app.tool != Tool::Road || app.hover_node.is_none() || ctx.wants_keyboard_input() {
        return;
    }
    let pressed = ctx.input(|i| {
        i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
    });
    if pressed {
        if let Some(id) = app.hover_node {
            app.delete_node(id);
        }
    }
}