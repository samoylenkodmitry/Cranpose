use super::*;

fn with_values(trigger: f32, squeeze: f32) -> Controller {
    Controller {
        trigger: Trigger {
            value: trigger,
            ..Trigger::default()
        },
        squeeze: Squeeze {
            value: squeeze,
            ..Squeeze::default()
        },
        ..Controller::default()
    }
}

// Runs the controller through frames with the given values, as the backend does, and answers the
// presses that each frame ends with.
fn presses(values: &[(f32, f32)]) -> Vec<(bool, bool)> {
    let mut before = Controller::default();
    values
        .iter()
        .map(|&(trigger, squeeze)| {
            let mut now = with_values(trigger, squeeze);
            now.settle_presses(&before);
            before = now;
            (now.trigger.pressed, now.squeeze.pressed)
        })
        .collect()
}

#[test]
fn a_trigger_presses_above_the_high_mark_and_lets_go_below_the_low_mark() {
    let pulls = [0.0, 0.5, 0.6, 0.5, 0.45, 0.3, 0.5, 0.7];
    let trigger: Vec<bool> = presses(&pulls.map(|pull| (pull, 0.0)))
        .into_iter()
        .map(|(trigger, _)| trigger)
        .collect();
    assert_eq!(
        trigger,
        [false, false, true, true, true, false, false, true]
    );
}

#[test]
fn a_click_that_gives_a_value_of_one_is_a_press() {
    let squeeze: Vec<bool> = presses(&[(0.0, 0.0), (0.0, 1.0), (0.0, 1.0), (0.0, 0.0)])
        .into_iter()
        .map(|(_, squeeze)| squeeze)
        .collect();
    assert_eq!(squeeze, [false, true, true, false]);
}

#[test]
fn the_trigger_and_the_grip_press_apart() {
    let both = presses(&[(0.9, 0.1), (0.9, 0.9), (0.1, 0.9)]);
    assert_eq!(both, [(true, false), (true, true), (false, true)]);
}
