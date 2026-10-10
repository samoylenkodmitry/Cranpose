#![doc = include_str!("../README.md")]

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
    Glass, GlassContourHighlight, GlassDeformation, GlassDynamics, GlassFaceResponse,
    GlassFaceTone, GlassKeyFill, GlassMorph, GlassRefraction, GlassShadow, GlassSpectrum,
    GlassSpecularHighlight, GlassVariant, LiquidModifierExt, LiquidShape, glass_light_direction,
    set_glass_light_direction,
};
pub use motion::{LiquidMotion, liquid_press_scale};
pub use theme::{
    LiquidColors, LiquidSegmentedStyle, LiquidTheme, LiquidThemeSpec, LiquidTypography,
    ProvideLiquidSegmentedStyle, SchemeMode, liquid_colors, liquid_glass_tint_amount,
    liquid_segmented_style, liquid_typography,
};
pub use widgets::*;

/// Everything an app needs to build Liquid UI.
pub mod prelude {
    pub use crate::{
        appearance::{GlassTintAmount, InvalidGlassTintAmount},
        dynamics::{LiquidDynamics, LiquidPose, rememberLiquidDynamics},
        icons,
        material::{
            Glass, GlassContourHighlight, GlassDeformation, GlassDynamics, GlassFaceResponse,
            GlassFaceTone, GlassKeyFill, GlassMorph, GlassRefraction, GlassShadow, GlassSpectrum,
            GlassSpecularHighlight, GlassVariant, LiquidModifierExt, LiquidShape,
        },
        motion::{LiquidMotion, liquid_press_scale},
        theme::{
            LiquidColors, LiquidSegmentedStyle, LiquidTheme, LiquidThemeSpec, LiquidTypography,
            ProvideLiquidSegmentedStyle, SchemeMode, liquid_colors, liquid_glass_tint_amount,
            liquid_segmented_style, liquid_typography,
        },
        widgets::*,
    };
}
