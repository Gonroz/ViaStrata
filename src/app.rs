//! Application state and the top-level update loop.
//!
//! `App` is the one mutable root that every part of the UI borrows from. It
//! holds the camera (`View`), the node graph (`NodeGraph`), the finished
//! roads and in-progress draft, plus tool selection and fetch state.
//!
//! Higher-level *operations* that touch more than one subfield — finish a
//! road, delete a node, undo — live as methods on `App` so callers never
//! need to know which subfield an operation "belongs to." Scene *queries*
//! (find node near point, which nodes are shared, which segment is closest)
//! are also methods here, since they answer questions about App's own
//! state.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, vec2, Pos2, Vec2};

use crate::geo::parse_latlon;
use crate::imagery::{fetch_imagery, Fetched, Source};
use crate::model::{CenterStyle, NodeGraph, NodeId, Road, Tool, Unit};
use crate::ui;
use crate::ui::canvas::View;

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
    // ----- Camera ----------------------------------------------------
    /// Camera state: which image is loaded, and where/how it's displayed.
    pub view: View,

    // ----- Tools + scale ---------------------------------------------
    pub tool: Tool,
    pub unit: Unit,
    /// Points the user has clicked with the Calibrate tool. Cleared on
    /// successful apply.
    pub calib_pts: Vec<Pos2>,
    /// Real-world distance between the two calibration points, *expressed in
    /// the currently selected unit*. Converted with `Unit::to_m` at apply.
    pub calib_dist: f32,
    /// Screen pixels per meter once known. `None` means the Road tool refuses
    /// to place points — without a scale there's no way to size them.
    pub px_per_m: Option<f32>,

    // ----- Scene: nodes + roads --------------------------------------
    /// Node graph: every distinct point roads are built from.
    pub graph: NodeGraph,
    /// Finished roads, in paint order (later entries draw over earlier ones).
    pub roads: Vec<Road>,
    /// The road currently being drawn, as a sequence of node IDs. Empty when
    /// idle. Enter, right-click, double-click, or clicking an existing node
    /// commits it.
    pub current: Vec<NodeId>,
    /// Lane count for the *next* committed road.
    pub lanes: u32,
    /// Lane width in meters for the *next* committed road.
    pub lane_m: f32,
    /// Center-line style for the *next* committed road.
    pub center_style: CenterStyle,

    // ----- Interaction -----------------------------------------------
    /// The node the user is currently dragging, or `None`. Latched on
    /// primary press while the cursor is over a node; cleared on release.
    pub dragging: Option<NodeId>,
    /// The node under the cursor this frame. Recomputed by the canvas every
    /// frame and used for handle highlighting and the drag/delete hit-test.
    pub hover_node: Option<NodeId>,
    /// Where the secondary mouse button was last pressed, in screen pixels.
    /// Used by `input::handle_secondary_click` to synthesize its own
    /// right-click detection, because egui's built-in click detection on a
    /// `click_and_drag` response requires *zero* movement between press and
    /// release, which tiny hand jitter violates. Cleared on release.
    pub secondary_press: Option<Pos2>,

    // ----- Satellite fetch -------------------------------------------
    pub coord_text: String,
    pub source: Source,
    /// Zoom level for the fetch; clamped to `source.max_zoom()` in
    /// `start_fetch`.
    pub tile_z: u32,
    /// Radius in tiles *around* the target tile. The final crop is
    /// `2 * radius` tiles wide — the extra tile is crop margin.
    pub radius: u32,
    /// Optional API token for sources that support one (currently only Esri).
    pub token: String,
    /// Channel receiving an in-flight fetch. `None` when idle.
    pub rx: Option<Receiver<Result<Fetched, String>>>,
    /// Attribution string shown in the corner of the canvas.
    pub attribution: String,
    /// Set on construction; cleared on the first `update`. When true, the
    /// app fires an initial fetch of the default coordinates as soon as an
    /// egui `Context` is available (which is not the case during `Default`).
    pub initial_fetch_queued: bool,

    pub status: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            view: View::default(),

            tool: Tool::Pan,
            unit: Unit::Feet,
            calib_pts: Vec::new(),
            calib_dist: 100.0,
            px_per_m: None,

            graph: NodeGraph::new(),
            roads: Vec::new(),
            current: Vec::new(),
            lanes: 2,
            // 11 ft is a common US lane width; store the metric truth once.
            lane_m: 11.0 * 0.3048,
            center_style: CenterStyle::YellowSolid,

            dragging: None,
            hover_node: None,
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
    // Scene queries
    // ============================================================

    /// Resolves a road's node list to positions for drawing. Skips nodes
    /// that no longer exist — this shouldn't happen given the orphan-cleanup
    /// discipline below, but skipping is cheap insurance against panicking
    /// mid-frame.
    pub fn road_positions(&self, road: &Road) -> Vec<Pos2> {
        road.nodes
            .iter()
            .filter_map(|id| self.graph.pos(*id))
            .collect()
    }

    /// Same as `road_positions` but for the in-progress draft.
    pub fn draft_positions(&self) -> Vec<Pos2> {
        self.current
            .iter()
            .filter_map(|id| self.graph.pos(*id))
            .collect()
    }

    /// Finds the nearest node to `img_pt` within `radius` image pixels.
    ///
    /// The draft's nodes are checked first so that, on a tie, a point the
    /// user just placed wins over one buried under an older road.
    pub fn find_node(&self, img_pt: Pos2, radius: f32) -> Option<NodeId> {
        let r2 = radius * radius;
        let mut best: Option<(NodeId, f32)> = None;
        let mut seen: HashSet<NodeId> = HashSet::new();

        let mut consider = |id: NodeId| {
            if !seen.insert(id) {
                return;
            }
            let Some(p) = self.graph.pos(id) else { return };
            let d2 = (p - img_pt).length_sq();
            if d2 <= r2 {
                match best {
                    Some((_, bd2)) if bd2 <= d2 => {}
                    _ => best = Some((id, d2)),
                }
            }
        };

        for &id in &self.current {
            consider(id);
        }
        for road in &self.roads {
            for &id in &road.nodes {
                consider(id);
            }
        }
        best.map(|(id, _)| id)
    }

    /// Returns the set of nodes referenced by two or more roads (or by the
    /// draft and any road). These are the nodes that get a joint fill and a
    /// ring around their handle.
    pub fn shared_nodes(&self) -> HashSet<NodeId> {
        let mut counts: HashMap<NodeId, usize> = HashMap::new();
        for road in &self.roads {
            for &id in &road.nodes {
                *counts.entry(id).or_insert(0) += 1;
            }
        }
        for &id in &self.current {
            *counts.entry(id).or_insert(0) += 1;
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c >= 2)
            .map(|(id, _)| id)
            .collect()
    }

    /// Finds the closest committed-road segment to `pos`, returning
    /// `(road_index, segment_start_index, projected_point)`.
    ///
    /// The segment spans node indices `segment_start_index` and
    /// `segment_start_index + 1`. The caller inserts the new node at
    /// `segment_start_index + 1` to place it *between* the two existing
    /// nodes, preserving the road's shape.
    ///
    /// Uses the raw control points, not the tessellated spline. The spline
    /// passes through every control point, so the approximation is close
    /// enough for a click-to-insert gesture.
    pub fn closest_road_segment(
        &self,
        pos: Pos2,
        max_dist: f32,
    ) -> Option<(usize, usize, Pos2)> {
        let max_sq = max_dist * max_dist;
        let mut best: Option<(usize, usize, Pos2, f32)> = None;

        for (ri, road) in self.roads.iter().enumerate() {
            let pts = self.road_positions(road);
            for i in 0..pts.len().saturating_sub(1) {
                let a = pts[i];
                let b = pts[i + 1];
                let proj = project_point_to_segment(pos, a, b);
                let d2 = (proj - pos).length_sq();
                if d2 <= max_sq {
                    match best {
                        Some((_, _, _, bd2)) if bd2 <= d2 => {}
                        _ => best = Some((ri, i, proj, d2)),
                    }
                }
            }
        }
        best.map(|(ri, i, p, _)| (ri, i, p))
    }

    // ============================================================
    // Graph mutations
    // ============================================================

    /// Allocates a new node at `pos`. Thin wrapper so callers don't need to
    /// reach into `app.graph` directly.
    pub fn add_node(&mut self, pos: Pos2) -> NodeId {
        self.graph.add(pos)
    }

    /// Moves a node. Every road referencing it moves with it — that's the
    /// whole point of the graph.
    pub fn move_node(&mut self, id: NodeId, delta: Vec2) {
        self.graph.move_by(id, delta);
    }

    /// Deletes a node entirely — from every road, from the draft, and from
    /// the graph.
    ///
    /// When the node sits in the *middle* of a road, the road is split into
    /// two pieces either side of the deletion. Pieces with fewer than two
    /// nodes are dropped, which is why a road that was `[A, B, C, D]` and
    /// loses `B` becomes `[C, D]` — the leading `[A]` is not a road.
    ///
    /// Nodes that become orphaned by the split are cleaned up at the end.
    pub fn delete_node(&mut self, id: NodeId) {
        let mut new_roads: Vec<Road> = Vec::with_capacity(self.roads.len());

        for road in self.roads.drain(..) {
            if !road.nodes.contains(&id) {
                new_roads.push(road);
                continue;
            }

            // A closed loop can reference the same node twice; handle every
            // occurrence in one pass by slicing around each position.
            let positions: Vec<usize> = road
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| **n == id)
                .map(|(i, _)| i)
                .collect();

            let mut start = 0usize;
            for &pos in &positions {
                let piece = road.nodes[start..pos].to_vec();
                if piece.len() >= 2 {
                    new_roads.push(Road {
                        nodes: piece,
                        lanes: road.lanes,
                        lane_m: road.lane_m,
                        center: road.center,
                    });
                }
                start = pos + 1;
            }
            let tail = road.nodes[start..].to_vec();
            if tail.len() >= 2 {
                new_roads.push(Road {
                    nodes: tail,
                    lanes: road.lanes,
                    lane_m: road.lane_m,
                    center: road.center,
                });
            }
        }

        self.roads = new_roads;
        self.current.retain(|n| *n != id);
        self.graph.remove(id);
        self.gc_orphan_nodes();

        if self.hover_node == Some(id) {
            self.hover_node = None;
        }
        if self.dragging == Some(id) {
            self.dragging = None;
        }
    }

    /// Removes a node if nothing references it anymore.
    fn remove_if_orphan(&mut self, id: NodeId) {
        let still_used = self.roads.iter().any(|r| r.nodes.contains(&id))
            || self.current.contains(&id);
        if !still_used {
            self.graph.remove(id);
        }
    }

    /// Removes every node that no longer has a reference from a road or the
    /// draft. Safety sweep after destructive edits.
    fn gc_orphan_nodes(&mut self) {
        let mut used: HashSet<NodeId> = HashSet::new();
        for road in &self.roads {
            for &id in &road.nodes {
                used.insert(id);
            }
        }
        for &id in &self.current {
            used.insert(id);
        }
        self.graph.gc(|id| used.contains(&id));
    }

    // ============================================================
    // Draft lifecycle
    // ============================================================

    /// Throws away the in-progress draft, cleaning up any nodes that were
    /// only referenced by it.
    pub fn discard_draft(&mut self) {
        let draft: Vec<NodeId> = std::mem::take(&mut self.current);
        for id in draft {
            self.remove_if_orphan(id);
        }
        self.dragging = None;
        self.hover_node = None;
    }

    /// Commits the draft into `roads` if it has at least two *distinct*
    /// consecutive nodes. Otherwise the draft is discarded.
    ///
    /// Consecutive duplicates are removed here rather than prevented at
    /// placement time so that a stray click on the node you just placed
    /// doesn't create a zero-length segment.
    pub fn finish_road(&mut self) {
        let mut deduped: Vec<NodeId> = Vec::with_capacity(self.current.len());
        for &id in &self.current {
            if deduped.last() != Some(&id) {
                deduped.push(id);
            }
        }
        self.current.clear();

        if deduped.len() >= 2 {
            self.roads.push(Road {
                nodes: deduped,
                lanes: self.lanes,
                lane_m: self.lane_m,
                center: self.center_style,
            });
        } else {
            // Single-point or empty draft: discard, and clean up any nodes
            // that just lost their last reference.
            for id in deduped {
                self.remove_if_orphan(id);
            }
        }
        self.dragging = None;
        self.hover_node = None;
    }

    /// Called when the user primary-clicks in the canvas with the Road tool
    /// active.
    ///
    /// - Empty draft + snapped to a node: start the draft there.
    /// - Empty draft + no snap: start the draft at a new node.
    /// - Draft in progress + snapped to a node: append (unless it's already
    ///   the last point) and commit. That's the "terminate at a node"
    ///   gesture, and it's also how closed loops get made.
    /// - Draft in progress + no snap: append a new node.
    pub fn road_click(&mut self, img_pos: Pos2, snap: Option<NodeId>) {
        match (self.current.is_empty(), snap) {
            (true, Some(id)) => {
                self.current.push(id);
            }
            (true, None) => {
                let id = self.add_node(img_pos);
                self.current.push(id);
            }
            (false, Some(id)) => {
                if self.current.last() != Some(&id) {
                    self.current.push(id);
                }
                self.finish_road();
            }
            (false, None) => {
                let id = self.add_node(img_pos);
                self.current.push(id);
            }
        }
    }

    /// Ctrl+Z. Pops the last draft point first, then the last committed road.
    /// Nodes left orphaned by either action are removed.
    pub fn undo(&mut self) {
        if let Some(id) = self.current.pop() {
            self.remove_if_orphan(id);
        } else if let Some(road) = self.roads.pop() {
            for id in road.nodes {
                self.remove_if_orphan(id);
            }
        }
        self.dragging = None;
        self.hover_node = None;
    }

    /// Clears every road, the draft, and every node. Used by the "Clear all"
    /// button; encapsulation is intentional so the button can't half-clear
    /// the scene.
    pub fn clear_all_roads(&mut self) {
        self.roads.clear();
        self.current.clear();
        self.graph.clear();
        self.dragging = None;
        self.hover_node = None;
    }

    // ============================================================
    // Image loading & fetch
    // ============================================================

    /// Installs an image as the canvas background and resets the drawing
    /// state. Roads and nodes are cleared along with the scale, because
    /// their pixel coordinates refer to the old image.
    pub fn set_base(&mut self, ctx: &egui::Context, img: &image::RgbaImage) {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());

        // `Linear` minification with a mipmap chain is what keeps the
        // fit-to-window view crisp — without it, egui takes a single
        // averaged tap and the result is both soft and aliased.
        let tex_opts = egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        };

        let tex = ctx.load_texture("base", ci, tex_opts);
        self.view.reset_for_new_image(tex, vec2(w as f32, h as f32));

        self.calib_pts.clear();
        self.px_per_m = None;
        self.roads.clear();
        self.current.clear();
        self.graph.clear();
        self.dragging = None;
        self.hover_node = None;
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
            "Loaded z{} imagery, {}×{} px. Scale {:.3} {}/px. Click to add points; click an existing node to end there; right-click or double-click to finish.",
            f.z,
            f.img.width(),
            f.img.height(),
            unit_mpp,
            self.unit.label()
        );
    }

    // ============================================================
    // Keyboard shortcuts
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
            self.undo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.finish_road();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.discard_draft();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // One-shot initial fetch, deferred until egui hands us a Context.
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

/// Projects `p` onto the line segment `a..b`, clamped to the segment's
/// endpoints so the result is always on the segment.
///
/// Small geometry helper, kept free rather than a method because it doesn't
/// touch any `App` state and could be reused by other modules later.
fn project_point_to_segment(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}