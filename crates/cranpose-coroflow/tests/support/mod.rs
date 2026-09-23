#![allow(dead_code)]

use std::time::{Duration, Instant};

use cranpose_core::{Composition, MemoryApplier};

pub const PATIENCE: Duration = Duration::from_secs(5);
pub const QUIET_PERIOD: Duration = Duration::from_millis(200);

pub fn composition() -> Composition<MemoryApplier> {
    Composition::new(MemoryApplier::new())
}

pub fn pump_until(
    composition: &mut Composition<MemoryApplier>,
    render: impl FnMut(&mut Composition<MemoryApplier>),
    done: impl FnMut() -> bool,
) -> bool {
    pump_for(PATIENCE, composition, render, done)
}

pub fn pump_for(
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
