//! Right-hand side panel with visual pickers for road markings.
//!
//! The center-line style picker used to be a `ComboBox` in the toolbar, but
//! a small drawn preview communicates each style far more clearly than its
//! name does ("Solid yellow" vs "Double yellow" is easy to mix up in text).
//! The panel is a natural place to grow additional visual pickers later
//! (lane count, lane widths, road types) without crowding the top bar.
//!
//! Each picker item is a single `allocate_exact_size` call with
//! `Sense::click()`. The whole row is the hit target, not just a thumbnail,
//! because small targets are annoying to hit with a mouse. Selection state
//! is drawn by the same code that reacts to the click, so there is no risk
//! of the two disagreeing.

use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2};

use crate::app::App;
use crate::model::CenterStyle;
use crate::ui::paint::paint_road_preview;

/// Every style in the picker, in display order. Kept as a `const` slice so
/// adding a variant to `CenterStyle` needs only one more entry here.
const CENTER_STYLES: &[CenterStyle] = &[
    CenterStyle::None,
    CenterStyle::YellowSolid,
    CenterStyle::YellowDouble,
    CenterStyle::YellowDashed,
];

/// Height of each picker row in logical pixels. Enough for a small label
/// plus a legible road thumbnail underneath.
const ITEM_HEIGHT: f32 = 56.0;

/// Padding inside each row, between the row's border and its contents.
const ITEM_PAD: f32 = 6.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("side_menu")
        // Fixed width; the thumbnails don't benefit from extra space and a
        // resizable panel would fight the central canvas for room.
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

/// One clickable row: a label at the top, a road preview underneath, and a
/// background/highlight that reflects selection and hover.
fn center_style_item(app: &mut App, ui: &mut egui::Ui, style: CenterStyle) {
    let selected = app.center_style == style;

    // The whole row is the widget. `allocate_exact_size` gives us both the
    // rect we paint into and the response we react to, so the visuals and
    // the hit target cannot drift apart.
    let size = Vec2::new(ui.available_width(), ITEM_HEIGHT);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

    // Background colour. Uses three distinct states rather than two so
    // hovering an unselected item is visible without competing with the
    // selected item's highlight.
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

    // Label at the top of the row.
    let label_pos = rect.left_top() + Vec2::new(ITEM_PAD + 2.0, ITEM_PAD);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        style.label(),
        FontId::proportional(12.0),
        Color32::from_gray(220),
    );

    // Preview fills the rest of the row below the label. We reserve a fixed
    // strip for the label rather than measuring it, which keeps every row's
    // thumbnail at the same vertical position regardless of label width.
    let preview_top = label_pos.y + 16.0;
    let preview_rect = Rect::from_min_max(
        egui::pos2(rect.left() + ITEM_PAD, preview_top),
        egui::pos2(rect.right() - ITEM_PAD, rect.bottom() - ITEM_PAD),
    );
    paint_road_preview(ui.painter(), preview_rect, style);

    if resp.clicked() {
        app.center_style = style;
    }
}