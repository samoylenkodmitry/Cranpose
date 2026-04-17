use crate::{collect_modifier_slices, Modifier, ScrollState};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerEvent, PointerEventKind,
};
use cranpose_ui_graphics::Point;
use std::rc::Rc;
use std::sync::Arc;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

/// Returns (handler, chain). The chain must be kept alive while the handler
/// is used — dropping it cancels the underlying pointer input task.
fn pointer_handler_for(modifier: Modifier) -> (Rc<dyn Fn(PointerEvent)>, ModifierNodeChain) {
    let elements = modifier.elements();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_modifier_slices(&chain);
    let handler = slices
        .pointer_inputs()
        .first()
        .cloned()
        .expect("scroll modifier should provide pointer input handler");
    (handler, chain)
}

#[test]
fn vertical_scroll_clips_to_bounds_by_default() {
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(0.0);
        let modifier = Modifier::empty().vertical_scroll(scroll_state, false);
        let elements = modifier.elements();

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        chain.update_from_slice(&elements, &mut context);

        let slices = collect_modifier_slices(&chain);
        assert!(slices.clip_to_bounds());
    });
}

#[test]
fn wheel_scroll_updates_vertical_scroll_state() {
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        let event = PointerEvent::new(
            PointerEventKind::Scroll,
            Point { x: 0.0, y: 0.0 },
            Point { x: 0.0, y: 0.0 },
        )
        .with_scroll_delta(Point { x: 0.0, y: 48.0 });

        handler(event.clone());

        assert!(
            event.is_consumed(),
            "wheel event should be consumed by scrollable modifier"
        );
        assert!(
            (scroll_state.value_non_reactive() - 52.0).abs() < 0.001,
            "vertical wheel delta should move vertical scroll state"
        );
    });
}

#[test]
fn wheel_scroll_uses_horizontal_delta_for_horizontal_scroll() {
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(scroll_state.clone(), false));

        let event = PointerEvent::new(
            PointerEventKind::Scroll,
            Point { x: 0.0, y: 0.0 },
            Point { x: 0.0, y: 0.0 },
        )
        .with_scroll_delta(Point { x: 30.0, y: 120.0 });

        handler(event.clone());

        assert!(
            event.is_consumed(),
            "wheel event should be consumed by horizontal scrollable modifier"
        );
        assert!(
            (scroll_state.value_non_reactive() - 70.0).abs() < 0.001,
            "horizontal scroll should use horizontal wheel component"
        );
    });
}
