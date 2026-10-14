//! Domain types. Everything that describes *what a road is*, with no
//! dependency on egui widgets or the render pipeline.

mod graph;
mod road;
mod tool;
mod unit;

pub use graph::{NodeGraph, NodeId};
pub use road::{CenterStyle, Road};
pub use tool::Tool;
pub use unit::Unit;