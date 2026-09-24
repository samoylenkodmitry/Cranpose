use std::{future::Future, sync::OnceLock};

use coroflow::{Emitter, Flow, FlowExt, MutableStateFlow, StateFlow};
use cranpose_services::{
    LifecycleObserver, LifecycleState, current_lifecycle_state, observe_lifecycle,
};

struct LifecycleFlow {
    state: MutableStateFlow<LifecycleState>,
    _observer: LifecycleObserver,
}

/// The host's lifecycle as a state flow — Android's
/// `lifecycle.currentStateFlow`.
pub fn lifecycle_state_flow() -> StateFlow<LifecycleState> {
    static FLOW: OnceLock<LifecycleFlow> = OnceLock::new();
    FLOW.get_or_init(|| {
        let state = MutableStateFlow::new(current_lifecycle_state());
        let target = state.clone();
        let observer = observe_lifecycle(move |event| target.set(event.to));
        state.set(current_lifecycle_state());
        LifecycleFlow {
            state,
            _observer: observer,
        }
    })
    .state
    .as_state_flow()
}

/// Whether work tied to `min` runs in `state`. A host that reports no
/// lifecycle stays [`LifecycleState::Created`], which counts as active.
pub fn is_active_for(state: LifecycleState, min: LifecycleState) -> bool {
    state == LifecycleState::Created || state.is_at_least(min)
}

/// Lifecycle-aware operators for any flow.
pub trait LifecycleFlowExt: Flow + Clone + Sized + 'static {
    /// Emits this flow's values only while the host is at least `min`,
    /// cancelling the collection below it and starting it again when the
    /// host comes back — Android's `flowWithLifecycle`.
    fn flow_with_lifecycle(self, min: LifecycleState) -> impl Flow<Item = Self::Item> {
        lifecycle_state_flow()
            .map(move |state| is_active_for(state, min))
            .distinct_until_changed()
            .transform_latest(async move |active, emitter: Emitter<Self::Item>| {
                if active {
                    emitter.emit_all(self).await;
                }
            })
    }
}

impl<F: Flow + Clone + Sized + 'static> LifecycleFlowExt for F {}

/// Runs `block` each time the host reaches `min` and cancels it when the host
/// falls below; returns once the host is destroyed — Android's
/// `repeatOnLifecycle`.
pub async fn repeat_on_lifecycle<F, Fut>(min: LifecycleState, block: F)
where
    F: FnOnce() -> Fut + Clone,
    Fut: Future<Output = ()>,
{
    lifecycle_state_flow()
        .transform_while(async move |state, emitter| {
            emitter.emit(is_active_for(state, min)).await;
            state != LifecycleState::Destroyed
        })
        .distinct_until_changed()
        .collect_latest(async move |active| {
            if active {
                block().await;
            }
        })
        .await;
}
