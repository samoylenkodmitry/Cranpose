use cranpose::{PointerButton, Robot};

use crate::robot_exit;

// Drags the mouse with `button` held from `from` to `to` in ten steps.
pub(crate) fn drag(robot: &Robot, button: PointerButton, from: (f32, f32), to: (f32, f32)) {
    let mut steps = (1..=10).map(|step| {
        let t = step as f32 / 10.0;
        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
    });
    let dragged = robot
        .move_to(from.0, from.1)
        .and_then(|()| robot.mouse_button_down(button))
        .and_then(|()| steps.try_for_each(|(x, y)| robot.move_to(x, y)))
        .and_then(|()| robot.mouse_button_up(button));
    if let Err(err) = dragged {
        robot_exit::fail(
            robot,
            &format!("{button:?} drag from {from:?} to {to:?}: {err}"),
        );
    }
}
