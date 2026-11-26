use std::{cell::Cell, rc::Rc, time::Duration};

use cranpose_core::{
    Composition, MemoryApplier,
    concurrency::{delay, next_alarm_deadline},
    location_key, rememberCoroutineScope,
};
use web_time::Instant;

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "verifies host-driven deadline elapsing across real wall time"
)]
fn host_timers_drive_delay_without_a_background_thread() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let done = Rc::new(Cell::new(false));
    let done_in_task = Rc::clone(&done);

    assert!(next_alarm_deadline().is_none());

    composition
        .render(key, move || {
            let scope = rememberCoroutineScope();
            let flag = Rc::clone(&done_in_task);
            scope.launch(async move {
                delay(Duration::from_millis(25)).await;
                flag.set(true);
            });
        })
        .expect("render succeeds");

    let runtime = composition.runtime_handle();

    // First drain polls the task and arms the host alarm inside runtime TLS.
    runtime.drain_ui();
    assert!(!done.get());
    let deadline = next_alarm_deadline().expect("delay armed a host alarm");
    assert!(deadline > Instant::now());

    // Draining before the deadline must not complete the delay early.
    runtime.drain_ui();
    assert!(!done.get());

    std::thread::sleep(Duration::from_millis(35));
    assert!(runtime.has_pending_ui());
    runtime.drain_ui();
    assert!(done.get());
    assert!(next_alarm_deadline().is_none());
}
