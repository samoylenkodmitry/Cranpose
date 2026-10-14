use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use cranpose_core::{CollectEvents, Composition, EventStream, Key, MemoryApplier};

pub(crate) fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn pump_until(
    composition: &mut Composition<MemoryApplier>,
    done: impl Fn() -> bool,
) -> bool {
    let runtime = composition.runtime_handle();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        runtime.drain_ui();
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

/// Renders content that collects `stream()` into `seen`, each item as
/// `describe` names it.
pub(crate) fn render_collector<T: 'static>(
    composition: &mut Composition<MemoryApplier>,
    key: Key,
    stream: fn() -> EventStream<T>,
    describe: fn(&T) -> String,
    seen: &Rc<RefCell<Vec<String>>>,
) {
    let sink = Rc::clone(seen);
    composition
        .render(key, move || {
            let events = stream();
            let sink = Rc::clone(&sink);
            CollectEvents(events, (), move |item: T| {
                sink.borrow_mut().push(describe(&item));
            });
        })
        .expect("render succeeds");
}
