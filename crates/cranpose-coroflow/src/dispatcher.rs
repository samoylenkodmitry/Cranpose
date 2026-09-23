use coroflow::{ConfinedDispatcher, Dispatch, Runnable, SystemClock};
use cranpose_core::current_runtime_handle;

#[cfg(not(target_arch = "wasm32"))]
struct UiExecutor {
    dispatcher: cranpose_core::UiDispatcher,
}

#[cfg(not(target_arch = "wasm32"))]
impl Dispatch for UiExecutor {
    fn dispatch(&self, runnable: Runnable) {
        self.dispatcher.post(move || runnable.run());
    }
}

#[cfg(target_arch = "wasm32")]
struct UiExecutor;

#[cfg(target_arch = "wasm32")]
impl Dispatch for UiExecutor {
    fn dispatch(&self, runnable: Runnable) {
        match current_runtime_handle() {
            Some(runtime) => runtime.post_ui(move || runnable.run()),
            None => log::error!(
                "cranpose-coroflow: the page's runtime is gone; a coroutine step was dropped"
            ),
        }
    }
}

/// The main-thread dispatcher of the Cranpose runtime on this thread —
/// Android's `Dispatchers.Main`.
///
/// Coroutine steps are posted to the runtime's UI queue, which wakes the frame
/// loop, so a value published from a background pool reaches the UI on the
/// next frame. In the browser, where there is one thread and one runtime per
/// page, steps go to the page's runtime. Returns `None` when this thread has
/// no runtime.
pub fn main_dispatcher() -> Option<ConfinedDispatcher> {
    let runtime = current_runtime_handle()?;
    #[cfg(not(target_arch = "wasm32"))]
    let executor = UiExecutor {
        dispatcher: runtime.dispatcher(),
    };
    #[cfg(target_arch = "wasm32")]
    let executor = {
        drop(runtime);
        UiExecutor
    };
    Some(ConfinedDispatcher::for_current_thread(
        executor,
        SystemClock::shared(),
    ))
}

pub(crate) fn require_main_dispatcher(caller: &str) -> ConfinedDispatcher {
    main_dispatcher()
        .unwrap_or_else(|| panic!("{caller} requires an active Cranpose runtime on this thread"))
}
