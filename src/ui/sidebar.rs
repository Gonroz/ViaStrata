//! Right-hand side panel: road cross-section editor and center-line style
//! picker.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::App;
use crate::model::{
    display_unit_to_meters, width_in_display_unit, CenterStyle, Direction, Lane, LaneKind,
};
use crate::ui::paint::{paint_cross_section, paint_road_preview};

/// Every center style in the picker, in display order.
const CENTER_STYLES: &[CenterStyle] = &[
    CenterStyle::None,
    CenterStyle::YellowSolid,
    CenterStyle::YellowDouble,
    CenterStyle::YellowDashed,
];

const CENTER_ITEM_HEIGHT: f32 = 56.0;
const ITEM_PAD: f32 = 6.0;

/// Height of the cross-section preview strip in the sidebar.
const CROSS_SECTION_HEIGHT: f32 = 56.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("side_menu")
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                cross_section_section(app, ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                center_line_section(app, ui);
            });
        });
}

// ============================================================
// Cross-section editor
// ============================================================

fn cross_section_section(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.heading("Road cross-section");
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new("Lanes, left to right")
            .color(Color32::from_gray(170))
            .size(12.0),
    );
    ui.add_space(6.0);

    // Preview strip.
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, CROSS_SECTION_HEIGHT), Sense::hover());
    paint_cross_section(
        ui.painter(),
        rect,
        &app.scene.draft_lanes,
        app.scene.center_style,
    );
    ui.add_space(8.0);

    // Lane rows. We snapshot the count so a mid-loop removal doesn't shift
    // the indices out from under us. The bounds check inside `lane_row`
    // handles the case where a lane was just deleted.
    let count = app.scene.draft_lanes.len();
    for i in 0..count {
        lane_row(app, ui, i);
        ui.add_space(2.0);
    }

    ui.add_space(6.0);
    if ui.button("+ Add travel lane").clicked() {
        app.scene.draft_lanes.push(Lane::travel_forward());
    }
}

/// One row in the cross-section editor: kind picker, direction toggle,
/// width in the current unit, delete button.
///
/// Rows are laid out horizontally with fixed allocations so the columns
/// line up across rows regardless of how long a kind's label is.
fn lane_row(app: &mut App, ui: &mut egui::Ui, index: usize) {
    if index >= app.scene.draft_lanes.len() {
        return;
    }
    let unit = app.unit;
    let lane = app.scene.draft_lanes[index];

    let mut kind = lane.kind;
    let mut direction = lane.direction;
    let mut width_display = width_in_display_unit(&lane, unit);
    let mut remove = false;

    ui.horizontal(|ui| {
        // Kind picker.
        egui::ComboBox::from_id_salt(("lane_kind", index))
            .width(88.0)
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for &k in LaneKind::all() {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });

        // Direction toggle. Disabled for parking and median; those keep a
        // hidden `direction` value that the renderer ignores.
        if kind.uses_direction() {
            let is_fwd = direction == Direction::Forward;
            if ui.selectable_label(is_fwd, "→").clicked() {
                direction = Direction::Forward;
            }
            if ui.selectable_label(!is_fwd, "←").clicked() {
                direction = Direction::Backward;
            }
        } else {
            ui.add_space(30.0);
        }

        // Width in the current display unit.
        ui.add(
            egui::DragValue::new(&mut width_display)
                .speed(0.05)
                .range(0.5..=10.0)
                .suffix(format!(" {}", unit.label()))
                .fixed_decimals(2),
        );

        // Delete. Disabled when it would leave the road with zero lanes.
        let can_delete = app.scene.draft_lanes.len() > 1;
        if ui
            .add_enabled(can_delete, egui::Button::new("✕").small())
            .clicked()
        {
            remove = true;
        }
    });

    // Apply edits back to the scene.
    let new_width_m = display_unit_to_meters(width_display, unit);
    if let Some(slot) = app.scene.draft_lanes.get_mut(index) {
        // If the kind changed, snap the width to the new kind's default —
        // a bike lane left at 3.35m because it used to be a travel lane is
        // worse than a small surprise.
        if slot.kind != kind {
            slot.kind = kind;
            slot.width_m = kind.default_width_m();
        } else {
            slot.width_m = new_width_m;
        }
        slot.direction = direction;
    }

    if remove && app.scene.draft_lanes.len() > 1 {
        app.scene.draft_lanes.remove(index);
    }
}

// ============================================================
// Center-line style picker
// ============================================================

fn center_line_section(app: &mut App, ui: &mut egui::Ui) {
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
}

fn center_style_item(app: &mut App, ui: &mut egui::Ui, style: CenterStyle) {
    let selected = app.scene.center_style == style;

    let size = Vec2::new(ui.available_width(), CENTER_ITEM_HEIGHT);
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
        Align2::LEFT_TOP,
        style.label(),
        FontId::proportional(12.0),
        Color32::from_gray(220),
    );

    let preview_top = label_pos.y + 16.0;
    let preview_rect = Rect::from_min_max(
        Pos2::new(rect.left() + ITEM_PAD, preview_top),
        Pos2::new(rect.right() - ITEM_PAD, rect.bottom() - ITEM_PAD),
    );
    paint_road_preview(ui.painter(), preview_rect, style);

    if resp.clicked() {
        app.scene.center_style = style;
    }
}