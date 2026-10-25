//! The focused field reports its window bounds through the shell, so a
//! platform can place a native editor over it (Android autofill), and the
//! bounds follow the field when it moves.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{mutableStateOf, remember};
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, FocusRequester, Modifier, Rect,
    TextFieldState,
};

use crate::support_text_field::shell_with_keyboard;

#[test]
fn the_focused_field_reports_its_window_bounds_and_follows_a_move() {
    let requester = FocusRequester::new();
    let offset_slot = Rc::new(Cell::new(None));
    let (content_requester, content_offset) = (requester.clone(), Rc::clone(&offset_slot));
    let (mut shell, _keyboard) = shell_with_keyboard(move || {
        let state = remember(|| TextFieldState::new("name")).with(|state| *state);
        let offset = remember(|| mutableStateOf(40.0_f32)).with(|offset| *offset);
        content_offset.set(Some(offset));
        BasicTextFieldWithOptions(
            state,
            Modifier::empty()
                .offset(12.0, offset.get())
                .width(200.0)
                .height(48.0)
                .focus_requester(&content_requester),
            BasicTextFieldOptions::default(),
        );
    });
    assert!(
        shell.ime_field_bounds().is_none(),
        "bounds were reported with no focused field"
    );

    requester.request_focus().expect("field focus");
    shell.update();
    assert_eq!(
        shell.ime_field_bounds(),
        Some(Rect {
            x: 12.0,
            y: 40.0,
            width: 200.0,
            height: 48.0,
        })
    );

    offset_slot.get().expect("composed offset").set(90.0);
    shell.update();
    assert_eq!(
        shell.ime_field_bounds().map(|bounds| bounds.y),
        Some(90.0),
        "the bounds follow the field"
    );
}
