//! Liquid UI — cranpose's first-party component library.
//!
//! An iOS-26-style "Liquid Glass" design system: translucent lens materials
//! over live backdrop (refraction, blur, vibrancy, chromatic aberration,
//! specular rims), semantic theming with automatic light/dark, and
//! spring-physics motion. Pure UI code — no platform dependencies; runs on
//! every cranpose target.
//!
//! ```ignore
//! use cranpose::liquid::prelude::*;
//!
//! LiquidTheme(LiquidThemeSpec::default(), || {
//!     GlassButton(Modifier::empty(), GlassButtonSpec::prominent(), on_click, || {
//!         GlassButtonLabel("Add");
//!     });
//! });
//! ```

#![deny(unsafe_code)]
#![allow(non_snake_case)]

pub mod dynamics;
pub mod icons;
pub mod material;
pub mod motion;
pub mod theme;
pub mod widgets;

pub use cranpose_ui_graphics::{
    GlassProfileCurve, GlassProfileError, GlassProfileKnot, GlassSurfaceProfile,
    MAX_GLASS_PROFILE_KNOTS,
};
pub use dynamics::{remember_liquid_dynamics, LiquidDynamics, LiquidPose};
pub use material::{
    Glass, GlassDeformation, GlassDynamics, GlassShadow, GlassVariant, LiquidModifierExt,
    LiquidShape,
};
pub use motion::{liquid_press_scale, LiquidMotion};
pub use theme::{
    liquid_colors, liquid_typography, LiquidColors, LiquidTheme, LiquidThemeSpec, LiquidTypography,
    SchemeMode,
};
pub use widgets::*;

/// Everything an app needs to build Liquid UI.
pub mod prelude {
    pub use crate::dynamics::{remember_liquid_dynamics, LiquidDynamics, LiquidPose};
    pub use crate::icons;
    pub use crate::material::{
        Glass, GlassDeformation, GlassDynamics, GlassShadow, GlassVariant, LiquidModifierExt,
        LiquidShape,
    };
    pub use crate::motion::{liquid_press_scale, LiquidMotion};
    pub use crate::theme::{
        liquid_colors, liquid_typography, LiquidColors, LiquidTheme, LiquidThemeSpec,
        LiquidTypography, SchemeMode,
    };
    pub use crate::widgets::*;
    pub use cranpose_ui_graphics::{
        GlassProfileCurve, GlassProfileError, GlassProfileKnot, GlassSurfaceProfile,
        MAX_GLASS_PROFILE_KNOTS,
    };
}
