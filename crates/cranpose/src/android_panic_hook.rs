//! Panic-hook chaining for the Android runtime.
//!
//! An application installs its own panic hook — for crash reporting, say —
//! before handing control to `AppLauncher::run_android()`. `android.rs`'s
//! `run()` used to call `std::panic::set_hook()` unconditionally, silently
//! replacing whatever the application had installed. [`chained_panic_hook`]
//! fixes that: it wraps the framework's own diagnostic hook around the hook
//! that was already installed (captured via `std::panic::take_hook()`
//! before the framework installs its own), so both run.
//!
//! `android.rs` is gated to `target_os = "android"` and cannot be unit
//! tested on the host. The chaining itself is plain `std::panic`, nothing
//! Android-specific, so it lives here instead, where it is also built on
//! the host under `cfg(test)`.

use std::panic::PanicHookInfo;

/// Wraps `own_hook` so it runs, then `previous_hook` runs — the hook
/// returned by `std::panic::take_hook()` right before the framework installs
/// its own — producing a single hook that runs both instead of one
/// replacing the other.
///
/// `own_hook` must not panic. A panic raised while a panic hook is already
/// running for the panic it was called for is an unrecoverable nested panic:
/// the process aborts immediately, before unwinding starts, so not even
/// `catch_unwind` around `own_hook` can stop it from also taking
/// `previous_hook` — and with it, the application's own crash reporting —
/// down. Keep `own_hook` to operations that cannot panic (plain getters,
/// `downcast_ref`, `format!`, `std::backtrace::Backtrace::force_capture()`,
/// logging), the same way `own_hook` in `android.rs` does.
pub(crate) fn chained_panic_hook(
    own_hook: impl Fn(&PanicHookInfo<'_>) + Sync + Send + 'static,
    previous_hook: Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send>,
) -> Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send> {
    Box::new(move |info| {
        own_hook(info);
        previous_hook(info);
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    // `std::panic::set_hook`/`take_hook` mutate process-global state, so
    // tests that install a hook must not run concurrently with each other.
    static HOOK_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn chained_hook_runs_both_the_framework_hook_and_the_previously_installed_one() {
        let _guard = HOOK_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let saved_hook = std::panic::take_hook();

        // Stand in for an application's own hook, installed before the
        // framework runs and captured here exactly as `android.rs` captures
        // it: via `take_hook()`.
        let marker_fired = Arc::new(AtomicBool::new(false));
        let marker_for_previous = Arc::clone(&marker_fired);
        std::panic::set_hook(Box::new(move |_| {
            marker_for_previous.store(true, Ordering::SeqCst)
        }));
        let previous_hook = std::panic::take_hook();

        let own_fired = Arc::new(AtomicBool::new(false));
        let own_for_hook = Arc::clone(&own_fired);
        std::panic::set_hook(chained_panic_hook(
            move |_| own_for_hook.store(true, Ordering::SeqCst),
            previous_hook,
        ));

        let panicked = std::panic::catch_unwind(|| panic!("chained_panic_hook test panic"));

        std::panic::set_hook(saved_hook);

        assert!(
            panicked.is_err(),
            "the panic should have unwound and been caught"
        );
        assert!(
            own_fired.load(Ordering::SeqCst),
            "framework's own hook did not run"
        );
        assert!(
            marker_fired.load(Ordering::SeqCst),
            "previously installed hook was replaced instead of chained onto"
        );
    }
}
