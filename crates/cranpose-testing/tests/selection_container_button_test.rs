// A button inside a `SelectionContainer` takes a click as it does
// anywhere else, through the public shell.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Button, ButtonSpec, Column, ColumnSpec, Modifier, Text, TextStyle, widgets::SelectionContainer,
};

#[test]
fn a_click_on_a_button_in_a_selection_container_reaches_the_button() {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let counted = Rc::clone(&counted);
            SelectionContainer(Modifier::empty(), move || {
                let counted = Rc::clone(&counted);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    let counted = Rc::clone(&counted);
                    Button(
                        Modifier::empty().size_points(120.0, 40.0),
                        ButtonSpec::default(),
                        move || counted.set(counted.get() + 1),
                        || {
                            Text("Press", Modifier::empty(), TextStyle::default());
                        },
                    );
                    Text("Words to select", Modifier::empty(), TextStyle::default());
                });
            });
        },
    );
    shell.set_viewport(300.0, 300.0);
    shell.set_buffer_size(300, 300);
    shell.update_at_frame_time_nanos(16_000_000);
    let (x, y) = (60.0, 20.0);
    shell.set_cursor_at_time(x, y, Some(100));
    shell.pointer_pressed_at_time(Some(100));
    shell.pointer_released_at_position_time(x, y, Some(140));
    shell.update_at_frame_time_nanos(32_000_000);
    assert_eq!(clicks.get(), 1, "the button took the click");
}
