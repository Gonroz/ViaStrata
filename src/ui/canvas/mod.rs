//! The central canvas: view, input routing, and rendering.

pub mod view;

mod input;
mod render;

pub use view::View;

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

use crate::app::App;

/// Image <-> screen transform for one frame. Snapshotted after camera
/// updates so input and render agree on the numbers the user actually saw.
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

    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.rect_min + self.offset + p.to_vec2() * self.zoom
    }

    pub fn to_image(&self, s: Pos2) -> Pos2 {
        ((s - self.rect_min - self.offset) / self.zoom).to_pos2()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

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

        view::apply_camera(app, &resp, rect);

        let xform = Xform::new(rect, app.view.offset, app.view.zoom);

        input::handle(app, &resp, &xform, ctx);
        render::draw(app, &painter, &resp, &xform, rect);
    });
}