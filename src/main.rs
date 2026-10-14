//! Via Strata: draw roads to real-world scale over satellite imagery.

mod app;
mod geo;
mod imagery;
mod model;
mod scene;
mod spline;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1300.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Via Strata",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::default()))),
    )
}