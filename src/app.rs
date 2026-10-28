//! Application state and the top-level update loop.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Pos2};

use crate::geo::parse_latlon;
use crate::imagery::{fetch_imagery, Fetched, Source};
use crate::model::{EditMode, Tool, Unit};
use crate::scene::Scene;
use crate::ui;
use crate::ui::canvas::View;

const MAX_TEX: u32 = 8192;

pub struct App {
    pub view: View,
    pub scene: Scene,

    /// Which broad layer is being edited. Gates which tools are available
    /// and which entities are hoverable, draggable, and rendered as handles.
    pub mode: EditMode,

    pub tool: Tool,
    pub unit: Unit,
    pub calib_pts: Vec<Pos2>,
    pub calib_dist: f32,
    pub px_per_m: Option<f32>,

    pub secondary_press: Option<Pos2>,

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

            mode: EditMode::Road,

            tool: Tool::Road,
            unit: Unit::Feet,
            calib_pts: Vec::new(),
            calib_dist: 100.0,
            px_per_m: None,

            secondary_press: None,

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
    // Mode switching
    // ============================================================

    /// Changes the active edit mode.
    ///
    /// The tool is reset to the mode's default so that switching modes
    /// always lands you on a productive tool. Without this, going
    /// Roads → Details → Roads would strand you on Pan (since Pan is valid
    /// in every mode and thus never gets reset by the "is this tool valid"
    /// check alone), and node editing would silently stop working.
    ///
    /// Calibrate is preserved across mode switches because it's a stateful
    /// flow — you're in the middle of clicking two points — and interrupting
    /// it by browsing modes would lose your in-progress measurement.
    pub fn set_mode(&mut self, mode: EditMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;

        if self.tool != Tool::Calibrate {
            self.tool = mode.default_tool();
        }

        // Clear transient interaction state that may reference entities
        // from the previous mode.
        self.secondary_press = None;
        self.scene.hover_node = None;
        self.scene.hover_handle = None;
        self.scene.dragging = None;
        self.scene.dragging_handle = None;
        self.scene.selected_segment = None;
    }

    // ============================================================
    // Image loading & fetch
    // ============================================================

    pub fn set_base(&mut self, ctx: &egui::Context, img: &image::RgbaImage) {
        self.view.install_image(ctx, img);

        self.calib_pts.clear();
        self.px_per_m = None;
        self.scene.clear_all_paths();
        self.secondary_press = None;
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
        self.mode = EditMode::Road;
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.scene.undo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            match (self.mode, self.tool) {
                (EditMode::Road, Tool::Road) => self.scene.finish_road(),
                (EditMode::Sublayer, Tool::Pavement) => self.scene.finish_pavement(),
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

        ui::toolbar::show(self, ctx);
        ui::toolbar::show_status(self, ctx);
        ui::sidebar::show(self, ctx);
        ui::canvas::show(self, ctx);
    }
}