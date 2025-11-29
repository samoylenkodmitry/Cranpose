//! Testing utilities and harness for Compose-RS

#![allow(non_snake_case)]

pub mod testing;
pub mod test_renderer;
pub mod test_rule;

pub use test_rule::*;

// Re-export testing utilities
// pub use testing::*;

pub mod prelude {
    pub use crate::testing::*;
}
