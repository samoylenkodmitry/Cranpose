//! Testing utilities and harness for Cranpose

#![deny(unsafe_code)]
#![allow(non_snake_case)]

pub mod robot;
pub mod robot_assertions;
#[cfg(feature = "desktop-robot")]
pub mod robot_helpers;
pub mod testing;

// Re-export testing utilities
pub use robot::*;
#[cfg(feature = "desktop-robot")]
pub use robot_assertions::assert_robot_fps_over;
pub use robot_assertions::{Bounds, SemanticElementLike};
#[cfg(feature = "desktop-robot")]
pub use robot_helpers::*;
pub use testing::*;

pub mod prelude {
    pub use crate::robot::*;
    pub use crate::robot_assertions;
    #[cfg(feature = "desktop-robot")]
    pub use crate::robot_assertions::assert_robot_fps_over;
    pub use crate::robot_assertions::{Bounds, SemanticElementLike};
    #[cfg(feature = "desktop-robot")]
    pub use crate::robot_helpers::*;
    pub use crate::testing::*;
}
