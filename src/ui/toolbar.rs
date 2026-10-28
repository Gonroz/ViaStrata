//! Top toolbar (three rows) and bottom status bar.

use eframe::egui::{self, Color32};

use crate::app::App;
use crate::imagery::Source;
use crate::model::{EditMode, Tool, Unit};

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        image_row(app, ctx, ui);
        location_row(app, ctx, ui);
        tool_row(app, ui);
    });
}

pub fn show_status(app: &App, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.label(&app.status);
    });
}

fn image_row(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("Open image…").clicked() {
            app.open_image(ctx);
        }
        ui.separator();

        for mode in EditMode::all() {
            let is_current = app.mode == *mode;
            if ui.selectable_label(is_current, mode.label()).clicked() && !is_current {
                app.set_mode(*mode);
            }
        }
        ui.separator();

        for &tool in app.mode.tools() {
            let label = app.mode.tool_label(tool);
            ui.selectable_value(&mut app.tool, tool, label);
        }
        ui.separator();

        ui.label("Units:");
        ui.selectable_value(&mut app.unit, Unit::Feet, "ft");
        ui.selectable_value(&mut app.unit, Unit::Meters, "m");
        ui.separator();

        match app.px_per_m {
            Some(s) => {
                let px_per_unit = s * app.unit.to_m();
                ui.label(format!("Scale: {:.3} px/{}", px_per_unit, app.unit.label()));
            }
            None => {
                ui.colored_label(Color32::from_rgb(220, 120, 60), "Scale not set");
            }
        }
    });
}

fn location_row(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("Location:");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut app.coord_text)
                .hint_text("lat, lon  (e.g. 42.8864, -78.8784)")
                .desired_width(240.0),
        );
        let enter_in_field = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        for src in [Source::Esri, Source::EsriClarity, Source::Usgs] {
            ui.selectable_value(&mut app.source, src, src.label());
        }

        let maxz = app.source.max_zoom();
        ui.label("Zoom:");
        ui.add(egui::DragValue::new(&mut app.tile_z).range(10..=maxz));
        ui.label("Radius (tiles):");
        ui.add(egui::DragValue::new(&mut app.radius).range(1..=8));

        if matches!(app.source, Source::Esri | Source::EsriClarity) {
            ui.label("Key:");
            ui.add(
                egui::TextEdit::singleline(&mut app.token)
                    .password(true)
                    .hint_text("optional")
                    .desired_width(90.0),
            );
        }

        let idle = app.rx.is_none();
        if ui
            .add_enabled(idle, egui::Button::new("Fetch satellite"))
            .clicked()
            || (enter_in_field && idle)
        {
            app.start_fetch(ctx);
        }
    });
}

fn tool_row(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| match app.tool {
        Tool::Calibrate => {
            let suffix = format!(" {}", app.unit.label());
            ui.label("Real distance between the two points:");
            ui.add(
                egui::DragValue::new(&mut app.calib_dist)
                    .speed(0.5)
                    .range(0.1..=100000.0)
                    .suffix(suffix),
            );
            let ready = app.calib_pts.len() == 2;
            if ui
                .add_enabled(ready, egui::Button::new("Apply scale"))
                .clicked()
            {
                let px = (app.calib_pts[1] - app.calib_pts[0]).length();
                let meters = app.calib_dist * app.unit.to_m();
                if px > 0.0 && meters > 0.0 {
                    app.px_per_m = Some(px / meters);
                    app.tool = app.mode.default_tool();
                    app.status =
                        "Click to add road points. Double-click or Enter to finish.".into();
                }
            }
        }
        Tool::Road => {
            ui.label("Cross-section and markings are in the side panel →");
            ui.separator();
            if ui.button("Finish (Enter)").clicked() {
                app.scene.finish_road();
            }
            if ui.button("Undo (Ctrl+Z)").clicked() {
                app.scene.undo();
            }
            if ui.button("Clear all").clicked() {
                app.scene.clear_all_paths();
            }
        }
        Tool::Pavement => {
            ui.label("Click to add corners. Click the first corner again to close the shape.");
            ui.separator();
            if ui.button("Finish (Enter)").clicked() {
                app.scene.finish_pavement();
            }
            if ui.button("Undo (Ctrl+Z)").clicked() {
                app.scene.undo();
            }
            if ui.button("Clear all").clicked() {
                app.scene.clear_all_paths();
            }
        }
        Tool::Pan => {
            ui.label("Middle-drag or WASD to pan, scroll to zoom.");
            if app.mode == EditMode::Details || app.mode == EditMode::Decorations {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("{} tools coming soon", app.mode.label()))
                        .color(Color32::from_gray(150))
                        .italics(),
                );
            }
        }
    });
}