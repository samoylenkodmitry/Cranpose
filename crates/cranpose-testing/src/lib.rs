//! Testing utilities and harness for Cranpose

#![allow(non_snake_case)]

pub mod placed_semantics;
pub mod robot;
pub mod robot_assertions;
#[cfg(feature = "desktop-robot")]
pub mod robot_helpers;
pub mod testing;

pub use placed_semantics::{PlacedSemanticsNode, placed_semantics_from_applier};
pub use robot::*;
#[cfg(feature = "desktop-robot")]
pub use robot_assertions::assert_robot_fps_over;
pub use robot_assertions::{Bounds, SemanticElementLike};
#[cfg(feature = "desktop-robot")]
pub use robot_helpers::*;
pub use testing::*;

pub mod prelude {
    #[cfg(feature = "desktop-robot")]
    pub use crate::robot_assertions::assert_robot_fps_over;
    #[cfg(feature = "desktop-robot")]
    pub use crate::robot_helpers::*;
    pub use crate::{
        placed_semantics::{PlacedSemanticsNode, placed_semantics_from_applier},
        robot::*,
        robot_assertions,
        robot_assertions::{Bounds, SemanticElementLike},
        testing::*,
    };
}
