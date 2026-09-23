use coroflow::{ConfinedDispatcher, Dispatch, Runnable, SystemClock};
use cranpose_core::{UiDispatcher, current_runtime_handle};

struct UiExecutor {
    dispatcher: UiDispatcher,
}

impl Dispatch for UiExecutor {
    fn dispatch(&self, runnable: Runnable) {
        self.dispatcher.post(move || runnable.run());
    }
}

/// The main-thread dispatcher of the Cranpose runtime on this thread —
/// Android's `Dispatchers.Main`.
///
/// Coroutine steps are posted to the runtime's UI queue, which wakes the frame
/// loop, so a value published from a background pool reaches the UI on the
/// next frame. Returns `None` when this thread has no runtime.
pub fn main_dispatcher() -> Option<ConfinedDispatcher> {
    current_runtime_handle().map(|runtime| {
        ConfinedDispatcher::for_current_thread(
            UiExecutor {
                dispatcher: runtime.dispatcher(),
            },
            SystemClock::shared(),
        )
    })
}

pub(crate) fn require_main_dispatcher(caller: &str) -> ConfinedDispatcher {
    main_dispatcher()
        .unwrap_or_else(|| panic!("{caller} requires an active Cranpose runtime on this thread"))
}
