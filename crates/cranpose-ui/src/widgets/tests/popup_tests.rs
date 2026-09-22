use cranpose_foundation::PointerEvent;

use super::*;
use crate::modifier::collect_slices_from_modifier;

#[test]
fn refreshing_popup_content_does_not_invalidate_the_popup_list() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
    composition
        .render(
            cranpose_core::location_key(file!(), line!(), column!()),
            || {
                let registry = PopupRegistry::hosted();
                let id = registry.allocate_id();
                registry.upsert(id, Point::default(), Rc::new(|| {}), Some(Rc::new(|| {})));
                let revision = registry.inner.revision.as_ref().expect("hosted revision");
                let before = revision.get_non_reactive();
                let calls = Rc::new(Cell::new(0));
                let content_calls = calls.clone();
                let dismiss_calls = calls.clone();
                registry.upsert(
                    id,
                    Point::new(10.0, 20.0),
                    Rc::new(move || content_calls.set(content_calls.get() + 1)),
                    Some(Rc::new(move || dismiss_calls.set(dismiss_calls.get() + 10))),
                );
                assert_eq!(
                    revision.get_non_reactive(),
                    before,
                    "only the changed popup needs recomposition"
                );
                let entries = registry.snapshot();
                assert_eq!(entries.len(), 1);
                let updated = entries[0].data.get_non_reactive();
                assert_eq!(updated.position, Point::new(10.0, 20.0));
                (updated.content)();
                assert_eq!(calls.get(), 1);
                (updated.on_dismiss.expect("updated dismiss callback"))();
                assert_eq!(calls.get(), 11);
                registry.upsert(id, Point::default(), Rc::new(|| {}), None);
                assert_eq!(revision.get_non_reactive(), before + 1);
                registry.remove(id);
                assert_eq!(revision.get_non_reactive(), before + 2);
            },
        )
        .expect("composition");
}

#[test]
fn non_dismissable_exit_frame_has_no_modal_scrim_callback() {
    let callback: Rc<dyn Fn()> = Rc::new(|| {});
    assert!(popup_dismiss_callback(false, Rc::clone(&callback)).is_none());
    assert!(popup_dismiss_callback(true, callback).is_some());
}

#[test]
fn dismiss_scrim_consumes_the_whole_tap_before_dismissing() {
    let _app_context = crate::render_state::app_context_test_scope();
    let dismissed = Rc::new(Cell::new(false));
    let action: Rc<dyn Fn()> = {
        let dismissed = Rc::clone(&dismissed);
        Rc::new(move || dismissed.set(true))
    };
    let modifier = popup_scrim_pointer_input(7, action);
    let slices = collect_slices_from_modifier(&modifier);
    assert_eq!(slices.pointer_inputs().len(), 1);
    let handler = slices.pointer_inputs()[0].clone();

    let event = |kind| PointerEvent::new(kind, Point::new(12.0, 18.0), Point::new(12.0, 18.0));
    let down = event(PointerEventKind::Down);
    handler(down.clone());
    assert!(
        down.is_consumed(),
        "covered controls must never receive Down"
    );
    assert!(!dismissed.get(), "dismissal fires on release");

    let up = event(PointerEventKind::Up);
    handler(up.clone());
    assert!(up.is_consumed(), "the release stays inside the scrim");
    assert!(
        dismissed.get(),
        "a completed outside tap dismisses the popup"
    );
    for terminal in [PointerEventKind::Up, PointerEventKind::Cancel] {
        dismissed.set(false);
        handler(event(PointerEventKind::Down));
        let handled = event(terminal);
        handled.consume();
        handler(handled);
        assert!(
            !dismissed.get(),
            "a handled or cancelled press stays inside the popup"
        );
    }
}
