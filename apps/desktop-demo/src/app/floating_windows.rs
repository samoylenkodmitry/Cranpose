#![allow(non_snake_case)]

use cranpose_core::rememberMutableStateOf;
use cranpose_ui::{
    composable, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row,
    RowSpec, Text,
};

use super::{
    chrome_tabs::{label_style, INK},
    flame_window::flame_window_app,
    pet::pet_app,
};

pub fn toggle_label(open: bool, opened: &'static str, closed: &'static str) -> &'static str {
    if open {
        closed
    } else {
        opened
    }
}

#[composable]
pub fn FloatingWindowsTab() {
    let pet_open = rememberMutableStateOf(|| true);
    let flame_open = rememberMutableStateOf(|| true);
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec {
            vertical_arrangement: LinearArrangement::SpacedBy(12.0),
            ..ColumnSpec::default()
        },
        move || {
            Text(
                "Windows of their own",
                Modifier::empty(),
                label_style(20.0, INK),
            );
            Text(
                "Each of these is a subtree with the window modifier on it: a borderless, \
                 transparent window above every other, drawn by a shader and shaped by what \
                 the shader draws. Nothing else in the application knows a window exists.",
                Modifier::empty(),
                label_style(13.0, Color(1.0, 1.0, 1.0, 0.72)),
            );
            Row(
                Modifier::empty(),
                RowSpec {
                    horizontal_arrangement: LinearArrangement::SpacedBy(10.0),
                    ..RowSpec::default()
                },
                move || {
                    Toggle(
                        toggle_label(pet_open.get(), "Open the pet", "Close the pet"),
                        move || pet_open.set(!pet_open.get()),
                    );
                    Toggle(
                        toggle_label(flame_open.get(), "Light the flame", "Put the flame out"),
                        move || flame_open.set(!flame_open.get()),
                    );
                },
            );
            Text(
                "Drag either one anywhere on the desktop; click it to change its face and its \
                 fire.",
                Modifier::empty(),
                label_style(12.0, Color(1.0, 1.0, 1.0, 0.55)),
            );
        },
    );
    if pet_open.get() {
        pet_app();
    }
    if flame_open.get() {
        flame_window_app();
    }
}

#[composable]
fn Toggle(label: &'static str, on_click: impl Fn() + 'static) {
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                label,
                Modifier::empty().padding(8.0),
                label_style(13.0, INK),
            );
        },
    );
}

#[cfg(test)]
#[path = "tests/floating_windows_tests.rs"]
mod tests;
