use std::{rc::Rc, sync::Arc};

use super::*;
use crate::{
    snapshot_v2::runtime::TestRuntimeGuard,
    state::{MutationPolicy, NeverEqual, SnapshotMutableState, StateRecord},
};

fn reset_runtime() -> TestRuntimeGuard {
    crate::snapshot_pinning::reset_pinning_table();
    reset_runtime_for_tests()
}

#[cfg(test)]
#[path = "integration_tests_tests.rs"]
mod tests;
