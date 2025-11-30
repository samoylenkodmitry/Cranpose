//! Testing utilities and harness for Compose-RS

#![allow(non_snake_case)]

pub mod testing;
pub mod robot;
pub mod robot_assertions;

// Re-export testing utilities
pub use testing::*;
pub use robot::*;

pub mod prelude {
    pub use crate::testing::*;
    pub use crate::robot::*;
    pub use crate::robot_assertions;
}
