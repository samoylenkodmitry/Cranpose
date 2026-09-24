use std::sync::{Arc, Mutex, PoisonError};

use cranpose_services::{
    LifecycleState, advance_lifecycle, current_lifecycle_state, observe_lifecycle,
    window_lifecycle_state,
};

#[test]
fn a_window_is_resumed_when_focused_paused_when_only_visible_and_stopped_when_hidden() {
    assert_eq!(window_lifecycle_state(true, true), LifecycleState::Resumed);
    assert_eq!(window_lifecycle_state(true, false), LifecycleState::Paused);
    assert_eq!(window_lifecycle_state(false, true), LifecycleState::Stopped);
    assert_eq!(
        window_lifecycle_state(false, false),
        LifecycleState::Stopped
    );
}

#[test]
fn advancing_the_lifecycle_passes_through_every_state_in_between() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let _observer = observe_lifecycle(move |event| {
        log.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.to);
    });
    assert_eq!(current_lifecycle_state(), LifecycleState::Created);
    for (visible, focused) in [
        (true, true),
        (true, false),
        (false, false),
        (true, false),
        (true, true),
    ] {
        advance_lifecycle(window_lifecycle_state(visible, focused));
    }
    advance_lifecycle(LifecycleState::Destroyed);
    advance_lifecycle(LifecycleState::Resumed);
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        seen,
        vec![
            LifecycleState::Started,
            LifecycleState::Resumed,
            LifecycleState::Paused,
            LifecycleState::Stopped,
            LifecycleState::Started,
            LifecycleState::Resumed,
            LifecycleState::Paused,
            LifecycleState::Stopped,
            LifecycleState::Destroyed,
        ]
    );
}
