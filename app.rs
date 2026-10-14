//! Application state and the top-level update loop.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, vec2, Pos2, TextureHandle, Vec2};

use crate::geo::parse_latlon;
use crate::imagery::{fetch_imagery, Fetched, Source};
use crate::model::{CenterStyle, Road, Tool, Unit, VertexRef};
use crate::ui;

/// Upper bound on the loaded image's longest side.
///
/// GPU texture limits vary, but 8192 is the lowest common denominator across
/// integrated GPUs and mobile-class hardware. Larger images are downscaled
/// on load. Because calibration happens *after* downscaling, pixel
/// coordinates stay consistent within the app's lifetime — but not across
/// sessions where a different limit applies.
const MAX_TEX: u32 = 8192;

/// All application state. `Default` starts in a neutral "empty" state with
/// the Pan tool active and no imagery loaded.
pub struct App {
    // ----- Base image + view -----------------------------------------
    /// GPU texture of the loaded base image, or `None` before any is set.
    pub tex: Option<TextureHandle>,
    /// Size of the base image in *image* pixels (not screen pixels).
    pub img_size: Vec2,
    /// Screen pixels per image pixel.
    pub zoom: f32,
    /// Screen offset of the image origin, relative to the canvas top-left.
    pub offset: Vec2,
    /// Set when a new image is loaded; the canvas consumes it on the next
    /// frame to fit the image to the viewport exactly once.
    pub fit_pending: bool,
    /// Set on construction; cleared on the first `update`. When true, the
    /// app fires an initial fetch of the default coordinates as soon as an
    /// egui `Context` is available (which is not the case during `Default`).
    pub initial_fetch_queued: bool,

    // ----- Tools + scale ---------------------------------------------
    pub tool: Tool,
    pub unit: Unit,
    /// Points the user has clicked with the Calibrate tool. Cleared on
    /// successful apply.
    pub calib_pts: Vec<Pos2>,
    /// Real-world distance between the two calibration points, *expressed in
    /// the currently selected unit*. Converted with `Unit::to_m` at apply
    /// time.
    pub calib_dist: f32,
    /// Screen pixels per meter once known. `None` means the Road tool refuses
    /// to place points — without a scale there's no way to size them.
    pub px_per_m: Option<f32>,

    // ----- Roads -----------------------------------------------------
    /// Finished roads, in paint order (later entries draw over earlier ones).
    pub roads: Vec<Road>,
    /// The road currently being drawn, in image-pixel coordinates. Empty
    /// when idle. Enter or double-click commits it into `roads`.
    pub current: Vec<Pos2>,
    pub lanes: u32,
    pub lane_m: f32,
    /// Center-line style applied to the next committed road. This is a
    /// *drafting* setting — it's captured into `Road::center` when the road
    /// is finished, so existing roads keep their own style even if the user
    /// then changes this for the next one.
    pub center_style: CenterStyle,

    // ----- Vertex editing --------------------------------------------
    /// The vertex the user is currently dragging. Set when the primary
    /// button goes down on a vertex, cleared on release.
    pub dragging: Option<VertexRef>,
    /// The vertex under the cursor this frame, if any. Recomputed by the
    /// canvas every frame. Used both to highlight handles and to decide
    /// whether a click should place a *new* point or grab an existing one.
    pub hover_vertex: Option<VertexRef>,

    // ----- Satellite fetch -------------------------------------------
    pub coord_text: String,
    pub source: Source,
    /// Zoom level for the fetch; clamped to `source.max_zoom()` in
    /// `start_fetch`.
    pub tile_z: u32,
    /// Radius in tiles *around* the target tile. Note that the final crop is
    /// only `2 * radius` tiles wide — the extra tile is margin the crop is
    /// carved out of, not part of the output. So radius 2 yields a
    /// 1024×1024 image, not 1280×1280.
    pub radius: u32,
    /// Optional API token for sources that support one (currently only Esri).
    pub token: String,
    /// Channel receiving an in-flight fetch. `None` when idle.
    pub rx: Option<Receiver<Result<Fetched, String>>>,
    /// Attribution string shown in the corner of the canvas.
    pub attribution: String,

    pub status: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            tex: None,
            img_size: Vec2::ZERO,
            zoom: 1.0,
            offset: Vec2::ZERO,
            fit_pending: false,
            initial_fetch_queued: true,

            tool: Tool::Pan,
            unit: Unit::Feet,
            calib_pts: Vec::new(),
            calib_dist: 100.0,
            px_per_m: None,

            roads: Vec::new(),
            current: Vec::new(),
            lanes: 2,
            // 11 ft is a common US lane width; store the metric truth once.
            lane_m: 11.0 * 0.3048,
            // Solid yellow is the safest default for a two-way residential
            // street; the user can switch per-road from the side panel.
            center_style: CenterStyle::YellowSolid,

            dragging: None,
            hover_vertex: None,

            // Prefilled so the app is useful the moment it opens. These
            // coordinates are an intersection in the Buffalo, NY area.
            coord_text: "42.922367, -78.867269".into(),
            source: Source::Esri,
            // z19 is Esri's practical ceiling: higher levels return upscaled
            // tiles rather than new detail. The app allows up to z20 (see
            // `Source::max_zoom`) so you can experiment.
            tile_z: 19,
            // Radius 3 -> a 1536x1536 crop (~336m of ground at this lat).
            // That is 1.5x the linear size of radius 2, and 2.25x the area.
            radius: 3,
            token: String::new(),
            rx: None,
            attribution: String::new(),

            status: "Loading default imagery…".into(),
        }
    }
}

impl App {
    /// Installs an image as the canvas background and resets the drawing state.
    ///
    /// Resetting is deliberate: the pixel coordinates of any existing roads
    /// refer to the *old* image and would be meaningless against a new one.
    /// The scale (`px_per_m`) is cleared too — a new image needs calibration
    /// or a fresh fetch.
    pub fn set_base(&mut self, ctx: &egui::Context, img: &image::RgbaImage) {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());

        // Texture filtering choices — this is where "why does it look
        // blurry?" gets answered.
        //
        // * `Linear` magnification keeps the imagery smooth when the user
        //   zooms past 1:1. `Nearest` would look like a checkerboard of
        //   hard pixels, which is wrong for photographic imagery.
        // * `Linear` minification *plus a mipmap chain* is the important
        //   one. When the whole image is fit into a smaller viewport, egui
        //   samples it down. Without mipmaps it takes a single averaged tap
        //   and the result is both soft and aliased. With mipmaps it reads
        //   from a prefiltered level, so the fit view stays crisp.
        // * `ClampToEdge` prevents a visible border seam when the image is
        //   displayed at exactly 1:1 and samples land on the edge texel.
        let tex_opts = egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        };

        self.tex = Some(ctx.load_texture("base", ci, tex_opts));
        self.img_size = vec2(w as f32, h as f32);
        self.fit_pending = true;
        self.calib_pts.clear();
        self.px_per_m = None;
        self.roads.clear();
        self.current.clear();
        self.dragging = None;
        self.hover_vertex = None;
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
                // Very large images exceed GPU texture limits; downscale if
                // needed. Calibration happens *after* loading, so the user's
                // two-point measurement is against the downscaled image and
                // stays consistent for the session.
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
        // Clamp here rather than in the widget so that switching sources
        // (which changes max_zoom) can never leave us with an illegal value.
        self.tile_z = self.tile_z.min(src.max_zoom());
        let (z, r) = (self.tile_z, self.radius.max(1));
        let token = self.token.clone();

        // The worker runs on a plain OS thread and hands the result back via
        // a one-shot channel. We kick a repaint so the UI wakes up when it
        // lands, instead of relying on the user to move the mouse.
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

    /// Non-blocking pickup of a finished download, if any.
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
        // A fetched image comes with its scale baked in — no calibration
        // needed, so jump straight to drawing.
        self.px_per_m = Some(f.px_per_m);
        self.tool = Tool::Road;
        self.attribution = self.source.attribution().to_string();
        let unit_mpp = f.m_per_px / self.unit.to_m() as f64;
        self.status = format!(
            "Loaded z{} imagery, {}×{} px. Scale set automatically: {:.3} {}/px. Click to draw roads.",
            f.z,
            f.img.width(),
            f.img.height(),
            unit_mpp,
            self.unit.label()
        );
    }

    /// Commits the in-progress road into `roads`, if it has at least two
    /// points. A single-point road is not a road; it's a misclick, so it's
    /// discarded silently.
    pub fn finish_road(&mut self) {
        if self.current.len() >= 2 {
            self.roads.push(Road {
                pts: std::mem::take(&mut self.current),
                lanes: self.lanes,
                lane_m: self.lane_m,
                center: self.center_style,
            });
        } else {
            self.current.clear();
        }
        // The vertex we were hovering belonged to `current`, which just went
        // away (or moved into `roads` under a new identity). Reset both to
        // avoid dangling references.
        self.dragging = None;
        self.hover_vertex = None;
    }

    /// Ctrl+Z. Pops the last point from the in-progress road first, then the
    /// last finished road. That matches the mental model: you're undoing
    /// your most recent action, not the entire road you're mid-way through.
    pub fn undo(&mut self) {
        if !self.current.is_empty() {
            self.current.pop();
        } else {
            self.roads.pop();
        }
        self.dragging = None;
        self.hover_vertex = None;
    }

    /// Keyboard shortcuts, ignored while typing in a text field.
    ///
    /// Vertex deletion is handled in the canvas instead of here, because it
    /// needs the hover state that only the canvas computes.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.finish_road();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.current.clear();
            self.dragging = None;
            self.hover_vertex = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // One-shot initial fetch. `Default::default()` runs before egui
        // hands us a Context, so we defer to the first real update call.
        // After firing once, `initial_fetch_queued` is cleared so that a
        // user who later clears the coordinates and reopens the app does
        // not get a surprise re-fetch.
        if self.initial_fetch_queued {
            self.initial_fetch_queued = false;
            if !self.coord_text.trim().is_empty() {
                self.start_fetch(ctx);
            }
        }

        self.poll_fetch(ctx);
        self.handle_shortcuts(ctx);

        // Panel order matters in egui. Top/bottom panels first, then side
        // panels, and the central panel last — otherwise an earlier panel
        // claims space that a later one needs. The central panel takes
        // whatever is left over after the others have taken their cuts.
        ui::toolbar::show(self, ctx);
        ui::toolbar::show_status(self, ctx);
        ui::sidebar::show(self, ctx);
        ui::canvas::show(self, ctx);
    }
}