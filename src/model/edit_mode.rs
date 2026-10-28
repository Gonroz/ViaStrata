//! Edit modes: which broad layer of the design is being edited.
//!
//! Modes set the context for tool selection. Each mode exposes a specific
//! set of tools, and switching modes may switch the active tool if the
//! current one isn't valid for the new mode.
//!
//! The ordering here matches the design doc's layer order: sublayer → roads
//! → details → decorations. Broad to specific.

use super::tool::Tool;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditMode {
    /// The base material layer: asphalt, concrete, grass, water. Broad
    /// shapes that cover the satellite image so roads have a surface to
    /// sit on.
    Sublayer,
    /// The roads themselves: parametrized cross-sections with lanes and
    /// markings.
    Road,
    /// Road drawings and above-road shapes: crosswalks, stop lines, turn
    /// arrows, pedestrian islands, curb jutouts.
    Details,
    /// Scene-setting decorations: trees, benches, bus shelters, buildings.
    /// Purely visual; doesn't affect the road geometry.
    Decorations,
}

impl EditMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sublayer => "Sublayer",
            Self::Road => "Roads",
            Self::Details => "Details",
            Self::Decorations => "Decorations",
        }
    }

    pub fn all() -> &'static [EditMode] {
        &[Self::Sublayer, Self::Road, Self::Details, Self::Decorations]
    }

    /// Whether a tool can be selected while in this mode.
    ///
    /// Pan and Calibrate are universally available — Pan is needed for
    /// navigation regardless of what you're editing, and Calibrate is about
    /// setting up the image, not about the design layer.
    ///
    /// Pavement is available in both Sublayer (as the primary asphalt
    /// material) and Road (as a filled element inside a road scene). In
    /// Road mode it retains the current behavior; in Sublayer mode it's
    /// labeled "Asphalt" to match the material framing.
    pub fn allows_tool(self, tool: Tool) -> bool {
        match tool {
            Tool::Pan | Tool::Calibrate => true,
            Tool::Road => matches!(self, Self::Road),
            Tool::Pavement => matches!(self, Self::Sublayer | Self::Road),
        }
    }

    /// The tools available in this mode, in display order.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Self::Sublayer => &[Tool::Pan, Tool::Calibrate, Tool::Pavement],
            Self::Road => &[Tool::Pan, Tool::Calibrate, Tool::Road, Tool::Pavement],
            Self::Details | Self::Decorations => &[Tool::Pan, Tool::Calibrate],
        }
    }

    /// The tool that gets selected when this mode is entered with an
    /// invalid tool active. Chosen so that switching modes lands you on
    /// something useful rather than requiring a second click.
    pub fn default_tool(self) -> Tool {
        match self {
            Self::Sublayer => Tool::Pavement,
            Self::Road => Tool::Road,
            Self::Details | Self::Decorations => Tool::Pan,
        }
    }

    /// Label to show for `tool` when it's presented in this mode. Same
    /// underlying tool, different framing.
    pub fn tool_label(self, tool: Tool) -> &'static str {
        match tool {
            Tool::Pan => "Pan",
            Tool::Calibrate => "Calibrate scale",
            Tool::Road => "Road",
            Tool::Pavement => match self {
                Self::Sublayer => "Asphalt",
                _ => "Pavement",
            },
        }
    }
}