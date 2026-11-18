use cranpose_core::{MutableState, RuntimeHandle, current_runtime_handle};

use crate::host::{LifecycleObserver, LifecycleState, current_lifecycle_state, observe_lifecycle};

// The lifecycle of the host as a state that the compositions of one runtime
// read, with the registration that moves it. A runtime gets its source when
// a composition of it first reads `local_lifecycle_state()`, so an app that
// never reads the local keeps no state and no registration.
pub(crate) struct LifecycleSource {
    runtime: RuntimeHandle,
    state: MutableState<LifecycleState>,
    _observer: LifecycleObserver,
}

impl LifecycleSource {
    fn new(runtime: RuntimeHandle) -> Self {
        let state = MutableState::with_runtime(current_lifecycle_state(), runtime.clone());
        // The host moves its lifecycle from any thread. The state moves on the
        // thread of its runtime: a host that moves it from another thread has
        // the runtime's dispatcher post the change.
        #[cfg(not(target_arch = "wasm32"))]
        let observer = {
            let dispatcher = runtime.dispatcher();
            let owner = std::thread::current().id();
            observe_lifecycle(move |event| {
                if std::thread::current().id() == owner {
                    publish(state, event.to);
                } else {
                    dispatcher.post(move || publish(state, event.to));
                }
            })
        };
        #[cfg(target_arch = "wasm32")]
        let observer = observe_lifecycle(move |event| publish(state, event.to));
        // A transition between the first read of the host and the registration.
        publish(state, current_lifecycle_state());
        Self {
            runtime,
            state,
            _observer: observer,
        }
    }
}

// A runtime that is gone leaves its state released, and its observer stays
// until the thread reads the local in another runtime.
fn publish(state: MutableState<LifecycleState>, to: LifecycleState) {
    if state.try_value().is_some_and(|current| current != to) {
        state.set(to);
    }
}

// The lifecycle of the host as the composition now running reads it: the scope
// that reads it composes again on each transition. A read outside any runtime
// gets the state of the moment.
pub(crate) fn observed_lifecycle_state() -> LifecycleState {
    let Some(runtime) = current_runtime_handle() else {
        return current_lifecycle_state();
    };
    let state = crate::thread_state::with(|thread| {
        let mut sources = thread.locals.lifecycle_sources.borrow_mut();
        if let Some(source) = sources
            .iter()
            .find(|source| source.runtime.id() == runtime.id())
        {
            return source.state;
        }
        // A runtime that is gone leaves its source behind until the next
        // runtime of this thread reads the local.
        sources.retain(|source| source.state.is_alive());
        let source = LifecycleSource::new(runtime);
        let state = source.state;
        sources.push(source);
        state
    });
    state.get()
}
