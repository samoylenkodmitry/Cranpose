#![allow(non_snake_case)]

use cranpose::{LocalWindowState, WindowModifierExt, WindowState};
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{composable, Modifier, Point, PointerEventKind, PointerInputScope};

pub fn clicks_after_press(
    clicks: u32,
    pressed_at: Option<Point>,
    released_at: Option<Point>,
) -> u32 {
    if pressed_at == released_at {
        clicks + 1
    } else {
        clicks
    }
}

#[derive(Clone, Copy)]
pub struct FloatingInput {
    window: Option<WindowState>,
    pressed: MutableState<bool>,
    hovered: MutableState<bool>,
    clicks: MutableState<u32>,
    pressed_at: MutableState<Option<Point>>,
}

#[composable]
pub fn rememberFloatingInput() -> FloatingInput {
    FloatingInput {
        window: LocalWindowState::current(),
        pressed: rememberMutableStateOf(|| false),
        hovered: rememberMutableStateOf(|| false),
        clicks: rememberMutableStateOf(|| 0u32),
        pressed_at: rememberMutableStateOf(|| None::<Point>),
    }
}

impl FloatingInput {
    pub fn pressed(self) -> bool {
        self.pressed.get()
    }

    pub fn hovered(self) -> bool {
        self.hovered.get()
    }

    pub fn clicks(self) -> u32 {
        self.clicks.get()
    }
}

pub trait FloatingInputModifierExt {
    fn floating_input(self, input: FloatingInput) -> Modifier;
}

impl FloatingInputModifierExt for Modifier {
    fn floating_input(self, input: FloatingInput) -> Modifier {
        let FloatingInput {
            window,
            pressed,
            hovered,
            clicks,
            pressed_at,
        } = input;
        self.window_drag_area_with_callbacks(
            move || {
                pressed.set(true);
                pressed_at.set(window.and_then(WindowState::position));
            },
            move || {
                pressed.set(false);
                clicks.set(clicks_after_press(
                    clicks.get(),
                    pressed_at.get(),
                    window.and_then(WindowState::position),
                ));
            },
        )
        .pointer_input((), move |scope: PointerInputScope| async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Enter => hovered.set(true),
                            PointerEventKind::Exit => hovered.set(false),
                            _ => {}
                        }
                    }
                })
                .await;
        })
    }
}

#[cfg(test)]
#[path = "tests/floating_input_tests.rs"]
mod tests;
