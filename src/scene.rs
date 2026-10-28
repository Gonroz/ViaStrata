//! The scene: everything about the design being drawn, independent of
//! camera, tool selection, and fetch state.

use std::collections::{HashMap, HashSet};

use eframe::egui::{Pos2, Vec2};

use crate::model::{
    default_two_way, recompute_auto_handles, AnchorRef, CenterStyle, HandleRef, HandleSide, Lane,
    Material, NodeGraph, NodeId, PathAnchor, PathOwner, Pavement, Road,
};

/// Identifies a specific road segment: the span between anchors
/// `segment` and `segment + 1` of road `road`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentRef {
    pub road: usize,
    pub segment: usize,
}

pub struct Scene {
    pub graph: NodeGraph,
    pub roads: Vec<Road>,
    pub pavements: Vec<Pavement>,
    pub current_road: Vec<PathAnchor>,
    pub current_pavement: Vec<PathAnchor>,

    /// Cross-section for the *next* committed road.
    pub draft_lanes: Vec<Lane>,
    /// Center-line style for the next committed road.
    pub center_style: CenterStyle,
    /// Material for the next committed pavement shape.
    pub draft_material: Material,

    /// Currently selected road segment, if any.
    pub selected_segment: Option<SegmentRef>,

    pub dragging: Option<NodeId>,
    pub hover_node: Option<NodeId>,
    pub dragging_handle: Option<HandleRef>,
    pub hover_handle: Option<HandleRef>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            graph: NodeGraph::new(),
            roads: Vec::new(),
            pavements: Vec::new(),
            current_road: Vec::new(),
            current_pavement: Vec::new(),

            draft_lanes: default_two_way(),
            center_style: CenterStyle::YellowSolid,
            draft_material: Material::Asphalt,

            selected_segment: None,

            dragging: None,
            hover_node: None,
            dragging_handle: None,
            hover_handle: None,
        }
    }
}

impl Scene {
    // ============================================================
    // Segment selection
    // ============================================================

    pub fn select_segment(&mut self, road: usize, segment: usize) {
        if let Some(r) = self.roads.get(road) {
            if segment < r.num_segments() {
                self.selected_segment = Some(SegmentRef { road, segment });
            }
        }
    }

    pub fn deselect_segment(&mut self) {
        if let Some(seg) = self.selected_segment.take() {
            if let Some(road) = self.roads.get_mut(seg.road) {
                if road.segment_equals_default(seg.segment) {
                    road.segment_overrides.remove(&seg.segment);
                }
            }
        }
    }

    // ============================================================
    // Queries
    // ============================================================

    pub fn road_positions(&self, road: &Road) -> Vec<Pos2> {
        road.anchors
            .iter()
            .filter_map(|a| self.graph.pos(a.node))
            .collect()
    }

    pub fn anchor_position(&self, r: AnchorRef) -> Option<Pos2> {
        let anchor = match r.owner {
            PathOwner::Road(i) => self.roads.get(i)?.anchors.get(r.index)?,
            PathOwner::Pavement(i) => self.pavements.get(i)?.anchors.get(r.index)?,
        };
        self.graph.pos(anchor.node)
    }

    pub fn anchor(&self, r: AnchorRef) -> Option<&PathAnchor> {
        match r.owner {
            PathOwner::Road(i) => self.roads.get(i)?.anchors.get(r.index),
            PathOwner::Pavement(i) => self.pavements.get(i)?.anchors.get(r.index),
        }
    }

    fn anchor_mut(&mut self, r: AnchorRef) -> Option<&mut PathAnchor> {
        match r.owner {
            PathOwner::Road(i) => self.roads.get_mut(i)?.anchors.get_mut(r.index),
            PathOwner::Pavement(i) => self.pavements.get_mut(i)?.anchors.get_mut(r.index),
        }
    }

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

        for a in &self.current_road {
            consider(a.node);
        }
        for a in &self.current_pavement {
            consider(a.node);
        }
        for road in &self.roads {
            for a in &road.anchors {
                consider(a.node);
            }
        }
        for pa in &self.pavements {
            for a in &pa.anchors {
                consider(a.node);
            }
        }
        best.map(|(id, _)| id)
    }

    pub fn find_handle(&self, img_pt: Pos2, radius: f32) -> Option<HandleRef> {
        let r2 = radius * radius;
        for (i, road) in self.roads.iter().enumerate() {
            for (ai, a) in road.anchors.iter().enumerate() {
                if let Some(h) = handle_hit(&self.graph, a, PathOwner::Road(i), ai, img_pt, r2) {
                    return Some(h);
                }
            }
        }
        for (i, pa) in self.pavements.iter().enumerate() {
            for (ai, a) in pa.anchors.iter().enumerate() {
                if let Some(h) =
                    handle_hit(&self.graph, a, PathOwner::Pavement(i), ai, img_pt, r2)
                {
                    return Some(h);
                }
            }
        }
        None
    }

    pub fn shared_nodes(&self) -> HashSet<NodeId> {
        let mut counts: HashMap<NodeId, usize> = HashMap::new();
        for road in &self.roads {
            for a in &road.anchors {
                *counts.entry(a.node).or_insert(0) += 1;
            }
        }
        for pa in &self.pavements {
            for a in &pa.anchors {
                *counts.entry(a.node).or_insert(0) += 1;
            }
        }
        for a in &self.current_road {
            *counts.entry(a.node).or_insert(0) += 1;
        }
        for a in &self.current_pavement {
            *counts.entry(a.node).or_insert(0) += 1;
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c >= 2)
            .map(|(id, _)| id)
            .collect()
    }

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

    pub fn segment_at(&self, pos: Pos2, px_per_m: f32) -> Option<SegmentRef> {
        let mut best: Option<(SegmentRef, f32)> = None;
        for (ri, road) in self.roads.iter().enumerate() {
            let pts = self.road_positions(road);
            for i in 0..pts.len().saturating_sub(1) {
                let a = pts[i];
                let b = pts[i + 1];
                let proj = project_point_to_segment(pos, a, b);
                let d = (proj - pos).length();
                let half_w = road.segment_width_m(i) * px_per_m / 2.0;
                if d <= half_w {
                    let slack = half_w - d;
                    match best {
                        Some((_, bs)) if bs >= slack => {}
                        _ => best = Some((SegmentRef { road: ri, segment: i }, slack)),
                    }
                }
            }
        }
        best.map(|(s, _)| s)
    }

    // ============================================================
    // Graph mutations
    // ============================================================

    pub fn add_node(&mut self, pos: Pos2) -> NodeId {
        self.graph.add(pos)
    }

    pub fn move_node(&mut self, id: NodeId, delta: Vec2) {
        if self.graph.move_by(id, delta) {
            self.refresh_auto_handles();
        }
    }

    pub fn move_handle(&mut self, href: HandleRef, delta: Vec2, break_tangent: bool) {
        let current = match self.anchor(href.anchor_ref()) {
            Some(a) => *a,
            None => return,
        };
        let mut new_in = current.in_handle;
        let mut new_out = current.out_handle;

        match href.side {
            HandleSide::In => {
                new_in += delta;
                if !break_tangent {
                    new_out = -new_in;
                }
            }
            HandleSide::Out => {
                new_out += delta;
                if !break_tangent {
                    new_in = -new_out;
                }
            }
        }

        if let Some(a) = self.anchor_mut(href.anchor_ref()) {
            a.auto = false;
            a.in_handle = new_in;
            a.out_handle = new_out;
        }
    }

    pub fn pull_out_handles(&mut self, node: NodeId, delta: Vec2) {
        for road in &mut self.roads {
            for a in &mut road.anchors {
                if a.node == node {
                    a.auto = false;
                    a.out_handle += delta;
                    a.in_handle = -a.out_handle;
                }
            }
        }
        for pa in &mut self.pavements {
            for a in &mut pa.anchors {
                if a.node == node {
                    a.auto = false;
                    a.out_handle += delta;
                    a.in_handle = -a.out_handle;
                }
            }
        }
    }

    pub fn delete_node(&mut self, id: NodeId) {
        self.roads = split_roads_on_node(std::mem::take(&mut self.roads), id);
        self.pavements = split_pavements_on_node(std::mem::take(&mut self.pavements), id);

        self.current_road.retain(|a| a.node != id);
        self.current_pavement.retain(|a| a.node != id);
        self.graph.remove(id);
        self.gc_orphan_nodes();

        if self.hover_node == Some(id) {
            self.hover_node = None;
        }
        if self.dragging == Some(id) {
            self.dragging = None;
        }
        self.hover_handle = None;
        self.dragging_handle = None;
        self.selected_segment = None;
    }

    fn remove_if_orphan(&mut self, id: NodeId) {
        let still_used = self
            .roads
            .iter()
            .any(|r| r.anchors.iter().any(|a| a.node == id))
            || self
                .pavements
                .iter()
                .any(|p| p.anchors.iter().any(|a| a.node == id))
            || self.current_road.iter().any(|a| a.node == id)
            || self.current_pavement.iter().any(|a| a.node == id);
        if !still_used {
            self.graph.remove(id);
        }
    }

    fn gc_orphan_nodes(&mut self) {
        let mut used: HashSet<NodeId> = HashSet::new();
        for road in &self.roads {
            for a in &road.anchors {
                used.insert(a.node);
            }
        }
        for pa in &self.pavements {
            for a in &pa.anchors {
                used.insert(a.node);
            }
        }
        for a in &self.current_road {
            used.insert(a.node);
        }
        for a in &self.current_pavement {
            used.insert(a.node);
        }
        self.graph.gc(|id| used.contains(&id));
    }

    pub fn refresh_auto_handles(&mut self) {
        let graph = &self.graph;
        for road in &mut self.roads {
            recompute_auto_handles(&mut road.anchors, graph, false);
        }
        for pa in &mut self.pavements {
            recompute_auto_handles(&mut pa.anchors, graph, true);
        }
        recompute_auto_handles(&mut self.current_road, graph, false);
        recompute_auto_handles(&mut self.current_pavement, graph, false);
    }

    // ============================================================
    // Draft lifecycle
    // ============================================================

    pub fn discard_drafts(&mut self) {
        let road: Vec<PathAnchor> = std::mem::take(&mut self.current_road);
        let pa: Vec<PathAnchor> = std::mem::take(&mut self.current_pavement);
        for a in road.iter().chain(pa.iter()) {
            self.remove_if_orphan(a.node);
        }
        self.dragging = None;
        self.hover_node = None;
        self.dragging_handle = None;
        self.hover_handle = None;
    }

    pub fn finish_road(&mut self) {
        let deduped = dedup_anchors(&self.current_road);
        self.current_road.clear();

        if deduped.len() >= 2 && !self.draft_lanes.is_empty() {
            self.roads.push(Road {
                anchors: deduped,
                lanes: self.draft_lanes.clone(),
                center: self.center_style,
                segment_overrides: HashMap::new(),
            });
            self.refresh_auto_handles();
        } else {
            for a in deduped {
                self.remove_if_orphan(a.node);
            }
        }
        self.dragging = None;
        self.hover_node = None;
        self.dragging_handle = None;
        self.hover_handle = None;
    }

    pub fn finish_pavement(&mut self) {
        let deduped = dedup_anchors(&self.current_pavement);
        self.current_pavement.clear();

        if deduped.len() >= 3 {
            self.pavements.push(Pavement {
                anchors: deduped,
                material: self.draft_material,
            });
            self.refresh_auto_handles();
        } else {
            for a in deduped {
                self.remove_if_orphan(a.node);
            }
        }
        self.dragging = None;
        self.hover_node = None;
        self.dragging_handle = None;
        self.hover_handle = None;
    }

    pub fn road_click(&mut self, img_pos: Pos2, snap: Option<NodeId>) {
        match (self.current_road.is_empty(), snap) {
            (true, Some(id)) => {
                self.current_road.push(PathAnchor::new(id));
            }
            (true, None) => {
                let id = self.add_node(img_pos);
                self.current_road.push(PathAnchor::new(id));
            }
            (false, Some(id)) => {
                if self.current_road.last().map(|a| a.node) != Some(id) {
                    self.current_road.push(PathAnchor::new(id));
                }
                self.finish_road();
            }
            (false, None) => {
                let id = self.add_node(img_pos);
                self.current_road.push(PathAnchor::new(id));
            }
        }
        self.refresh_auto_handles();
    }

    pub fn pavement_click(&mut self, img_pos: Pos2, snap: Option<NodeId>) {
        match (self.current_pavement.is_empty(), snap) {
            (true, Some(id)) => {
                self.current_pavement.push(PathAnchor::new_corner(id));
            }
            (true, None) => {
                let id = self.add_node(img_pos);
                self.current_pavement.push(PathAnchor::new_corner(id));
            }
            (false, Some(id)) => {
                let is_first = self.current_pavement.first().map(|a| a.node) == Some(id);
                let is_last = self.current_pavement.last().map(|a| a.node) == Some(id);

                if is_first && self.current_pavement.len() >= 3 {
                    self.finish_pavement();
                } else if !is_last {
                    self.current_pavement.push(PathAnchor::new_corner(id));
                }
            }
            (false, None) => {
                let id = self.add_node(img_pos);
                self.current_pavement.push(PathAnchor::new_corner(id));
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some(a) = self.current_road.pop() {
            self.remove_if_orphan(a.node);
        } else if let Some(a) = self.current_pavement.pop() {
            self.remove_if_orphan(a.node);
        } else if let Some(road) = self.roads.pop() {
            for a in road.anchors {
                self.remove_if_orphan(a.node);
            }
        } else if let Some(pa) = self.pavements.pop() {
            for a in pa.anchors {
                self.remove_if_orphan(a.node);
            }
        }
        self.dragging = None;
        self.hover_node = None;
        self.dragging_handle = None;
        self.hover_handle = None;
        self.selected_segment = None;
    }

    pub fn clear_all_paths(&mut self) {
        self.roads.clear();
        self.pavements.clear();
        self.current_road.clear();
        self.current_pavement.clear();
        self.graph.clear();
        self.dragging = None;
        self.hover_node = None;
        self.dragging_handle = None;
        self.hover_handle = None;
        self.selected_segment = None;
    }
}

// ============================================================
// Free functions
// ============================================================

fn project_point_to_segment(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

fn dedup_anchors(anchors: &[PathAnchor]) -> Vec<PathAnchor> {
    let mut out: Vec<PathAnchor> = Vec::with_capacity(anchors.len());
    for &a in anchors {
        if out.last().map(|x: &PathAnchor| x.node) != Some(a.node) {
            out.push(a);
        }
    }
    out
}

fn handle_hit(
    graph: &NodeGraph,
    anchor: &PathAnchor,
    owner: PathOwner,
    anchor_index: usize,
    img_pt: Pos2,
    r2: f32,
) -> Option<HandleRef> {
    let pos = graph.pos(anchor.node)?;
    if anchor.out_handle.length_sq() > 1e-6 {
        if (pos + anchor.out_handle - img_pt).length_sq() <= r2 {
            return Some(HandleRef {
                owner,
                anchor_index,
                side: HandleSide::Out,
            });
        }
    }
    if anchor.in_handle.length_sq() > 1e-6 {
        if (pos + anchor.in_handle - img_pt).length_sq() <= r2 {
            return Some(HandleRef {
                owner,
                anchor_index,
                side: HandleSide::In,
            });
        }
    }
    None
}

fn split_roads_on_node(roads: Vec<Road>, id: NodeId) -> Vec<Road> {
    let mut out = Vec::with_capacity(roads.len());
    for road in roads {
        if !road.anchors.iter().any(|a| a.node == id) {
            out.push(road);
            continue;
        }
        let positions: Vec<usize> = road
            .anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.node == id)
            .map(|(i, _)| i)
            .collect();

        let mut start = 0usize;
        for &pos in &positions {
            let piece = road.anchors[start..pos].to_vec();
            if piece.len() >= 2 {
                out.push(Road {
                    anchors: piece,
                    lanes: road.lanes.clone(),
                    center: road.center,
                    segment_overrides: HashMap::new(),
                });
            }
            start = pos + 1;
        }
        let tail = road.anchors[start..].to_vec();
        if tail.len() >= 2 {
            out.push(Road {
                anchors: tail,
                lanes: road.lanes.clone(),
                center: road.center,
                segment_overrides: HashMap::new(),
            });
        }
    }
    out
}

fn split_pavements_on_node(pavements: Vec<Pavement>, id: NodeId) -> Vec<Pavement> {
    let mut out = Vec::with_capacity(pavements.len());
    for pa in pavements {
        if !pa.anchors.iter().any(|a| a.node == id) {
            out.push(pa);
            continue;
        }
        let positions: Vec<usize> = pa
            .anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.node == id)
            .map(|(i, _)| i)
            .collect();

        let mut start = 0usize;
        for &pos in &positions {
            let piece = pa.anchors[start..pos].to_vec();
            if piece.len() >= 3 {
                out.push(Pavement {
                    anchors: piece,
                    material: pa.material,
                });
            }
            start = pos + 1;
        }
        let tail = pa.anchors[start..].to_vec();
        if tail.len() >= 3 {
            out.push(Pavement {
                anchors: tail,
                material: pa.material,
            });
        }
    }
    out
}