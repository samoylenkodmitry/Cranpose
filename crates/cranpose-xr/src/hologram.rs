//! The colors of a hologram and what stands around its controls.

use cranpose::Color;

/// The colors of a [`SpatialHologram`](crate::SpatialHologram): the glass of its objects, the
/// objects of chosen options and the light of its projector, beams and sparks. Each color is the
/// light that an object adds to what lies behind it, so a dark color shows faint and black shows
/// nothing.
///
/// Jetpack XR has no hologram and so no such colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HologramColors {
    /// The glass of every object, its rim and its edges
    pub glass: Color,
    /// The object of a chosen option, a switch that is on, and the flash of a press
    pub chosen: Color,
    /// The projector's disc and light, the beam under a chosen gem and the sparks
    pub light: Color,
}

impl HologramColors {
    /// Cyan glass with brighter cyan for the chosen objects.
    pub const CYAN: Self = Self {
        glass: Color(0.16, 0.62, 1.0, 1.0),
        chosen: Color(0.3, 0.95, 1.0, 1.0),
        light: Color(0.15, 0.7, 1.0, 1.0),
    };

    /// Violet glass with gold for the chosen objects and the light.
    pub const VIOLET_GOLD: Self = Self {
        glass: Color(0.55, 0.36, 1.0, 1.0),
        chosen: Color(1.0, 0.66, 0.2, 1.0),
        light: Color(1.0, 0.55, 0.18, 1.0),
    };
}

/// What stands around a [`SpatialHologram`](crate::SpatialHologram)'s controls. The page rises out
/// of the projector as it shows and goes back into it as it hides, also with no projector drawn.
///
/// Jetpack XR has no hologram and so no such stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HologramStage {
    /// A disc of rings under the page that sends a cone of light up to it
    pub projector: bool,
    /// The bands of glass that wind beside the page, every other one on its left
    pub ribbons: u32,
    /// The sparks that drift beside the page
    pub particles: u32,
}

impl HologramStage {
    /// Nothing around the controls, as for a single button.
    pub const NONE: Self = Self {
        projector: false,
        ribbons: 0,
        particles: 0,
    };

    /// A projector, two ribbons and forty sparks, as for a page of settings.
    pub const PAGE: Self = Self {
        projector: true,
        ribbons: 2,
        particles: 40,
    };

    // Whether anything of the stage moves with time.
    pub(crate) fn moves(self) -> bool {
        self.projector || self.ribbons > 0 || self.particles > 0
    }
}
