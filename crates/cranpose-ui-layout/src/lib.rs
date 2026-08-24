//! Layout contracts & policies for Cranpose

#![allow(non_snake_case)]

mod alignment;
mod arrangement;
mod axis;
mod constraints;
mod core;
mod intrinsics;

pub use core::*;

pub use alignment::*;
pub use arrangement::*;
pub use axis::*;
pub use constraints::*;
pub use intrinsics::*;

pub mod prelude {
    pub use crate::{
        alignment::{Alignment, HorizontalAlignment, VerticalAlignment},
        arrangement::LinearArrangement,
        constraints::Constraints,
        core::{Measurable, MeasureScope, Placeable},
    };
}
