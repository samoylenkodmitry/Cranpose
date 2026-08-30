use std::panic::PanicHookInfo;

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

    static HOOK_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn chained_hook_runs_both_the_framework_hook_and_the_previously_installed_one() {
        let _guard = HOOK_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let saved_hook = std::panic::take_hook();

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
