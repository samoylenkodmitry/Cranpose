// Two taps count as a double tap by the times of their pointer events, not by
// when the events reach the app: in a text field and in a `SelectionContainer`,
// with the platform's sample time (Android, the web) and with the shell's
// frame-clock time (desktop, iOS, watchOS).

use cranpose_app_shell::{AppShell, PointerEventTime};
use cranpose_core::location_key;
use cranpose_foundation::PointerSource;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, Key, KeyEvent, Modifier, Text, TextStyle, widgets::SelectionContainer,
};

use crate::support_text_field::{COMMAND, FocusedField, text_width};

const TEXT: &str = "hello world";

#[derive(Clone, Copy)]
enum Clock {
    // The shell takes every event at its last frame time, as it does for
    // events that arrive in one batch.
    Platform,
    FrameClock,
}

impl Clock {
    fn at(self, shell: &AppShell<TestRenderer>, millis: u64) -> PointerEventTime {
        let frame = shell.exact_pointer_event_time(None);
        match self {
            Self::Platform => PointerEventTime {
                platform_time_ms: Some(millis as i64),
                ..frame
            },
            Self::FrameClock => PointerEventTime {
                animation_time_nanos: frame.animation_time_nanos + millis * 1_000_000,
                ..frame
            },
        }
    }
}

// The x of a tap inside "world".
fn world_x(shell: &AppShell<TestRenderer>) -> f32 {
    text_width(shell, "hello wor")
}

// No frame runs before or after the tap.
fn tap(shell: &mut AppShell<TestRenderer>, time: PointerEventTime, x: f32, y: f32) {
    shell.set_cursor_at_event_time(x, y, time);
    assert!(
        shell.pointer_pressed_at_event_time(time),
        "the tap lands on the text"
    );
    shell.pointer_released_at_event_time(time);
}

fn field_after_two_taps(clock: Clock, apart_ms: u64, second_x_offset: f32) -> (usize, usize) {
    let mut field = FocusedField::new(TEXT, BasicTextFieldOptions::default());
    let x = world_x(&field.shell);
    let first = clock.at(&field.shell, 1_000);
    let second = clock.at(&field.shell, 1_000 + apart_ms);
    tap(&mut field.shell, first, x, 10.0);
    tap(&mut field.shell, second, x + second_x_offset, 10.0);
    field.shell.update();
    field.selection()
}

#[test]
fn text_field_taps_a_second_apart_are_two_single_taps() {
    for clock in [Clock::Platform, Clock::FrameClock] {
        let (start, end) = field_after_two_taps(clock, 1_000, 0.0);
        assert_eq!(start, end, "the second tap places the caret");
        assert!((6..=11).contains(&start), "the caret is in 'world'");
    }
}

#[test]
fn text_field_taps_100_ms_apart_are_a_double_tap() {
    for clock in [Clock::Platform, Clock::FrameClock] {
        assert_eq!(field_after_two_taps(clock, 100, 0.0), (6, 11));
    }
}

#[test]
fn text_field_taps_far_apart_in_space_are_two_single_taps() {
    for clock in [Clock::Platform, Clock::FrameClock] {
        let (start, end) = field_after_two_taps(clock, 100, -40.0);
        assert_eq!(start, end, "the second tap places the caret");
    }
}

#[test]
fn platform_time_counts_taps_whatever_frame_time_the_shell_took_them_at() {
    let mut field = FocusedField::new(TEXT, BasicTextFieldOptions::default());
    let x = world_x(&field.shell);
    let first = Clock::Platform.at(&field.shell, 5_000);
    let second = PointerEventTime {
        platform_time_ms: Some(5_100),
        animation_time_nanos: first.animation_time_nanos + 2_000_000_000,
        ..first
    };
    tap(&mut field.shell, first, x, 10.0);
    tap(&mut field.shell, second, x, 10.0);
    assert_eq!(field.selection(), (6, 11));
}

// What a copy takes from the container after the clicks.
fn container_copy_after_two_clicks(clock: Clock, apart_ms: u64) -> Option<String> {
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            SelectionContainer(Modifier::empty(), || {
                Text(TEXT, Modifier::empty(), TextStyle::default());
            });
        },
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    shell.update();
    shell.set_pointer_source(PointerSource::Mouse);
    let x = world_x(&shell);
    let first = clock.at(&shell, 1_000);
    let second = clock.at(&shell, 1_000 + apart_ms);
    tap(&mut shell, first, x, 5.0);
    tap(&mut shell, second, x, 5.0);
    shell.update();
    shell.on_key_event(&KeyEvent::key_down_with_modifiers(Key::C, "c", COMMAND));
    shell
        .app_context()
        .enter(cranpose_ui::clipboard_session::clipboard_read_text)
}

#[test]
fn selection_container_clicks_a_second_apart_are_two_single_clicks() {
    for clock in [Clock::Platform, Clock::FrameClock] {
        assert_eq!(container_copy_after_two_clicks(clock, 1_000), None);
    }
}

#[test]
fn selection_container_clicks_100_ms_apart_are_a_double_click() {
    for clock in [Clock::Platform, Clock::FrameClock] {
        assert_eq!(
            container_copy_after_two_clicks(clock, 100).as_deref(),
            Some("world")
        );
    }
}
