#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod alignment;
mod alignment_lines;
mod arrangement;
mod axis;
mod constraints;
mod core;
mod intrinsics;

pub use core::*;

pub use alignment::*;
pub use alignment_lines::*;
pub use arrangement::*;
pub use axis::*;
pub use constraints::*;
pub use cranpose_ui_graphics::{ceil_to_grid, ceil_to_px, round_to_grid, round_to_px};
pub use intrinsics::*;

/// The layout API in one import.
pub mod prelude {
    pub use crate::{
        alignment::{Alignment, HorizontalAlignment, VerticalAlignment},
        arrangement::LinearArrangement,
        constraints::Constraints,
        core::{Measurable, MeasureScope, Placeable},
    };
}
