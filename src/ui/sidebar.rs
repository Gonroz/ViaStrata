//! Right-hand side panel with visual pickers for road markings.

use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2};

use crate::app::App;
use crate::model::CenterStyle;
use crate::ui::paint::paint_road_preview;

const CENTER_STYLES: &[CenterStyle] = &[
    CenterStyle::None,
    CenterStyle::YellowSolid,
    CenterStyle::YellowDouble,
    CenterStyle::YellowDashed,
];

const ITEM_HEIGHT: f32 = 56.0;
const ITEM_PAD: f32 = 6.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("side_menu")
        .resizable(false)
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Road markings");
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Center line style")
                    .color(Color32::from_gray(170))
                    .size(12.0),
            );
            ui.add_space(6.0);

            for &style in CENTER_STYLES {
                center_style_item(app, ui, style);
                ui.add_space(6.0);
            }
        });
}

fn center_style_item(app: &mut App, ui: &mut egui::Ui, style: CenterStyle) {
    let selected = app.scene.center_style == style;

    let size = Vec2::new(ui.available_width(), ITEM_HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

    let bg = if selected {
        Color32::from_rgb(54, 82, 124)
    } else if resp.hovered() {
        Color32::from_rgb(50, 50, 56)
    } else {
        Color32::from_rgb(38, 38, 42)
    };
    let border = if selected {
        Stroke::new(1.5_f32, Color32::from_rgb(120, 170, 230))
    } else {
        Stroke::NONE
    };
    ui.painter().rect(rect, 4.0, bg, border);

    let label_pos = rect.left_top() + Vec2::new(ITEM_PAD + 2.0, ITEM_PAD);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        style.label(),
        FontId::proportional(12.0),
        Color32::from_gray(220),
    );

    let preview_top = label_pos.y + 16.0;
    let preview_rect = Rect::from_min_max(
        egui::pos2(rect.left() + ITEM_PAD, preview_top),
        egui::pos2(rect.right() - ITEM_PAD, rect.bottom() - ITEM_PAD),
    );
    paint_road_preview(ui.painter(), preview_rect, style);

    if resp.clicked() {
        app.scene.center_style = style;
    }
}