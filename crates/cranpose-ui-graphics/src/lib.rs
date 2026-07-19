//! Pure math/data for drawing & units in Cranpose
//!
//! This crate contains geometry primitives, color definitions, brushes,
//! and unit types that are used throughout the Cranpose framework.

#![deny(unsafe_code)]
#![allow(non_snake_case)]

pub mod alpha_mask;
mod brush;
mod color;
pub mod framework_shaders;
mod geometry;
mod gradient_blur;
mod image;
pub mod liquid_glass;
pub mod render_effect;
mod render_hash;
mod shadow;
mod typography;
mod unit;
mod vector_path;

pub use alpha_mask::*;
pub use brush::*;
pub use color::*;
pub use geometry::*;
pub use gradient_blur::*;
pub use image::*;
pub use liquid_glass::*;
pub use render_effect::*;
pub use render_hash::*;
pub use shadow::*;
pub use typography::*;
pub use unit::*;
pub use vector_path::*;

pub mod prelude {
    pub use crate::brush::Brush;
    pub use crate::color::Color;
    pub use crate::geometry::{CornerRadii, EdgeInsets, Point, Rect, RoundedCornerShape, Size};
    pub use crate::image::{ColorFilter, ImageBitmap, ImageBitmapError, ImageSampling};
    pub use crate::unit::{Dp, Sp};
    pub use crate::vector_path::{PathFillRule, SvgPathError, VectorPath};
}
