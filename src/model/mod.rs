//! Domain types. Everything that describes *what a scene is*, with no
//! dependency on egui widgets or the render pipeline.

mod graph;
mod path;
mod pavement;
mod road;
mod tool;
mod unit;

pub use graph::{NodeGraph, NodeId};
pub use path::{
    recompute_auto_handles, tessellate, AnchorRef, HandleRef, HandleSide, PathAnchor, PathOwner,
};
pub use pavement::Pavement;
pub use road::{CenterStyle, Road};
pub use tool::Tool;
pub use unit::Unit;