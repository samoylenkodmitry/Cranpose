// The keys Compose's clickable takes press the focused control as they go
// down and click it as they come up, and a held one long-presses it.

use std::{cell::Cell, rc::Rc, time::Duration};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{Box, BoxSpec, FocusRequester, Key, KeyEvent, Modifier, Size};

struct Control {
    shell: AppShell<TestRenderer>,
    clicks: Rc<Cell<u32>>,
    long_presses: Rc<Cell<u32>>,
}

fn control() -> Control {
    let clicks = Rc::new(Cell::new(0));
    let long_presses = Rc::new(Cell::new(0));
    let focus = FocusRequester::new();
    let (counted, held, requester) = (Rc::clone(&clicks), Rc::clone(&long_presses), focus.clone());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let counted = Rc::clone(&counted);
            let held = Rc::clone(&held);
            Box(
                Modifier::empty()
                    .size(Size::new(120.0, 60.0))
                    .focus_requester(&requester)
                    .on_long_press("Show more", move || held.set(held.get() + 1))
                    .clickable(move |_| counted.set(counted.get() + 1)),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.set_viewport(200.0, 100.0);
    shell.set_buffer_size(200, 100);
    shell.update();
    focus.request_focus().expect("the control is composed");
    shell.update();
    Control {
        shell,
        clicks,
        long_presses,
    }
}

#[test]
fn a_click_key_presses_on_down_and_clicks_on_up() {
    for key in [
        Key::DirectionCenter,
        Key::Enter,
        Key::NumPadEnter,
        Key::Spacebar,
    ] {
        let mut control = control();
        assert!(control.shell.on_key_event(&KeyEvent::key_down(key, "")));
        control.shell.update();
        assert_eq!(control.clicks.get(), 0, "{key:?} down only presses");
        assert!(
            control
                .shell
                .on_key_event(&KeyEvent::key_down(key, "").with_repeat_count(1)),
            "a repeat of the held key is taken"
        );
        assert!(control.shell.on_key_event(&KeyEvent::key_up(key)));
        control.shell.update();
        assert_eq!(control.clicks.get(), 1, "{key:?} up clicks once");
    }
}

#[test]
fn a_held_click_key_long_presses_and_does_not_click() {
    let mut control = control();
    assert!(
        control
            .shell
            .on_key_event(&KeyEvent::key_down(Key::DirectionCenter, ""))
    );
    for _ in 0..30 {
        control
            .shell
            .update_after_exact_interval(Duration::from_millis(20));
    }
    assert_eq!(
        control.long_presses.get(),
        1,
        "held past the long press time"
    );
    control
        .shell
        .on_key_event(&KeyEvent::key_up(Key::DirectionCenter));
    control.shell.update();
    assert_eq!(control.clicks.get(), 0, "a long press does not click");
}
