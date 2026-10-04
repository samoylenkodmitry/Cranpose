#![doc = include_str!("../README.md")]
#![expect(non_snake_case)]

mod dispatcher;
mod hooks;
mod lifecycle;
mod lifecycle_effects;
mod saved_state;
mod snapshot_flow;
mod view_model;

pub use dispatcher::main_dispatcher;
pub use hooks::{
    CollectFlow, FlowCollect, Handle, StateFlowCollect, collects_in, rememberCoroutineScope,
    rememberHandle,
};
pub use lifecycle::{LifecycleFlowExt, is_active_for, lifecycle_state_flow, repeat_on_lifecycle};
pub use lifecycle_effects::{
    LifecyclePauseOrDisposeEffectResult, LifecycleResumeEffect, LifecycleResumePauseEffectScope,
    LifecycleStartEffect, LifecycleStartStopEffectScope, LifecycleStopOrDisposeEffectResult,
};
pub use saved_state::SavedStateHandle;
pub use snapshot_flow::{SnapshotFlow, SnapshotRun, snapshotFlow};
pub use view_model::{ProvideViewModelStore, ViewModelStore, ViewModelStoreOwner, viewModel};
