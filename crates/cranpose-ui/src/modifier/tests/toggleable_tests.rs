use std::cell::Cell;

use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};

use super::*;
use crate::modifier::{Point, collect_semantics_from_modifier, collect_slices_from_modifier};

fn tap(modifier: &Modifier) {
    let slices = collect_slices_from_modifier(modifier);
    let handlers = slices.pointer_inputs();
    assert_eq!(handlers.len(), 1, "toggleable takes pointer input once");
    let at = Point { x: 4.0, y: 4.0 };
    for kind in [PointerEventKind::Down, PointerEventKind::Up] {
        let mut event = PointerEvent::new(kind, at, at);
        event.buttons = PointerButtons::new().with(PointerButton::Primary);
        handlers[0](event);
    }
}

#[test]
fn a_click_reports_the_new_value_not_the_old_one() {
    let _app_context = crate::render_state::app_context_test_scope();
    for start in [false, true] {
        let seen: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
        let sink = seen.clone();
        let modifier =
            Modifier::empty().toggleable(start, None, None, move |next| sink.set(Some(next)));
        tap(&modifier);
        assert_eq!(
            seen.get(),
            Some(!start),
            "a toggle hands over the value it is moving to"
        );
    }
}

#[test]
fn a_toggleable_row_reads_as_clickable_and_carries_its_description() {
    let modifier =
        Modifier::empty().toggleable(true, Some("Haptics, on".to_string()), None, |_| {});
    let semantics =
        collect_semantics_from_modifier(&modifier).expect("a toggleable row publishes semantics");
    assert!(semantics.is_clickable);
    assert_eq!(
        semantics.content_description.as_deref(),
        Some("Haptics, on")
    );
    assert_eq!(semantics.toggled, Some(true));
    assert_eq!(semantics.role, None);
}

#[test]
fn a_role_reaches_the_semantics_so_a_reader_can_say_what_the_control_is() {
    let modifier = Modifier::empty().toggleable(
        false,
        Some("Haptics, off".to_string()),
        Some(SemanticsWidgetRole::Switch),
        |_| {},
    );
    let semantics = collect_semantics_from_modifier(&modifier).expect("semantics");
    assert_eq!(semantics.role, Some(SemanticsWidgetRole::Switch));
    assert!(semantics.is_clickable);
    assert_eq!(semantics.toggled, Some(false));
    assert_eq!(
        semantics.content_description.as_deref(),
        Some("Haptics, off")
    );
}
