//! Pure math/data for drawing & units in Cranpose
//!
//! This crate contains geometry primitives, color definitions, brushes,
//! and unit types that are used throughout the Cranpose framework.

#![allow(non_snake_case)]

mod brush;
mod color;
mod geometry;
mod image;
pub mod liquid_glass;
pub mod render_effect;
mod typography;
mod unit;

pub use brush::*;
pub use color::*;
pub use geometry::*;
pub use image::*;
pub use liquid_glass::*;
pub use render_effect::*;
pub use typography::*;
pub use unit::*;

pub mod prelude {
    pub use crate::brush::Brush;
    pub use crate::color::Color;
    pub use crate::geometry::{CornerRadii, EdgeInsets, Point, Rect, RoundedCornerShape, Size};
    pub use crate::image::{ColorFilter, ImageBitmap, ImageBitmapError};
    pub use crate::unit::{Dp, Sp};
}
