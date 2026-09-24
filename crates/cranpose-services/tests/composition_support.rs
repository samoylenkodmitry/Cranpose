use std::{
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use cranpose_core::{Composition, MemoryApplier};

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
