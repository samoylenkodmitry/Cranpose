use crate::scroll::{scroll_motion_context_for_key, ScrollMotionContextKey};
use crate::{collect_modifier_slices, Modifier, ScrollState};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerEvent, PointerEventKind,
};
use cranpose_ui_graphics::Point;
use std::cell::Cell;
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
fn scroll_invalidation_callback_ids_are_instance_owned() {
    let source = include_str!("../scroll.rs");
    let static_callback = ["static ", "NEXT_CALLBACK_ID"].concat();
    let static_state_id = ["static ", "NEXT_SCROLL_STATE_ID"].concat();

    assert!(
        !source.contains(&static_callback) && !source.contains(&static_state_id),
        "scroll ids must be owned by scroll state/context instances"
    );
}

#[test]
fn scroll_state_id_uses_retained_instance_identity() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let first = ScrollState::new(0.0);
        let first_clone = first.clone();
        let second = ScrollState::new(0.0);

        assert_ne!(first.id(), 0);
        assert_eq!(first.id(), first_clone.id());
        assert_ne!(first.id(), second.id());
    });
}

#[test]
fn scroll_invalidation_callback_ids_restart_per_instance() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let first = ScrollState::new(0.0);
        let second = ScrollState::new(0.0);

        assert_eq!(first.add_invalidate_callback(Box::new(|| {})), 1);
        assert_eq!(first.add_invalidate_callback(Box::new(|| {})), 2);
        assert_eq!(second.add_invalidate_callback(Box::new(|| {})), 1);
    });
}

#[test]
fn scroll_motion_callback_ids_restart_per_instance() {
    let first = crate::scroll::ScrollMotionContext::new();
    let second = crate::scroll::ScrollMotionContext::new();

    assert_eq!(first.add_invalidate_callback(Box::new(|| {})), 1);
    assert_eq!(first.add_invalidate_callback(Box::new(|| {})), 2);
    assert_eq!(second.add_invalidate_callback(Box::new(|| {})), 1);
}

fn scroll_wheel_event(dx: f32, dy: f32) -> PointerEvent {
    PointerEvent::new(
        PointerEventKind::Scroll,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    )
    .with_scroll_delta(Point { x: dx, y: dy })
}

#[test]
fn vertical_scroll_clips_to_bounds_by_default() {
    let _app_context = crate::render_state::app_context_test_scope();
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        let event = scroll_wheel_event(0.0, 48.0);

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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(scroll_state.clone(), false));

        let event = scroll_wheel_event(30.0, 120.0);

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

#[test]
fn scroll_state_invalidation_callback_can_register_follow_up_callback() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(0.0);
        scroll_state.set_max_value(100.0);
        let follow_up_called = Rc::new(Cell::new(false));
        let state_for_callback = scroll_state.clone();
        let follow_up_for_callback = Rc::clone(&follow_up_called);

        scroll_state.add_invalidate_callback(Box::new(move || {
            let follow_up = Rc::clone(&follow_up_for_callback);
            state_for_callback.add_invalidate_callback(Box::new(move || {
                follow_up.set(true);
            }));
        }));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scroll_state.dispatch_raw_delta(10.0);
        }));
        assert!(
            result.is_ok(),
            "scroll invalidation callbacks must be able to register follow-up callbacks"
        );

        scroll_state.dispatch_raw_delta(1.0);
        assert!(follow_up_called.get());
    });
}

#[test]
fn scroll_motion_invalidation_callback_can_register_follow_up_callback() {
    let _app_context = crate::render_state::app_context_test_scope();
    let key = ScrollMotionContextKey::ScrollState {
        state_id: 7,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let context = scroll_motion_context_for_key(key);
    let context_for_callback = context.clone();
    let follow_up_called = Rc::new(Cell::new(false));
    let follow_up_for_callback = Rc::clone(&follow_up_called);

    context.add_invalidate_callback(Box::new(move || {
        let follow_up = Rc::clone(&follow_up_for_callback);
        context_for_callback.add_invalidate_callback(Box::new(move || {
            follow_up.set(true);
        }));
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        context.set_active(true);
    }));
    assert!(
        result.is_ok(),
        "scroll motion invalidation callbacks must be able to register follow-up callbacks"
    );

    context.set_active(false);
    assert!(follow_up_called.get());
}

#[test]
fn scroll_motion_contexts_are_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);
    let key = ScrollMotionContextKey::ScrollState {
        state_id: 42,
        is_vertical: true,
        reverse_scrolling: false,
    };

    let first_context = first.enter(|| scroll_motion_context_for_key(key));
    let first_again = first.enter(|| scroll_motion_context_for_key(key));
    let second_context = second.enter(|| scroll_motion_context_for_key(key));

    assert!(
        first_context.ptr_eq(&first_again),
        "same app context should reuse motion contexts for a stable scroll key"
    );
    assert!(
        !first_context.ptr_eq(&second_context),
        "separate app contexts must not share scroll motion contexts"
    );
}

#[test]
fn scroll_motion_context_survives_modifier_recomposition_with_stale_pointer_task() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        let first = Modifier::empty().vertical_scroll(scroll_state.clone(), false);
        chain.update_from_slice(&first.elements(), &mut context);
        let stale_handler = collect_modifier_slices(&chain)
            .pointer_inputs()
            .first()
            .cloned()
            .expect("scroll modifier should provide pointer input handler");

        let recomposed = Modifier::empty().vertical_scroll(scroll_state.clone(), false);
        chain.update_from_slice(&recomposed.elements(), &mut context);
        assert!(
            !collect_modifier_slices(&chain).motion_context_animated(),
            "scroll motion should start inactive after recomposition"
        );

        let event = scroll_wheel_event(0.0, 48.0);
        stale_handler(event.clone());

        assert!(event.is_consumed(), "wheel event should still scroll");
        assert!(
            collect_modifier_slices(&chain).motion_context_animated(),
            "the active pointer task and recomposed render policy must share one scroll motion context"
        );
    });
}

#[test]
fn lazy_scroll_motion_context_survives_modifier_recomposition_with_stale_pointer_task() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(cranpose_foundation::lazy::remember_lazy_list_state());
        });
        let list_state = list_state.expect("lazy list state should be created");

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        let first = Modifier::empty().lazy_vertical_scroll(list_state, false);
        chain.update_from_slice(&first.elements(), &mut context);
        let stale_handler = collect_modifier_slices(&chain)
            .pointer_inputs()
            .first()
            .cloned()
            .expect("lazy scroll modifier should provide pointer input handler");

        let recomposed = Modifier::empty().lazy_vertical_scroll(list_state, false);
        chain.update_from_slice(&recomposed.elements(), &mut context);
        assert!(
            !collect_modifier_slices(&chain).motion_context_animated(),
            "lazy scroll motion should start inactive after recomposition"
        );

        let event = scroll_wheel_event(0.0, -48.0);
        stale_handler(event.clone());

        assert!(event.is_consumed(), "wheel event should still scroll");
        assert!(
            collect_modifier_slices(&chain).motion_context_animated(),
            "the active lazy-list pointer task and recomposed render policy must share one scroll motion context"
        );
    });
}
