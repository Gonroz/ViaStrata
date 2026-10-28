//! Base materials: what a pavement shape is filled with.
//!
//! Materials occupy the sublayer of the design — the surface a road sits on.
//! They're deliberately a small, fixed set rather than a full color picker,
//! because the point of a material is that it reads as a *surface* (asphalt
//! vs. concrete vs. grass) rather than as arbitrary color. Adding a fifth
//! later is a one-line change to `all()` and `color()`.

use eframe::egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Material {
    /// The default road surface. A very dark, near-neutral grey with a
    /// slight cool cast, chosen so white and yellow markings pop against
    /// it and satellite imagery doesn't show through.
    Asphalt,
    /// Structural concrete — brighter and cooler than asphalt. Good for
    /// plazas, bridge decks, and other poured surfaces.
    Concrete,
    /// Sidewalk paving. A warm, weathered tan — the color most city
    /// sidewalks settle into after a year in the sun.
    Sidewalk,
    /// Grass. Muted rather than lush, so it reads as a material rather
    /// than a highlight and doesn't compete with markings.
    Grass,
}

impl Material {
    pub fn label(self) -> &'static str {
        match self {
            Material::Asphalt => "Asphalt",
            Material::Concrete => "Concrete",
            Material::Sidewalk => "Sidewalk",
            Material::Grass => "Grass",
        }
    }

    /// Every material, in display order: darkest to lightest, since that
    /// roughly tracks how much of the imagery shows through and how
    /// strongly it contrasts with road markings.
    pub fn all() -> &'static [Material] {
        &[
            Material::Asphalt,
            Material::Concrete,
            Material::Sidewalk,
            Material::Grass,
        ]
    }

    /// The RGB triple used for both the fill and the sidebar swatch. Kept
    /// as raw bytes rather than a `Color32` so it can also be used to
    /// build colors with custom alpha (e.g. the draft preview).
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Material::Asphalt => [58, 58, 62],
            Material::Concrete => [150, 148, 145],
            Material::Sidewalk => [195, 178, 148],
            Material::Grass => [78, 110, 62],
        }
    }

    pub fn color(self) -> Color32 {
        let [r, g, b] = self.rgb();
        Color32::from_rgb(r, g, b)
    }

    /// The same color at reduced alpha, used for the in-progress draft
    /// preview so the user can see what they're drawing before committing.
    pub fn preview_color(self) -> Color32 {
        let [r, g, b] = self.rgb();
        Color32::from_rgba_unmultiplied(r, g, b, 110)
    }
}