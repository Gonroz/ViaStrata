//! Camera state: what the user sees and how.
//!
//! Split from input and render because the camera's lifetime is different:
//! it survives image reloads and is modified by both mouse gestures (zoom,
//! pan) and keyboard (WASD). It lives on `App` as `app.view`.
//!
//! Image upload also lives here rather than on `App`, because the texture
//! handle *is* part of the view state and the unique-name counter that
//! guarantees a clean re-upload belongs right next to it.

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

    /// Counter for generating unique texture names.
    ///
    /// Every `install_image` call uses a fresh name like `base_3`, `base_4`,
    /// and so on. That forces egui to fully free the previous GPU texture
    /// and allocate a new one, which is important for two reasons:
    ///
    /// * **Mipmaps.** egui's in-place texture update path re-uploads the
    ///   pixel data but doesn't reliably rebuild the mipmap chain. Without
    ///   fresh mipmaps, the minified view (i.e. the fit-to-window zoom)
    ///   looks soft and aliased, because the GPU has to sample the
    ///   full-resolution texture with a low-quality filter.
    ///
    /// * **Size changes.** If a new fetch returns an image of a different
    ///   size — because the user changed zoom, radius, or source — updating
    ///   in place leaves the size metadata and the actual pixel buffer
    ///   briefly out of sync. A fresh allocation avoids that entirely.
    ///
    /// The cost is one extra texture allocation per load, which is nothing.
    next_tex_name: u32,
}

impl View {
    /// Installs a fresh image as the base layer.
    ///
    /// Uploads the pixels under a unique texture name, resets the camera to
    /// `zoom = 1` at offset zero, and schedules a one-shot fit-to-window
    /// for the next frame. The fit is what gives a freshly loaded image a
    /// crisp, centered display that matches its actual pixel dimensions.
    pub fn install_image(&mut self, ctx: &egui::Context, img: &image::RgbaImage) {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());

        // Linear minification with a mipmap chain keeps the fit-to-window
        // view crisp. Linear magnification keeps pixel-level zoom smooth
        // rather than blocky. Clamp-to-edge avoids a visible border seam
        // when the image is displayed at exactly 1:1 and a sample lands on
        // the edge texel.
        let tex_opts = egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        };

        let name = format!("base_{}", self.next_tex_name);
        self.next_tex_name = self.next_tex_name.wrapping_add(1);

        // Assigning to `self.tex` drops the previous `TextureHandle`, which
        // tells egui to free the old GPU texture. Combined with the unique
        // name, that guarantees the new texture starts from a clean slate.
        self.tex = Some(ctx.load_texture(name, ci, tex_opts));
        self.img_size = Vec2::new(w as f32, h as f32);
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