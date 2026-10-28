//! Right-hand side panel. Contents depend on the active edit mode:
//!
//! * **Sublayer** — material picker (Asphalt / Concrete / Sidewalk / Grass).
//! * **Roads** — cross-section editor and center-line picker, switching
//!   between draft mode (nothing selected) and segment mode (a segment is
//!   selected).
//! * **Details** / **Decorations** — placeholder, since those modes have
//!   no tools yet.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::App;
use crate::model::{
    display_unit_to_meters, width_in_display_unit, CenterStyle, Direction, EditMode, Lane,
    LaneKind, Material,
};
use crate::scene::SegmentRef;
use crate::ui::paint::{paint_cross_section, paint_road_preview};

const CENTER_STYLES: &[CenterStyle] = &[
    CenterStyle::None,
    CenterStyle::YellowSolid,
    CenterStyle::YellowDouble,
    CenterStyle::YellowDashed,
];

const CENTER_ITEM_HEIGHT: f32 = 56.0;
const MATERIAL_ITEM_HEIGHT: f32 = 48.0;
const ITEM_PAD: f32 = 6.0;
const CROSS_SECTION_HEIGHT: f32 = 56.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("side_menu")
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match app.mode {
                EditMode::Sublayer => sublayer_sidebar(app, ui),
                EditMode::Road => road_sidebar(app, ui),
                EditMode::Details => placeholder_sidebar(
                    ui,
                    "Road details",
                    "Crosswalks, stop lines, turn arrows, and above-road shapes will live here.",
                ),
                EditMode::Decorations => placeholder_sidebar(
                    ui,
                    "Decorations",
                    "Trees, benches, shelters, and buildings will live here.",
                ),
            });
        });
}

// ============================================================
// Sublayer mode
// ============================================================

fn sublayer_sidebar(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.heading("Base materials");
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new("Fill shapes with the selected material")
            .color(Color32::from_gray(170))
            .size(12.0),
    );
    ui.add_space(8.0);

    for &mat in Material::all() {
        material_item(app, ui, mat);
        ui.add_space(6.0);
    }
}

fn material_item(app: &mut App, ui: &mut egui::Ui, mat: Material) {
    let selected = app.scene.draft_material == mat;
    let size = Vec2::new(ui.available_width(), MATERIAL_ITEM_HEIGHT);
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

    // Color swatch on the left.
    let swatch_size = 32.0;
    let swatch_rect = Rect::from_min_size(
        Pos2::new(rect.left() + ITEM_PAD + 2.0, rect.center().y - swatch_size / 2.0),
        Vec2::splat(swatch_size),
    );
    ui.painter().rect(swatch_rect, 3.0, mat.color(), Stroke::NONE);

    // Label to the right of the swatch.
    ui.painter().text(
        Pos2::new(swatch_rect.right() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        mat.label(),
        FontId::proportional(14.0),
        Color32::from_gray(225),
    );

    if resp.clicked() {
        app.scene.draft_material = mat;
    }
}

// ============================================================
// Road mode
// ============================================================

fn road_sidebar(app: &mut App, ui: &mut egui::Ui) {
    if let Some(seg) = app.scene.selected_segment {
        segment_mode(app, ui, seg);
    } else {
        draft_mode(app, ui);
    }
}

fn draft_mode(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.heading("Road cross-section");
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new("Applies to the next road you draw")
            .color(Color32::from_gray(170))
            .size(12.0),
    );
    ui.add_space(6.0);

    let mut lanes = app.scene.draft_lanes.clone();
    let mut center = app.scene.center_style;

    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, CROSS_SECTION_HEIGHT), Sense::hover());
    paint_cross_section(ui.painter(), rect, &lanes, center);
    ui.add_space(8.0);

    lane_editor(ui, &mut lanes, app.unit, "draft");
    ui.add_space(6.0);
    if ui.button("+ Add travel lane").clicked() {
        lanes.push(Lane::travel_forward());
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    center_line_section(ui, &mut center);

    app.scene.draft_lanes = lanes;
    app.scene.center_style = center;
}

fn segment_mode(app: &mut App, ui: &mut egui::Ui, seg: SegmentRef) {
    let Some(road) = app.scene.roads.get(seg.road) else {
        app.scene.selected_segment = None;
        return;
    };
    if seg.segment >= road.num_segments() {
        app.scene.selected_segment = None;
        return;
    }
    let total_segments = road.num_segments();

    ui.add_space(8.0);
    ui.heading("Selected segment");
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format!(
            "Road {} — segment {} of {}",
            seg.road + 1,
            seg.segment + 1,
            total_segments
        ))
        .color(Color32::from_gray(170))
        .size(12.0),
    );
    ui.add_space(6.0);

    let (mut lanes, mut center) = {
        let style = app.scene.roads[seg.road].style_for_segment(seg.segment);
        (style.lanes.clone(), style.center)
    };

    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, CROSS_SECTION_HEIGHT), Sense::hover());
    paint_cross_section(ui.painter(), rect, &lanes, center);
    ui.add_space(8.0);

    lane_editor(ui, &mut lanes, app.unit, "segment");
    ui.add_space(6.0);
    if ui.button("+ Add travel lane").clicked() {
        lanes.push(Lane::travel_forward());
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    center_line_section(ui, &mut center);

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    if ui.button("Reset to road default").clicked() {
        if let Some(road) = app.scene.roads.get_mut(seg.road) {
            road.reset_segment(seg.segment);
        }
        return;
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Right-click on the canvas when done.")
            .color(Color32::from_gray(150))
            .size(11.0),
    );

    if let Some(road) = app.scene.roads.get_mut(seg.road) {
        let style = road.segment_override_mut(seg.segment);
        style.lanes = lanes;
        style.center = center;
    }
}

// ============================================================
// Placeholder modes
// ============================================================

fn placeholder_sidebar(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.add_space(8.0);
    ui.heading(title);
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(description)
            .color(Color32::from_gray(170))
            .size(12.0),
    );
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new("Coming soon.")
            .color(Color32::from_gray(150))
            .italics(),
    );
}

// ============================================================
// Shared sub-editors
// ============================================================

fn lane_editor(ui: &mut egui::Ui, lanes: &mut Vec<Lane>, unit: crate::model::Unit, id_salt: &str) {
    let count = lanes.len();
    let mut to_remove: Option<usize> = None;

    for i in 0..count {
        if i >= lanes.len() {
            break;
        }
        let lane = lanes[i];
        let mut kind = lane.kind;
        let mut direction = lane.direction;
        let mut width_display = width_in_display_unit(&lane, unit);

        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt((id_salt, "kind", i))
                .width(88.0)
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for &k in LaneKind::all() {
                        ui.selectable_value(&mut kind, k, k.label());
                    }
                });

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

            ui.add(
                egui::DragValue::new(&mut width_display)
                    .speed(0.05)
                    .range(0.5..=10.0)
                    .suffix(format!(" {}", unit.label()))
                    .fixed_decimals(2),
            );

            let can_delete = lanes.len() > 1;
            if ui
                .add_enabled(can_delete, egui::Button::new("✕").small())
                .clicked()
            {
                to_remove = Some(i);
            }
        });

        let slot = &mut lanes[i];
        if slot.kind != kind {
            slot.kind = kind;
            slot.width_m = kind.default_width_m();
        } else {
            slot.width_m = display_unit_to_meters(width_display, unit);
        }
        slot.direction = direction;
    }

    if let Some(i) = to_remove {
        if lanes.len() > 1 {
            lanes.remove(i);
        }
    }
}

fn center_line_section(ui: &mut egui::Ui, center: &mut CenterStyle) {
    ui.heading("Road markings");
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new("Center line style")
            .color(Color32::from_gray(170))
            .size(12.0),
    );
    ui.add_space(6.0);

    for &style in CENTER_STYLES {
        center_style_item(ui, center, style);
        ui.add_space(6.0);
    }
}

fn center_style_item(ui: &mut egui::Ui, current: &mut CenterStyle, style: CenterStyle) {
    let selected = *current == style;

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
        *current = style;
    }
}