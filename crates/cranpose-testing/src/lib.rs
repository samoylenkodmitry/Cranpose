//! Testing utilities and harness for Cranpose

#![allow(non_snake_case)]

pub use cranpose_app_shell::{accessibility_audit, placed_semantics};
pub mod robot;
pub mod robot_assertions;
#[cfg(feature = "desktop-robot")]
pub mod robot_helpers;
pub mod testing;

pub use accessibility_audit::{
    AccessibilityIssue, AccessibilityIssueKind, KnownIssue, MINIMUM_TARGET_SIZE, assert_accessible,
    audit_accessibility, audit_changes, spoken_name,
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
            AccessibilityIssue, AccessibilityIssueKind, KnownIssue, assert_accessible,
            audit_accessibility, audit_changes,
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
