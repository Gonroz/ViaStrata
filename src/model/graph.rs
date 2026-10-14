//! The node graph: shared points that roads are built from.

use std::collections::HashMap;

use eframe::egui::{Pos2, Vec2};

/// Stable identifier for a point in the node graph.
///
/// Node IDs are allocated once and never reused, so a reference to
/// `NodeId(n)` is always either valid or deliberately removed — it never
/// silently points at a different location. The `u32` is opaque; don't do
/// arithmetic on it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u32);

/// All nodes in the app, keyed by stable ID.
///
/// A node is a *shared* point. Two roads that reference the same `NodeId`
/// always occupy the same position, and dragging that node moves both. This
/// is the entire reason for the graph: roads store `Vec<NodeId>`, not
/// `Vec<Pos2>`.
///
/// The graph knows *nothing* about roads. All "which nodes are still in
/// use" logic lives on `App`, which passes a predicate to `gc`. That keeps
/// the dependency arrow pointing one way: scene -> graph, never graph ->
/// scene.
#[derive(Default)]
pub struct NodeGraph {
    nodes: HashMap<NodeId, Pos2>,
    next_id: u32,
}

impl NodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a new node at `pos` and returns its ID. The ID is unique
    /// for the graph's lifetime, even after deletions.
    pub fn add(&mut self, pos: Pos2) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, pos);
        id
    }

    /// Looks up a node's position. Returns `None` if the ID is stale.
    pub fn pos(&self, id: NodeId) -> Option<Pos2> {
        self.nodes.get(&id).copied()
    }

    /// Moves a node by `delta`. Returns `true` if the ID was valid.
    pub fn move_by(&mut self, id: NodeId, delta: Vec2) -> bool {
        match self.nodes.get_mut(&id) {
            Some(p) => {
                *p += delta;
                true
            }
            None => false,
        }
    }

    /// Removes a node. Returns the position it occupied, if any.
    pub fn remove(&mut self, id: NodeId) -> Option<Pos2> {
        self.nodes.remove(&id)
    }

    /// Removes every node in the graph.
    ///
    /// `next_id` is deliberately *not* reset: IDs must never be reused, even
    /// across a clear, in case any stale reference is still alive somewhere.
    /// The cost is a few unused numbers, which is nothing.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Removes every node for which `is_used` returns `false`.
    ///
    /// The caller decides what "used" means — usually "referenced by some
    /// road or the current draft." Keeping the predicate external lets the
    /// graph stay ignorant of roads.
    pub fn gc(&mut self, is_used: impl Fn(NodeId) -> bool) {
        self.nodes.retain(|id, _| is_used(*id));
    }
}