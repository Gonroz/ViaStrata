//! Application state and the top-level update loop.
//!
//! `App` is the outer shell: camera, tool selection, calibration, image
//! loading, tile fetching. Everything about the *design being drawn* —
//! nodes, roads, pavements, drafts, and the interaction state that goes
//! with them — lives in `App::scene`, a `Scene` struct.
//!
//! Operations that touch both layers (loading a new image resets the
//! scene; a fetch installs an image and sets the scale) live here, on
//! `App`. Everything else is a method on whichever layer owns the data
//! it manipulates.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Pos2};

use crate::geo::parse_latlon;
use crate::imagery::{fetch_imagery, Fetched, Source};
use crate::model::{Tool, Unit};
use crate::scene::Scene;
use crate::ui;
use crate::ui::canvas::View;

/// Upper bound on the loaded image's longest side.
const MAX_TEX: u32 = 8192;

/// All application state.
pub struct App {
    // Camera
    pub view: View,

    // The design
    pub scene: Scene,

    // Tools + scale
    pub tool: Tool,
    pub unit: Unit,
    pub calib_pts: Vec<Pos2>,
    pub calib_dist: f32,
    pub px_per_m: Option<f32>,

    /// Where the secondary mouse button was last pressed. See
    /// `input::handle_secondary_click` for why we track this ourselves.
    /// Lives on `App` rather than `Scene` because it's general input state,
    /// not design state.
    pub secondary_press: Option<Pos2>,

    // Satellite fetch
    pub coord_text: String,
    pub source: Source,
    pub tile_z: u32,
    pub radius: u32,
    pub token: String,
    pub rx: Option<Receiver<Result<Fetched, String>>>,
    pub attribution: String,
    pub initial_fetch_queued: bool,

    pub status: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            view: View::default(),
            scene: Scene::default(),

            tool: Tool::Pan,
            unit: Unit::Feet,
            calib_pts: Vec::new(),
            calib_dist: 100.0,
            px_per_m: None,

            secondary_press: None,

            // Prefilled so the app is useful the moment it opens.
            coord_text: "42.922367, -78.867269".into(),
            source: Source::Esri,
            tile_z: 19,
            radius: 3,
            token: String::new(),
            rx: None,
            attribution: String::new(),
            initial_fetch_queued: true,

            status: "Loading default imagery…".into(),
        }
    }
}

impl App {
    // ============================================================
    // Image loading & fetch
    // ============================================================

    /// Installs an image as the canvas background and resets the drawing
    /// state. Roads, pavements, and nodes are cleared along with the scale,
    /// because their pixel coordinates refer to the old image.
    ///
    /// The actual texture upload is delegated to `View::install_image`,
    /// which also schedules a one-shot fit-to-window.
    pub fn set_base(&mut self, ctx: &egui::Context, img: &image::RgbaImage) {
        self.view.install_image(ctx, img);

        self.calib_pts.clear();
        self.px_per_m = None;
        self.scene.clear_all_paths();
        self.secondary_press = None;
        // Re-set by `apply_fetched` when we came from a fetch; open-image
        // mode intentionally leaves this blank.
        self.attribution.clear();
    }

    pub fn open_image(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
            .pick_file()
        else {
            return;
        };
        match image::open(&path) {
            Ok(mut img) => {
                if img.width().max(img.height()) > MAX_TEX {
                    img = img.resize(MAX_TEX, MAX_TEX, image::imageops::FilterType::Triangle);
                }
                let rgba = img.to_rgba8();
                self.set_base(ctx, &rgba);
                self.tool = Tool::Calibrate;
                self.status = "Step 1: click two points a known distance apart.".into();
            }
            Err(e) => self.status = format!("Could not open image: {e}"),
        }
    }

    pub fn start_fetch(&mut self, ctx: &egui::Context) {
        let Some((lat, lon)) = parse_latlon(&self.coord_text) else {
            self.status = "Couldn't parse coordinates. Use: 42.8864, -78.8784".into();
            return;
        };
        if !(-85.0..=85.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            self.status = "Coordinates out of range (lat -85..85, lon -180..180).".into();
            return;
        }
        let src = self.source;
        self.tile_z = self.tile_z.min(src.max_zoom());
        let (z, r) = (self.tile_z, self.radius.max(1));
        let token = self.token.clone();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let res = fetch_imagery(src, &token, lat, lon, z, r);
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        let side = 2 * r + 1;
        self.status = format!("Fetching {} tiles…", side * side);
    }

    fn poll_fetch(&mut self, ctx: &egui::Context) {
        let polled = self.rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(res)) => {
                self.rx = None;
                match res {
                    Ok(f) => self.apply_fetched(ctx, f),
                    Err(e) => self.status = format!("Fetch failed: {e}"),
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.rx = None;
                self.status = "Fetch failed: worker stopped unexpectedly.".into();
            }
            _ => {}
        }
    }

    fn apply_fetched(&mut self, ctx: &egui::Context, f: Fetched) {
        self.set_base(ctx, &f.img);
        self.px_per_m = Some(f.px_per_m);
        self.tool = Tool::Road;
        self.attribution = self.source.attribution().to_string();
        let unit_mpp = f.m_per_px / self.unit.to_m() as f64;
        self.status = format!(
            "Loaded z{} imagery, {}×{} px. Scale {:.3} {}/px. Click to draw; right-click to finish.",
            f.z,
            f.img.width(),
            f.img.height(),
            unit_mpp,
            self.unit.label()
        );
    }

    // ============================================================
    // Keyboard
    // ============================================================

    /// Keyboard shortcuts, ignored while typing in a text field.
    ///
    /// Node deletion is handled in the canvas instead of here, because it
    /// needs the hover state that only the canvas computes.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.scene.undo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            match self.tool {
                Tool::Road => self.scene.finish_road(),
                Tool::Pavement => self.scene.finish_pavement(),
                _ => {}
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.scene.discard_drafts();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.initial_fetch_queued {
            self.initial_fetch_queued = false;
            if !self.coord_text.trim().is_empty() {
                self.start_fetch(ctx);
            }
        }

        self.poll_fetch(ctx);
        self.handle_shortcuts(ctx);
        ui::canvas::view::apply_wasd(self, ctx);

        // Panel order matters in egui: top/bottom panels first, then side
        // panels, then the central panel takes whatever is left.
        ui::toolbar::show(self, ctx);
        ui::toolbar::show_status(self, ctx);
        ui::sidebar::show(self, ctx);
        ui::canvas::show(self, ctx);
    }
}