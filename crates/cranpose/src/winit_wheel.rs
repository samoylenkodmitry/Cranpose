use cranpose_app_shell::{Modifiers, WheelScroll};
use cranpose_ui::Point;
use winit::keyboard::ModifiersState;

pub(crate) fn wheel_scroll_from_winit(
    logical_delta: Point,
    modifiers: ModifiersState,
    uptime_millis: u64,
) -> WheelScroll {
    WheelScroll::new(logical_delta, uptime_millis).with_modifiers(Modifiers {
        shift: modifiers.contains(ModifiersState::SHIFT),
        ctrl: modifiers.contains(ModifiersState::CONTROL),
        alt: modifiers.contains(ModifiersState::ALT),
        meta: modifiers.contains(ModifiersState::META),
    })
}

#[cfg(test)]
#[path = "tests/winit_wheel_tests.rs"]
mod tests;
