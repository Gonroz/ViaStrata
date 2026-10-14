//! The central canvas: view, input routing, and rendering.
//!
//! Split into three concerns:
//!
//! * `view`   — camera state (fit/zoom/pan/WASD), stored in `App::view`
//! * `input`  — mouse and keyboard actions that mutate the scene
//! * `render` — drawing the scene with the current camera
//!
//! This module orchestrates them: it allocates the panel, applies the
//! camera, builds a coordinate transform, then hands off to `input` and
//! `render`. All three submodules work off the same `Xform`, snapshotted
//! once per frame, so camera changes and scene changes never disagree about
//! where the cursor is.
//!
//! `view` is public so `App::update` can call `view::apply_wasd` directly —
//! WASD needs to run every frame regardless of whether the canvas has hover
//! or focus, so it can't be invoked from inside `show`. `input` and `render`
//! are private: they're implementation details of `show`, and nothing
//! outside this module should be routing input or drawing the scene.

pub mod view;

mod input;
mod render;

pub use view::View;

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

use crate::app::App;

/// Image <-> screen coordinate transform for one frame.
///
/// Snapshotted by value at the start of the frame so that if `input` mutates
/// the view mid-frame (zoom-under-cursor, pan), `render` still draws using
/// the numbers the user actually saw.
#[derive(Clone, Copy)]
pub struct Xform {
    rect_min: Pos2,
    offset: Vec2,
    zoom: f32,
}

impl Xform {
    pub fn new(rect: Rect, offset: Vec2, zoom: f32) -> Self {
        Self {
            rect_min: rect.min,
            offset,
            zoom,
        }
    }

    /// Image pixels -> screen pixels.
    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.rect_min + self.offset + p.to_vec2() * self.zoom
    }

    /// Screen pixels -> image pixels.
    pub fn to_image(&self, s: Pos2) -> Pos2 {
        ((s - self.rect_min - self.offset) / self.zoom).to_pos2()
    }

    /// Current zoom factor (screen px per image px).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Converts a screen-space length to image space.
    pub fn screen_dist_to_image(&self, px: f32) -> f32 {
        px / self.zoom
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;

        if app.view.tex.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Enter coordinates above, or open an image",
                egui::FontId::proportional(20.0),
                Color32::GRAY,
            );
            return;
        }

        // Apply camera changes for this frame: fit-on-load, zoom-under-
        // cursor, drag-pan.
        view::apply_camera(app, &resp, rect);

        // Snapshot the transform *after* camera changes, so input and
        // render both work off the same numbers this frame.
        let xform = Xform::new(rect, app.view.offset, app.view.zoom);

        // Route input. Mutates `app` (graph, roads, draft, tool state).
        input::handle(app, &resp, &xform, ctx);

        // Draw the scene using the same transform.
        render::draw(app, &painter, &resp, &xform, rect);
    });
}