//! Camera state: what the user sees and how.
//!
//! Split from input and render because the camera's lifetime is different:
//! it survives image reloads and is modified by both mouse gestures (zoom,
//! pan) and keyboard (WASD). It lives on `App` as `app.view`.

use eframe::egui::{self, PointerButton, Rect, TextureHandle, Vec2};

use crate::app::App;
use crate::model::Tool;

/// Camera and base-image state.
#[derive(Default)]
pub struct View {
    /// GPU texture of the loaded base image, or `None` before any is set.
    pub tex: Option<TextureHandle>,
    /// Size of the base image in *image* pixels (not screen pixels).
    pub img_size: Vec2,
    /// Screen pixels per image pixel.
    pub zoom: f32,
    /// Screen offset of the image origin, relative to the canvas top-left.
    pub offset: Vec2,
    /// Set when a new image is loaded; the canvas consumes it on the next
    /// frame to fit the image to the viewport exactly once.
    pub fit_pending: bool,
}

impl View {
    /// Installs a fresh texture and schedules a one-shot fit-to-window on
    /// the next frame. Called by `App::set_base`.
    pub fn reset_for_new_image(&mut self, tex: TextureHandle, size: Vec2) {
        self.tex = Some(tex);
        self.img_size = size;
        self.zoom = 1.0;
        self.offset = Vec2::ZERO;
        self.fit_pending = true;
    }
}

/// Applies the camera for this frame: fit-on-load, zoom-under-cursor, and
/// drag-pan.
///
/// Deliberately separate from `input::handle` and `render::draw` so camera
/// changes and scene changes don't interleave in the same function.
pub fn apply_camera(app: &mut App, resp: &egui::Response, rect: Rect) {
    // One-shot fit on the first frame after a new image was installed.
    if app.view.fit_pending {
        let z = (rect.width() / app.view.img_size.x)
            .min(rect.height() / app.view.img_size.y)
            * 0.95;
        app.view.zoom = z;
        app.view.offset = (rect.size() - app.view.img_size * z) / 2.0;
        app.view.fit_pending = false;
    }

    // Zoom around the cursor: compute the image point under the cursor
    // *before* changing zoom, then re-solve offset so that point stays
    // pinned under the cursor afterwards.
    if resp.hovered() {
        let scroll = resp.ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            if let Some(mp) = resp.hover_pos() {
                let old = app.view.zoom;
                let new = (old * (scroll * 0.002).exp()).clamp(0.02, 60.0);
                let img_pt = (mp - rect.min - app.view.offset) / old;
                app.view.zoom = new;
                app.view.offset = mp - rect.min - img_pt * new;
            }
        }
    }

    // Pan: middle drag always, primary drag with the Pan tool. Right drag
    // is deliberately not a pan gesture — right-click has real actions
    // (finish road / delete node / insert node), and mixing those with a
    // pan would produce accidental hits.
    let pan = resp.dragged_by(PointerButton::Middle)
        || (app.tool == Tool::Pan && resp.dragged_by(PointerButton::Primary));
    if pan {
        app.view.offset += resp.drag_delta();
    }
}

/// WASD camera movement. Called from `App::update` every frame so it works
/// regardless of whether the canvas has hover/focus.
///
/// W/S move the camera up/down, A/D move it left/right — i.e. the content
/// moves the opposite way on screen. That's the convention every map and
/// RTS game uses.
///
/// Skipped when a text field has focus so typing "w" in the coordinate box
/// doesn't pan.
pub fn apply_wasd(app: &mut App, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() {
        return;
    }

    // Read direction + delta-time in one closure to avoid holding a borrow
    // across the mutation below.
    let (dir, dt) = ctx.input(|i| {
        let mut d = Vec2::ZERO;
        if i.key_down(egui::Key::W) {
            d.y += 1.0;
        }
        if i.key_down(egui::Key::S) {
            d.y -= 1.0;
        }
        if i.key_down(egui::Key::A) {
            d.x += 1.0;
        }
        if i.key_down(egui::Key::D) {
            d.x -= 1.0;
        }
        (d, i.stable_dt)
    });

    if dir == Vec2::ZERO {
        return;
    }

    // 700 logical px/sec feels right for a 1300px-wide window: enough to
    // cross the whole viewport in under two seconds, not so fast that fine
    // positioning is impossible.
    const SPEED_PX_PER_SEC: f32 = 700.0;

    // Clamp dt so that if the app was backgrounded while a key was held,
    // we don't take a huge jump on the first frame back.
    let dt = dt.min(0.1);
    app.view.offset += dir * SPEED_PX_PER_SEC * dt;

    // Egui only repaints on input events by default. Continuous motion
    // needs an explicit request, otherwise the view would only step once
    // per key event.
    ctx.request_repaint();
}