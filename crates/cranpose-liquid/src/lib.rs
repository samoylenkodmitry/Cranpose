//! Liquid UI — cranpose's first-party component library.
//!
//! An iOS-26-style "Liquid Glass" design system: translucent lens materials
//! over live backdrop (wcKSRD refraction, blur, vibrancy, and edge light),
//! semantic theming with automatic light/dark, and
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

#![allow(non_snake_case)]

pub mod appearance;
pub mod dynamics;
pub mod icons;
pub mod material;
pub mod motion;
pub mod theme;
pub mod widgets;

pub use appearance::{GlassTintAmount, InvalidGlassTintAmount};
pub use dynamics::{LiquidDynamics, LiquidPose, rememberLiquidDynamics};
pub use material::{
    Glass, GlassDeformation, GlassDynamics, GlassFaceResponse, GlassKeyFill, GlassMorph,
    GlassRefraction, GlassShadow, GlassSpectrum, GlassVariant, LiquidModifierExt, LiquidShape,
    glass_light_direction, set_glass_light_direction,
};
pub use motion::{LiquidMotion, liquid_press_scale};
pub use theme::{
    LiquidColors, LiquidTheme, LiquidThemeSpec, LiquidTypography, SchemeMode, liquid_colors,
    liquid_glass_tint_amount, liquid_typography,
};
pub use widgets::*;

/// Everything an app needs to build Liquid UI.
pub mod prelude {
    pub use crate::{
        appearance::{GlassTintAmount, InvalidGlassTintAmount},
        dynamics::{LiquidDynamics, LiquidPose, rememberLiquidDynamics},
        icons,
        material::{
            Glass, GlassDeformation, GlassDynamics, GlassFaceResponse, GlassKeyFill, GlassMorph,
            GlassRefraction, GlassShadow, GlassSpectrum, GlassVariant, LiquidModifierExt,
            LiquidShape,
        },
        motion::{LiquidMotion, liquid_press_scale},
        theme::{
            LiquidColors, LiquidTheme, LiquidThemeSpec, LiquidTypography, SchemeMode,
            liquid_colors, liquid_glass_tint_amount, liquid_typography,
        },
        widgets::*,
    };
}
