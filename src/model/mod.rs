//! Domain types. Everything that describes *what a scene is*, with no
//! dependency on egui widgets or the render pipeline.

mod graph;
mod lane;
mod path;
mod pavement;
mod road;
mod tool;
mod unit;

pub use graph::{NodeGraph, NodeId};
pub use lane::{default_two_way, display_unit_to_meters, total_width_m, width_in_display_unit};
pub use lane::{Direction, Lane, LaneKind};
pub use path::{
    recompute_auto_handles, tessellate, AnchorRef, HandleRef, HandleSide, PathAnchor, PathOwner,
};
pub use pavement::Pavement;
pub use road::{CenterStyle, Road};
pub use tool::Tool;
pub use unit::Unit;