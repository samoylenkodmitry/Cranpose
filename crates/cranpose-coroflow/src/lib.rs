//! Runs [`coroflow`] coroutines, flows and view models inside Cranpose.
//!
//! [`coroflow`] knows nothing about UI; this crate supplies the pieces Android
//! gets from `lifecycle-viewmodel-compose` and `runtime`:
//!
//! | Android | Cranpose |
//! |---|---|
//! | `Dispatchers.Main` | [`main_dispatcher`] |
//! | `viewModel { }` + `viewModelScope` | [`rememberViewModel`], a `Copy` [`Handle`] |
//! | `StateFlow.collectAsState()` | [`StateFlowCollect::collectAsState`] |
//! | `collectAsStateWithLifecycle()` | [`StateFlowCollect::collectAsStateWithLifecycle`] |
//! | `LaunchedEffect(key) { flow.collect { } }` | [`CollectFlow`] |
//! | `snapshotFlow { }` | [`snapshotFlow`] |

#![expect(non_snake_case)]

mod dispatcher;
mod hooks;
mod snapshot_flow;

pub use dispatcher::main_dispatcher;
pub use hooks::{
    CollectFlow, Handle, StateFlowCollect, collects_in, rememberHandle, rememberViewModel,
};
pub use snapshot_flow::{SnapshotFlow, SnapshotRun, snapshotFlow};
