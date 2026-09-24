use std::cell::{Cell, RefCell};

use cranpose_app_shell::Modifiers;
use cranpose_core::{location_key, remember};
use cranpose_ui::{
    BasicTextField, Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, Size, Spacer,
    TextFieldState, TextStyle, composable,
};

use super::*;

thread_local! {
    static CLICKS: Cell<u32> = const { Cell::new(0) };
    static SCROLL: RefCell<Option<ScrollState>> = const { RefCell::new(None) };
    static FIELD: RefCell<Option<TextFieldState>> = const { RefCell::new(None) };
}

#[composable]
fn input_probe() {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let field = remember(|| TextFieldState::new("")).with(|state| *state);
    SCROLL.with(|slot| *slot.borrow_mut() = Some(scroll));
    FIELD.with(|slot| *slot.borrow_mut() = Some(field));
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(100.0, 50.0)
                    .clickable(|_| CLICKS.with(|clicks| clicks.set(clicks.get() + 1))),
                BoxSpec::new(),
                || {},
            );
            BasicTextField(
                field,
                Modifier::empty().width(200.0).height(40.0),
                TextStyle::default(),
            );
            Spacer(Size {
                width: 0.0,
                height: 900.0,
            });
        },
    );
}

fn probe_shell() -> AppShell<WgpuRenderer> {
    CLICKS.with(|clicks| clicks.set(0));
    let mut shell = AppShell::new(
        WgpuRenderer::new(&[]),
        location_key(file!(), line!(), column!()),
        input_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();
    shell
}

fn click(input: &EmbedInput, shell: &mut AppShell<WgpuRenderer>, x: f32, y: f32) {
    input.dispatch(shell, HostEvent::PointerMove { x, y });
    input.dispatch(shell, HostEvent::PointerDown { x, y });
    input.dispatch(shell, HostEvent::PointerUp { x, y });
    shell.update();
}

fn field_text() -> String {
    FIELD.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(TextFieldState::text)
            .unwrap_or_default()
    })
}

#[test]
fn a_press_and_release_over_a_clickable_clicks_it() {
    let input = EmbedInput::new();
    let mut shell = probe_shell();

    click(&input, &mut shell, 50.0, 25.0);
    assert_eq!(CLICKS.with(Cell::get), 1);

    click(&input, &mut shell, 250.0, 25.0);
    assert_eq!(
        CLICKS.with(Cell::get),
        1,
        "a press outside the clickable must not click it"
    );
}

#[test]
fn leaving_the_surface_is_not_a_click() {
    let input = EmbedInput::new();
    let mut shell = probe_shell();

    input.dispatch(&mut shell, HostEvent::PointerMove { x: 50.0, y: 25.0 });
    input.dispatch(&mut shell, HostEvent::PointerLeave);
    shell.update();

    assert_eq!(CLICKS.with(Cell::get), 0);
}

#[test]
fn a_wheel_down_scrolls_the_content_under_the_pointer() {
    let input = EmbedInput::new();
    let mut shell = probe_shell();

    input.dispatch(
        &mut shell,
        HostEvent::Scroll {
            x: 150.0,
            y: 150.0,
            delta_x: 0.0,
            delta_y: -120.0,
            modifiers: Modifiers::NONE,
        },
    );
    shell.update();

    let offset = SCROLL.with(|slot| slot.borrow().as_ref().map(ScrollState::value_non_reactive));
    assert!(
        offset.is_some_and(|offset| offset > 0.0),
        "the column should have scrolled, offset {offset:?}"
    );
}

#[test]
fn typed_text_and_keys_edit_the_focused_field() {
    let input = EmbedInput::new();
    let mut shell = probe_shell();
    click(&input, &mut shell, 100.0, 70.0);

    input.dispatch(&mut shell, HostEvent::Text("hi".to_string()));
    shell.update();
    assert_eq!(field_text(), "hi");

    input.dispatch(
        &mut shell,
        HostEvent::Key {
            down: true,
            modifiers: Modifiers::NONE,
            code: "Backspace".to_string(),
        },
    );
    input.dispatch(
        &mut shell,
        HostEvent::Key {
            down: false,
            modifiers: Modifiers::NONE,
            code: "Backspace".to_string(),
        },
    );
    shell.update();
    assert_eq!(field_text(), "h");
}

#[test]
fn surface_and_message_events_are_not_input() {
    let input = EmbedInput::new();
    let mut shell = probe_shell();

    assert!(!input.dispatch(&mut shell, HostEvent::Theme { dark: true }));
    assert!(!input.dispatch(&mut shell, HostEvent::FrameAck(1)));
    assert!(!input.dispatch(&mut shell, HostEvent::Close));
}
