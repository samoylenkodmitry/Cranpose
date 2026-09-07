//! Pure math/data for drawing & units in Cranpose
//!
//! This crate contains geometry primitives, color definitions, brushes,
//! and unit types that are used throughout the Cranpose framework.

#![allow(non_snake_case)]

pub mod alpha_mask;
mod arc_trig_cache;
mod brush;
mod color;
pub mod framework_shaders;
mod fx_hash;
mod geometry;
mod gradient_blur;
mod image;
pub mod liquid_glass;
mod record;
pub mod render_effect;
mod render_hash;
mod shadow;
mod shape_records;
mod stroke;
mod typography;
pub mod unit;
mod vector_path;

pub use alpha_mask::*;
pub use brush::*;
pub use color::*;
pub use fx_hash::*;
pub use geometry::*;
pub use gradient_blur::*;
pub use image::*;
pub use liquid_glass::*;
pub use record::*;
pub use render_effect::*;
pub use render_hash::*;
pub use shadow::*;
pub use shape_records::*;
pub use stroke::*;
pub use typography::*;
pub use unit::*;
pub use vector_path::*;

pub mod prelude {
    pub use crate::{
        brush::Brush,
        color::Color,
        geometry::{CornerRadii, EdgeInsets, Point, Rect, RoundedCornerShape, Size},
        image::{ColorFilter, ImageBitmap, ImageBitmapError, ImageSampling},
        stroke::{ArcGeometry, Stroke, StrokeCap, StrokeJoin},
        unit::{Dp, Sp},
        vector_path::{PathFillRule, SvgPathError, VectorPath},
    };
}
