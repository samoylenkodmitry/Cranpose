//! Testing utilities and harness for Cranpose

#![allow(non_snake_case)]

pub mod accessibility_audit;
pub mod placed_semantics;
pub mod robot;
pub mod robot_assertions;
#[cfg(feature = "desktop-robot")]
pub mod robot_helpers;
pub mod testing;

pub use accessibility_audit::{
    AccessibilityIssue, AccessibilityIssueKind, MINIMUM_TARGET_SIZE, assert_accessible,
    audit_accessibility, spoken_name,
};
pub use placed_semantics::{
    PlacedSemanticsNode, placed_semantics_from_applier, placed_semantics_from_shell,
    placed_semantics_from_trees,
};
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
        accessibility_audit::{
            AccessibilityIssue, AccessibilityIssueKind, assert_accessible, audit_accessibility,
        },
        placed_semantics::{
            PlacedSemanticsNode, placed_semantics_from_applier, placed_semantics_from_shell,
        },
        robot::*,
        robot_assertions,
        robot_assertions::{Bounds, SemanticElementLike},
        testing::*,
    };
}
