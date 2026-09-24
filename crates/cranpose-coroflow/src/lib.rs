//! Runs [`coroflow`] coroutines, flows and view models inside Cranpose.
//!
//! [`coroflow`] knows nothing about UI; this crate supplies the pieces Android
//! gets from `lifecycle-viewmodel-compose` and `runtime`:
//!
//! | Android | Cranpose |
//! |---|---|
//! | `Dispatchers.Main` | [`main_dispatcher`], then `coroflow::Dispatchers::main()` |
//! | `viewModel { }` + `viewModelScope` | [`rememberViewModel`], a `Copy` [`Handle`] |
//! | `SavedStateHandle` | [`SavedStateHandle`] |
//! | `rememberCoroutineScope()` | [`rememberCoroutineScope`] |
//! | `StateFlow.collectAsState()` | [`StateFlowCollect::collectAsState`] |
//! | `Flow.collectAsState(initial)` | [`FlowCollect::collectAsState`] |
//! | `collectAsStateWithLifecycle()` | [`StateFlowCollect::collectAsStateWithLifecycle`], [`FlowCollect::collectAsStateWithLifecycle`] |
//! | `lifecycle.currentStateFlow`, `flowWithLifecycle`, `repeatOnLifecycle` | [`lifecycle_state_flow`], [`LifecycleFlowExt::flow_with_lifecycle`], [`repeat_on_lifecycle`] |
//! | `LaunchedEffect(key) { flow.collect { } }` | [`CollectFlow`] |
//! | `snapshotFlow { }` | [`snapshotFlow`] |

#![expect(non_snake_case)]

mod dispatcher;
mod hooks;
mod lifecycle;
mod saved_state;
mod snapshot_flow;

pub use dispatcher::main_dispatcher;
pub use hooks::{
    CollectFlow, FlowCollect, Handle, StateFlowCollect, collects_in, rememberCoroutineScope,
    rememberHandle, rememberViewModel,
};
pub use lifecycle::{LifecycleFlowExt, is_active_for, lifecycle_state_flow, repeat_on_lifecycle};
pub use saved_state::SavedStateHandle;
pub use snapshot_flow::{SnapshotFlow, SnapshotRun, snapshotFlow};
