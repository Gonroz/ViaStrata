//! Right-hand side panel: cross-section editor and center-line picker.
//!
//! Two modes:
//!
//! * **Draft mode** — nothing is selected. The editor writes to
//!   `Scene::draft_lanes` and `Scene::center_style`, which are what the
//!   next committed road will use.
//! * **Segment mode** — a road segment is selected. The editor writes to
//!   that segment's override. A "Reset to road default" button drops the
//!   override.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::App;
use crate::model::{
    display_unit_to_meters, width_in_display_unit, CenterStyle, Direction, Lane, LaneKind,
    SegmentStyle,
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
const ITEM_PAD: f32 = 6.0;
const CROSS_SECTION_HEIGHT: f32 = 56.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("side_menu")
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(seg) = app.scene.selected_segment {
                    segment_mode(app, ui, seg);
                } else {
                    draft_mode(app, ui);
                }
            });
        });
}

// ============================================================
// Draft mode
// ============================================================

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

    // Snapshot the cross-section into locals so the borrow checker is
    // happy while we edit. We write back at the end.
    let mut lanes = app.scene.draft_lanes.clone();
    let mut center = app.scene.center_style;

    // Preview strip.
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

// ============================================================
// Segment mode
// ============================================================

fn segment_mode(app: &mut App, ui: &mut egui::Ui, seg: SegmentRef) {
    // Validate the selection first — it could be stale if the road was
    // split or the road list changed shape.
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

    // Snapshot the segment's current effective style into locals, edit,
    // then write back. Using a snapshot avoids holding a borrow on
    // `app.scene.roads` across the UI calls.
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

    // Write the edited cross-section back as an override.
    if let Some(road) = app.scene.roads.get_mut(seg.road) {
        let style = road.segment_override_mut(seg.segment);
        style.lanes = lanes;
        style.center = center;
    }
}

// ============================================================
// Shared sub-editors
// ============================================================

/// Renders the lane rows. `id_salt` distinguishes the draft editor's
/// widget IDs from the segment editor's so egui doesn't confuse them.
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

        // Apply back to the lane.
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

// Silence a false-positive warning about SegmentStyle being unused; it's
// used implicitly via type inference in the segment editor's snapshots.
#[allow(unused_imports)]
use SegmentStyle as _SegmentStyleAlias;