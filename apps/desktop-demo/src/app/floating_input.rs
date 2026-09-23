use cranpose::WindowModifierExt;
use cranpose_core::{mutableStateOf, remember, MutableState};
use cranpose_ui::{composable, Modifier, PointerEventKind, PointerInputScope};

/// Whether the pointer is over a borderless window's content and holding it
/// down, and how many times it has been clicked. A press on the content drags
/// the window; letting go without dragging is a click, which is the framework's
/// own reading of a click, not this demo's.
#[derive(Clone, Copy)]
pub struct FloatingInput {
    pressed: MutableState<bool>,
    hovered: MutableState<bool>,
    clicks: MutableState<u32>,
}

impl FloatingInput {
    fn new() -> Self {
        FloatingInput {
            pressed: mutableStateOf(false),
            hovered: mutableStateOf(false),
            clicks: mutableStateOf(0u32),
        }
    }
}

#[composable]
pub fn rememberFloatingInput() -> FloatingInput {
    remember(FloatingInput::new).with(|input| *input)
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
            pressed,
            hovered,
            clicks,
        } = input;
        self.window_drag_area(|| {}, || {})
            .clickable(move |_| clicks.set(clicks.get() + 1))
            .pointer_input((), move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Enter => hovered.set(true),
                                PointerEventKind::Exit => hovered.set(false),
                                PointerEventKind::Down => pressed.set(true),
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    pressed.set(false)
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            })
    }
}
