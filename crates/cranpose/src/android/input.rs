use cranpose_app_shell::{AppShell, KeyEvent, PointerSource};
use cranpose_platform_android::AndroidPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{PointerButton, PointerId};

use super::keyboard::{AndroidKeyTranslator, is_system_key};

pub(super) enum PendingInput {
    PointerDown(f32, f32, Option<i64>, PointerSource),
    PointerUp(f32, f32, Option<i64>, PointerSource),
    PointerMove(f32, f32, Option<i64>, PointerSource),
    PointerCancel,
    Key(KeyEvent),
    SecondaryPointerDown(PointerId, f32, f32, Option<i64>),
    SecondaryPointerUp(PointerId, f32, f32, Option<i64>),
    SecondaryPointerMove(PointerId, f32, f32, Option<i64>),
    RotaryScroll(f32, u64),
    /// A mouse button other than the primary one went down (`true`) or up,
    /// with the pointer at a position.
    MouseButton(PointerButton, bool, f32, f32),
}

fn android_event_time_ms(event_time_ns: i64) -> i64 {
    event_time_ns / 1_000_000
}

fn android_pointer_source(
    pointer: &android_activity::input::Pointer<'_>,
    event_source: android_activity::input::Source,
) -> PointerSource {
    use android_activity::input::{Source, ToolType};

    use crate::android_input::{AndroidSourceKind, AndroidToolKind, resolve_pointer_source};

    let tool = match pointer.tool_type() {
        ToolType::Finger => AndroidToolKind::Finger,
        ToolType::Mouse => AndroidToolKind::Mouse,
        ToolType::Stylus | ToolType::Eraser => AndroidToolKind::Stylus,
        _ => AndroidToolKind::Indeterminate,
    };
    let source = match event_source {
        Source::Touchscreen => AndroidSourceKind::Touchscreen,
        Source::Stylus | Source::BluetoothStylus => AndroidSourceKind::Stylus,
        Source::Mouse | Source::MouseRelative => AndroidSourceKind::Mouse,
        _ => AndroidSourceKind::Other,
    };
    resolve_pointer_source(tool, source)
}

fn push_pending_input_from_android_key_event(
    key_event: &android_activity::input::KeyEvent<'_>,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    if key_event.key_code() == android_activity::input::Keycode::Back {
        if cranpose_services::back_interception_enabled() {
            if key_event.action() == android_activity::input::KeyAction::Up {
                cranpose_services::push_back_request();
            }
            return true;
        }
        return false;
    }
    if is_system_key(key_event.key_code()) {
        return false;
    }
    let Some(event) = key_translator.translate(key_event) else {
        return false;
    };
    pending_inputs.push(PendingInput::Key(event));
    true
}

fn shell_pointer_id(android_pointer_id: i32, primary_pointer_id: i32) -> PointerId {
    if android_pointer_id == primary_pointer_id {
        PointerId::PRIMARY
    } else {
        PointerId(android_pointer_id as u64 + 1)
    }
}

fn push_pending_inputs_from_android_event(
    event: &android_activity::input::InputEvent<'_>,
    android_platform: &AndroidPlatform,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) -> bool {
    let motion_event = match event {
        android_activity::input::InputEvent::MotionEvent(motion_event) => motion_event,
        android_activity::input::InputEvent::KeyEvent(key_event) => {
            return push_pending_input_from_android_key_event(
                key_event,
                key_translator,
                pending_inputs,
            );
        }
        _ => return false,
    };

    let time_ms = Some(android_event_time_ms(motion_event.event_time()));
    let event_source = motion_event.source();
    let logical_of = |x: f32, y: f32| {
        let logical = android_platform.pointer_position(x as f64, y as f64);
        (logical.x, logical.y)
    };

    match motion_event.action() {
        android_activity::input::MotionAction::Down => {
            let pointer = motion_event.pointer_at_index(0);
            *primary_pointer_id = Some(pointer.pointer_id());
            let (x, y) = logical_of(pointer.x(), pointer.y());
            pending_inputs.push(PendingInput::PointerDown(
                x,
                y,
                time_ms,
                android_pointer_source(&pointer, event_source),
            ));
            true
        }
        android_activity::input::MotionAction::PointerDown => {
            let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
            let Some(primary) = *primary_pointer_id else {
                return true;
            };
            let (x, y) = logical_of(pointer.x(), pointer.y());
            pending_inputs.push(PendingInput::SecondaryPointerDown(
                shell_pointer_id(pointer.pointer_id(), primary),
                x,
                y,
                time_ms,
            ));
            true
        }
        android_activity::input::MotionAction::Up => push_final_release(
            motion_event,
            event_source,
            time_ms,
            logical_of,
            pending_inputs,
            primary_pointer_id,
        ),
        android_activity::input::MotionAction::PointerUp => push_pointer_release(
            motion_event,
            event_source,
            time_ms,
            logical_of,
            pending_inputs,
            primary_pointer_id,
        ),
        android_activity::input::MotionAction::Move => push_pointer_move(
            motion_event,
            event_source,
            time_ms,
            logical_of,
            pending_inputs,
            primary_pointer_id,
        ),
        android_activity::input::MotionAction::Cancel => {
            pending_inputs.push(PendingInput::PointerCancel);
            *primary_pointer_id = None;
            true
        }
        android_activity::input::MotionAction::Scroll => {
            push_rotary_pending_input(motion_event, event_source, time_ms, pending_inputs)
        }
        _ => false,
    }
}

fn push_final_release(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    logical_of: impl Fn(f32, f32) -> (f32, f32),
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) -> bool {
    let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
    let (x, y) = logical_of(pointer.x(), pointer.y());
    match *primary_pointer_id {
        Some(primary) if pointer.pointer_id() != primary => {
            pending_inputs.push(PendingInput::SecondaryPointerUp(
                shell_pointer_id(pointer.pointer_id(), primary),
                x,
                y,
                time_ms,
            ));
        }
        _ => {}
    }
    pending_inputs.push(PendingInput::PointerUp(
        x,
        y,
        time_ms,
        android_pointer_source(&pointer, event_source),
    ));
    *primary_pointer_id = None;
    true
}

fn push_pointer_release(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    logical_of: impl Fn(f32, f32) -> (f32, f32),
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) -> bool {
    let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
    let Some(primary) = *primary_pointer_id else {
        return true;
    };
    let (x, y) = logical_of(pointer.x(), pointer.y());
    if pointer.pointer_id() == primary {
        for other in motion_event.pointers() {
            if other.pointer_id() == primary {
                continue;
            }
            let (ox, oy) = logical_of(other.x(), other.y());
            pending_inputs.push(PendingInput::SecondaryPointerUp(
                shell_pointer_id(other.pointer_id(), primary),
                ox,
                oy,
                time_ms,
            ));
        }
        pending_inputs.push(PendingInput::PointerUp(
            x,
            y,
            time_ms,
            android_pointer_source(&pointer, event_source),
        ));
        *primary_pointer_id = None;
    } else {
        pending_inputs.push(PendingInput::SecondaryPointerUp(
            shell_pointer_id(pointer.pointer_id(), primary),
            x,
            y,
            time_ms,
        ));
    }
    true
}

fn push_pointer_move(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    logical_of: impl Fn(f32, f32) -> (f32, f32),
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) -> bool {
    let primary = *primary_pointer_id;
    for pointer in motion_event.pointers() {
        let is_primary = primary == Some(pointer.pointer_id());
        let source = android_pointer_source(&pointer, event_source);
        if is_primary {
            for historical in pointer.history() {
                let (hx, hy) = logical_of(historical.x(), historical.y());
                pending_inputs.push(PendingInput::PointerMove(
                    hx,
                    hy,
                    Some(android_event_time_ms(historical.event_time())),
                    source,
                ));
            }
        }
        let (x, y) = logical_of(pointer.x(), pointer.y());
        match (is_primary, primary) {
            (true, _) => {
                pending_inputs.push(PendingInput::PointerMove(x, y, time_ms, source));
            }
            (false, Some(primary)) => {
                pending_inputs.push(PendingInput::SecondaryPointerMove(
                    shell_pointer_id(pointer.pointer_id(), primary),
                    x,
                    y,
                    time_ms,
                ));
            }
            (false, None) => {}
        }
    }
    true
}

fn push_rotary_pending_input(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    use crate::android_input::is_rotary_encoder_source;

    if !is_rotary_encoder_source(u32::from(event_source)) {
        return false;
    }

    let Some(pointer) = motion_event.pointers().next() else {
        return true;
    };
    let detents = pointer.axis_value(android_activity::input::Axis::Scroll);
    if !detents.is_finite() || detents == 0.0 {
        return true;
    }

    pending_inputs.push(PendingInput::RotaryScroll(
        detents,
        time_ms.unwrap_or(0).max(0) as u64,
    ));
    true
}

pub(super) fn drain_android_input_events(
    app: &android_activity::AndroidApp,
    android_platform: &AndroidPlatform,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
    mouse_buttons: &mut android_activity::input::ButtonState,
) {
    let Ok(mut iter) = app.input_events_iter() else {
        return;
    };

    for _ in 0..MAX_ANDROID_INPUT_EVENTS_PER_POLL {
        let event_available = iter.next(|event| {
            if push_mouse_button_inputs(
                event,
                android_platform,
                pending_inputs,
                primary_pointer_id,
                mouse_buttons,
            ) || push_pending_inputs_from_android_event(
                event,
                android_platform,
                key_translator,
                pending_inputs,
                primary_pointer_id,
            ) {
                android_activity::InputStatus::Handled
            } else {
                android_activity::InputStatus::Unhandled
            }
        });
        if !event_available {
            break;
        }
    }
}

const MAX_ANDROID_INPUT_EVENTS_PER_POLL: usize = 10;

/// Takes a mouse's events that the path for fingers would read wrongly.
/// Android sends a press of the right or middle button alone as `Down`, the
/// moves of a drag with no primary button as `Move`, and every button's
/// change as `ButtonPress` or `ButtonRelease`. The primary button presses at
/// `Down`, or at its `ButtonPress` after another button; it releases at its
/// `ButtonRelease`, or at `Up` on a device that sends none. The other buttons
/// press and release as the shell's other mouse buttons. Returns whether it
/// took the event.
///
/// The button a press or release changed is the one `mouse_buttons`, the
/// buttons the last mouse event held, gains or loses: Android's
/// `AMotionEvent_getActionButton` arrived in API 33, and a library that
/// calls it does not load on an older device.
fn push_mouse_button_inputs(
    event: &android_activity::input::InputEvent<'_>,
    android_platform: &AndroidPlatform,
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
    mouse_buttons: &mut android_activity::input::ButtonState,
) -> bool {
    use android_activity::input::{Button, InputEvent, MotionAction, ToolType};

    let InputEvent::MotionEvent(motion_event) = event else {
        return false;
    };
    let Some(pointer) = motion_event.pointers().next() else {
        return false;
    };
    if pointer.tool_type() != ToolType::Mouse {
        return false;
    }
    let at = android_platform.pointer_position(f64::from(pointer.x()), f64::from(pointer.y()));
    let time_ms = Some(android_event_time_ms(motion_event.event_time()));
    let held = motion_event.button_state();
    let before = std::mem::replace(mouse_buttons, held);
    match motion_event.action() {
        MotionAction::Down => held.0 != 0 && !held.primary(),
        MotionAction::Move if primary_pointer_id.is_none() => {
            pending_inputs.push(PendingInput::PointerMove(
                at.x,
                at.y,
                time_ms,
                PointerSource::Mouse,
            ));
            true
        }
        MotionAction::Up => primary_pointer_id.is_none(),
        action @ (MotionAction::ButtonPress | MotionAction::ButtonRelease) => {
            let pressed = action == MotionAction::ButtonPress;
            let mut changed = if pressed {
                held.0 & !before.0
            } else {
                before.0 & !held.0
            };
            while changed != 0 {
                let bit = 1 << changed.trailing_zeros();
                changed &= !bit;
                match shell_mouse_button(Button::from(bit)) {
                    Some(PointerButton::Primary) => match (pressed, *primary_pointer_id) {
                        (true, None) => {
                            *primary_pointer_id = Some(pointer.pointer_id());
                            pending_inputs.push(PendingInput::PointerDown(
                                at.x,
                                at.y,
                                time_ms,
                                PointerSource::Mouse,
                            ));
                        }
                        (false, Some(_)) => {
                            *primary_pointer_id = None;
                            pending_inputs.push(PendingInput::PointerUp(
                                at.x,
                                at.y,
                                time_ms,
                                PointerSource::Mouse,
                            ));
                        }
                        _ => {}
                    },
                    Some(button) => {
                        pending_inputs.push(PendingInput::MouseButton(button, pressed, at.x, at.y));
                    }
                    None => {}
                }
            }
            true
        }
        _ => false,
    }
}

fn shell_mouse_button(button: android_activity::input::Button) -> Option<PointerButton> {
    use android_activity::input::Button;

    match button {
        Button::Primary => Some(PointerButton::Primary),
        Button::Secondary => Some(PointerButton::Secondary),
        Button::Tertiary => Some(PointerButton::Middle),
        Button::Back => Some(PointerButton::Back),
        Button::Forward => Some(PointerButton::Forward),
        _ => None,
    }
}

/// Sends one input taken from Android's queue to `shell`.
fn dispatch_pending_input(shell: &mut AppShell<WgpuRenderer>, input: PendingInput) {
    match input {
        PendingInput::PointerDown(x, y, time_ms, source) => {
            shell.set_pointer_source(source);
            let event_time = shell.realtime_pointer_event_time(time_ms);
            shell.set_cursor_at_event_time(x, y, event_time);
            shell.pointer_pressed_at_event_time(event_time);
        }
        PendingInput::PointerUp(x, y, time_ms, source) => {
            shell.set_pointer_source(source);
            let event_time = shell.realtime_pointer_event_time(time_ms);
            shell.pointer_released_at_position_event_time(x, y, event_time);
        }
        PendingInput::PointerMove(x, y, time_ms, source) => {
            shell.set_pointer_source(source);
            let event_time = shell.realtime_pointer_event_time(time_ms);
            shell.set_cursor_at_event_time(x, y, event_time);
        }
        PendingInput::PointerCancel => {
            shell.cancel_gesture();
        }
        PendingInput::Key(event) => {
            shell.on_key_event(&event);
        }
        PendingInput::SecondaryPointerDown(id, x, y, time_ms) => {
            shell.set_pointer_source(PointerSource::Touch);
            shell.secondary_pointer_pressed(id, x, y, time_ms);
        }
        PendingInput::SecondaryPointerUp(id, x, y, time_ms) => {
            shell.set_pointer_source(PointerSource::Touch);
            shell.secondary_pointer_released(id, x, y, time_ms);
        }
        PendingInput::SecondaryPointerMove(id, x, y, time_ms) => {
            shell.set_pointer_source(PointerSource::Touch);
            shell.secondary_pointer_moved(id, x, y, time_ms);
        }
        PendingInput::RotaryScroll(detents, uptime_ms) => {
            shell.rotary_scrolled_by_detents(detents, uptime_ms);
        }
        PendingInput::MouseButton(button, pressed, x, y) => {
            shell.set_pointer_source(PointerSource::Mouse);
            shell.set_cursor(x, y);
            let mut surface = shell.primary();
            if pressed {
                surface.mouse_button_pressed(button);
            } else {
                surface.mouse_button_released(button);
            }
        }
    }
}

pub(super) fn dispatch_pending_inputs(
    shell: &mut AppShell<WgpuRenderer>,
    pending: &mut Vec<PendingInput>,
) {
    for input in pending.drain(..) {
        dispatch_pending_input(shell, input);
    }
}
