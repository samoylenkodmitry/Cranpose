#![allow(dead_code, reason = "each test binary uses a part of the support")]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_core::{Composition, MemoryApplier};

pub(crate) struct DropMarker(pub Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) const PATIENCE: Duration = Duration::from_secs(5);
pub(crate) const QUIET_PERIOD: Duration = Duration::from_millis(200);

pub(crate) fn composition() -> Composition<MemoryApplier> {
    Composition::new(MemoryApplier::new())
}

pub(crate) fn pump_until(
    composition: &mut Composition<MemoryApplier>,
    render: impl FnMut(&mut Composition<MemoryApplier>),
    done: impl FnMut() -> bool,
) -> bool {
    pump_for(PATIENCE, composition, render, done)
}

pub(crate) fn pump_for(
    patience: Duration,
    composition: &mut Composition<MemoryApplier>,
    mut render: impl FnMut(&mut Composition<MemoryApplier>),
    mut done: impl FnMut() -> bool,
) -> bool {
    let runtime = composition.runtime_handle();
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        runtime.drain_ui();
        render(composition);
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}
