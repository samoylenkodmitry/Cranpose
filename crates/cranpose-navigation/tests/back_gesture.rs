mod support;

use cranpose_services::{back_interception_enabled, push_back_request};
use support::{Probe, Screen, screens};

// One test per binary: back requests and their interception are process-wide.
#[test]
fn the_back_gesture_pops_while_there_is_a_screen_beneath() {
    let probe = Probe::default();
    let (mut host, nav) = screens(&probe);
    let nav = nav.get().expect("the host composed its controller");
    assert!(
        !back_interception_enabled(),
        "back leaves the app from the start destination"
    );
    nav.navigate(Screen::Detail(1));
    host.settle();
    assert!(back_interception_enabled());

    push_back_request();
    host.settle();
    assert_eq!(nav.back_stack(), [Screen::Home]);
    assert_eq!(probe.live(), [Screen::Home]);
    assert!(!back_interception_enabled());
}
