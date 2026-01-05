//! Testing utilities and harness for Compose-RS

#![allow(non_snake_case)]

pub mod robot;
pub mod robot_assertions;
pub mod testing;

#[cfg(feature = "robot-app")]
pub mod robot_helpers;

// Re-export testing utilities
pub use robot::*;
pub use robot_assertions::{Bounds, SemanticElementLike};
pub use testing::*;

#[cfg(feature = "robot-app")]
pub use robot_helpers::*;

pub mod prelude {
    pub use crate::robot::*;
    pub use crate::robot_assertions;
    pub use crate::robot_assertions::{Bounds, SemanticElementLike};
    pub use crate::testing::*;

    #[cfg(feature = "robot-app")]
    pub use crate::robot_helpers::*;
}
