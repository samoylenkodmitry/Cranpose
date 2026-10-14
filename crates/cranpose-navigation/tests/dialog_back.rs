mod support;

use cranpose_services::back_interception_enabled;
use support::{Probe, Screen, settle_robot, with_settings_dialog};

// One test per binary: back requests and their interception are process-wide.
#[test]
fn the_back_gesture_dismisses_a_dialog_route() {
    let probe = Probe::default();
    let (mut robot, nav) = with_settings_dialog(&probe);
    let nav = nav.get().expect("the host composed its controller");
    nav.navigate(Screen::Settings);
    settle_robot(&mut robot);
    assert!(
        !back_interception_enabled(),
        "the host leaves the back gesture to the dialog on top"
    );

    // The platform gives the back gesture to the innermost modal first.
    assert!(robot.shell_mut().dismiss_top_modal(), "the dialog takes it");
    settle_robot(&mut robot);
    assert_eq!(nav.back_stack(), [Screen::Home]);
    assert_eq!(probe.live(), [Screen::Home]);
    assert_eq!(probe.dropped(), 1, "the dialog's view model is dropped");
}
