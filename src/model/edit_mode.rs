//! Edit modes: which broad layer of the design is being edited.

use super::tool::Tool;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditMode {
    Sublayer,
    Road,
    Details,
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
    /// Pan and Calibrate are universal. Road is Road-mode only. Pavement is
    /// Sublayer-only (it's the base-material shape tool). Details and
    /// Decorations have no design tools yet.
    pub fn allows_tool(self, tool: Tool) -> bool {
        match tool {
            Tool::Pan | Tool::Calibrate => true,
            Tool::Road => matches!(self, Self::Road),
            Tool::Pavement => matches!(self, Self::Sublayer),
        }
    }

    pub fn tools(self) -> &'static [Tool] {
        match self {
            Self::Sublayer => &[Tool::Pan, Tool::Calibrate, Tool::Pavement],
            Self::Road => &[Tool::Pan, Tool::Calibrate, Tool::Road],
            Self::Details | Self::Decorations => &[Tool::Pan, Tool::Calibrate],
        }
    }

    pub fn default_tool(self) -> Tool {
        match self {
            Self::Sublayer => Tool::Pavement,
            Self::Road => Tool::Road,
            Self::Details | Self::Decorations => Tool::Pan,
        }
    }

    /// Label to show for `tool` in this mode.
    pub fn tool_label(self, tool: Tool) -> &'static str {
        match tool {
            Tool::Pan => "Pan",
            Tool::Calibrate => "Calibrate scale",
            Tool::Road => "Road",
            Tool::Pavement => match self {
                Self::Sublayer => "Place shape",
                _ => "Pavement",
            },
        }
    }
}