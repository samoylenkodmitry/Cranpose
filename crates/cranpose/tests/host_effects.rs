use std::{cell::Cell, rc::Rc};

use cranpose_ui::run_test_composition;

#[test]
fn keeping_the_screen_on_composes_enabled_and_disabled() {
    for enabled in [false, true] {
        run_test_composition(move || cranpose::KeepScreenOn(enabled));
    }
    cranpose_services::set_keep_screen_on(false);
}

#[test]
fn a_back_handler_composes_and_releases_its_interception() {
    let pressed = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&pressed);
    run_test_composition(move || {
        let counter = Rc::clone(&counter);
        cranpose::BackHandler(true, move || counter.set(counter.get() + 1));
    });
    assert_eq!(pressed.get(), 0, "nothing pressed back, yet it fired");
    assert!(
        !cranpose_services::back_interception_enabled(),
        "back interception outlived the composition that asked for it"
    );
}

#[test]
fn a_lifecycle_effect_composes_and_sees_nothing_until_the_host_says_so() {
    let events = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&events);
    run_test_composition(move || {
        let counter = Rc::clone(&counter);
        cranpose::LifecycleEffect((), move |_event| counter.set(counter.get() + 1));
    });
    assert_eq!(
        events.get(),
        0,
        "a lifecycle event arrived with no host to send one"
    );
}

#[test]
fn a_frame_effect_composes_running_and_stopped() {
    let frames = Rc::new(Cell::new(0usize));
    for running in [false, true] {
        let counter = Rc::clone(&frames);
        run_test_composition(move || {
            let counter = Rc::clone(&counter);
            cranpose::FrameEffect((), running, move |_nanos| counter.set(counter.get() + 1));
        });
    }
    assert_eq!(
        frames.get(),
        0,
        "a frame was delivered without one being dispatched"
    );
}

#[test]
fn the_update_state_is_readable_from_composition_and_starts_unknown() {
    run_test_composition(|| {
        let status = cranpose::rememberAppUpdateState();
        assert_eq!(
            status.get(),
            cranpose_services::AppUpdateStatus::default(),
            "an update was reported with no backend to report one"
        );
    });
}

#[test]
fn a_window_state_reports_the_size_it_was_created_with() {
    run_test_composition(|| {
        let state = cranpose::rememberWindowState(320.0, 240.0);
        assert_eq!(
            state.size_non_reactive(),
            cranpose_ui::Size::new(320.0, 240.0)
        );
    });
}
