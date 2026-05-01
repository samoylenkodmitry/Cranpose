//! Testing utilities and harness for Cranpose

#![allow(non_snake_case)]

pub mod robot;
pub mod robot_assertions;
pub mod robot_helpers;
pub mod testing;

// Re-export testing utilities
pub use robot::*;
pub use robot_assertions::{assert_robot_fps_over, Bounds, SemanticElementLike};
pub use robot_helpers::*;
pub use testing::*;

pub mod prelude {
    pub use crate::robot::*;
    pub use crate::robot_assertions;
    pub use crate::robot_assertions::{assert_robot_fps_over, Bounds, SemanticElementLike};
    pub use crate::robot_helpers::*;
    pub use crate::testing::*;
}
