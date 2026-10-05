#![doc = include_str!("../README.md")]

/// Host-provided accessibility preferences shared by animation and rendering.
///
/// ```
/// use cranpose_ui_graphics::accessibility::{
///     AccessibilityOptions, platform_accessibility_options, set_platform_accessibility_options,
/// };
///
/// set_platform_accessibility_options(AccessibilityOptions {
///     reduce_motion: true,
///     ..AccessibilityOptions::default()
/// });
/// assert!(platform_accessibility_options().reduce_motion);
/// ```
pub mod accessibility;
pub mod brush_sampling;

// A framework shader as the build script wrote it: `shaders/NAME` without
// its comments.
macro_rules! framework_wgsl {
    ($name:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/shaders/", $name))
    };
}

pub mod alpha_mask;
mod brush;
mod color;
mod float;
pub mod framework_shaders;
mod fx_hash;
mod geometry;
mod gradient_blur;
mod image;
pub mod layer_transform;
pub mod liquid_glass;
mod path;
mod pointer_icon;
mod record;
pub mod render_effect;
mod render_hash;
mod shader_warm_up;
mod shadow;
mod shape_records;
mod stroke;
pub mod transform;
mod typography;
pub mod unit;
mod vector_path;
mod vertex_gradient;

pub use alpha_mask::*;
pub use brush::*;
pub use color::*;
pub use fx_hash::*;
pub use geometry::*;
pub use gradient_blur::*;
pub use image::*;
pub use liquid_glass::*;
pub use path::*;
pub use pointer_icon::*;
pub use record::*;
pub use render_effect::*;
pub use render_hash::*;
pub use shader_warm_up::*;
pub use shadow::*;
pub use shape_records::*;
pub use stroke::*;
pub use transform::{ProjectiveTransform, WindowCoordinates};
pub use typography::*;
pub use unit::*;
pub use vector_path::*;

pub mod prelude {
    pub use crate::{
        brush::Brush,
        color::Color,
        geometry::{CornerRadii, EdgeInsets, Point, Rect, RoundedCornerShape, Size},
        image::{ColorFilter, ImageBitmap, ImageBitmapError, ImagePixelFormat, ImageSampling},
        pointer_icon::{CursorIcon, CustomPointerIcon, PointerIcon, PointerIconError},
        stroke::{ArcGeometry, Stroke, StrokeCap, StrokeJoin},
        unit::{Dp, Sp},
        vector_path::{PathFillRule, SvgPathError, VectorPath},
    };
}
