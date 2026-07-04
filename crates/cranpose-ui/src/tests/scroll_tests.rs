use crate::scroll::{scroll_motion_context_for_key, ScrollMotionContextKey};
use crate::{
    collect_modifier_slices, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, ScrollState, Size,
    Spacer,
};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    lazy::{remember_lazy_list_state, LazyListScope},
    BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind,
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

fn primary_buttons() -> PointerButtons {
    PointerButtons::new().with(PointerButton::Primary)
}

fn scroll_pointer_event(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y }).with_buttons(primary_buttons())
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
fn vertical_scroll_ignores_move_consumed_by_child_drag() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 0.0));
        let consumed_move = scroll_pointer_event(PointerEventKind::Move, 0.0, 32.0);
        consumed_move.consume();
        handler(consumed_move);

        assert_eq!(
            scroll_state.value_non_reactive(),
            100.0,
            "parent scroll must not process a move event already consumed by a child drag"
        );
    });
}

#[test]
fn touch_drag_moves_content_with_the_finger() {
    // Touch semantics: content must FOLLOW the finger.
    // Dragging the finger UP (y decreases) moves content up, revealing content
    // further down, i.e. the scroll offset must INCREASE by the drag distance.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
        // Finger moves up by 60px (beyond the 8px drag threshold).
        handler(scroll_pointer_event(PointerEventKind::Move, 0.0, 40.0));
        handler(scroll_pointer_event(PointerEventKind::Up, 0.0, 40.0));

        assert!(
            (scroll_state.value_non_reactive() - 160.0).abs() < 0.001,
            "finger up by 60 must increase scroll offset by 60 (content follows finger), got {}",
            scroll_state.value_non_reactive()
        );
    });
}

#[test]
fn touch_drag_down_moves_content_down() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 40.0));
        // Finger moves down by 60px: content follows, scroll offset decreases.
        handler(scroll_pointer_event(PointerEventKind::Move, 0.0, 100.0));
        handler(scroll_pointer_event(PointerEventKind::Up, 0.0, 100.0));

        assert!(
            (scroll_state.value_non_reactive() - 40.0).abs() < 0.001,
            "finger down by 60 must decrease scroll offset by 60 (content follows finger), got {}",
            scroll_state.value_non_reactive()
        );
    });
}

#[test]
fn lazy_touch_drag_up_scrolls_toward_later_items() {
    // Finger up => content up => later items become visible => the pending
    // lazy scroll delta must be negative (dispatch_scroll_delta(<0) scrolls forward).
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(remember_lazy_list_state());
        });
        let list_state = list_state.expect("lazy list state should be created");

        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
        handler(scroll_pointer_event(PointerEventKind::Move, 0.0, 40.0));
        handler(scroll_pointer_event(PointerEventKind::Up, 0.0, 40.0));

        let pending = list_state.peek_scroll_delta();
        assert!(
            (pending + 60.0).abs() < 0.001,
            "finger up by 60 must queue a forward (-60) lazy scroll delta, got {pending}"
        );
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
fn wheel_scroll_motion_context_clear_invalidates_modifier_slices() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let key = ScrollMotionContextKey::ScrollState {
        state_id: 7,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let context = scroll_motion_context_for_key(key);
    let invalidations = Rc::new(Cell::new(0_u32));
    let invalidations_for_callback = Rc::clone(&invalidations);
    context.add_invalidate_callback(Box::new(move || {
        invalidations_for_callback.set(invalidations_for_callback.get().saturating_add(1));
    }));

    context.activate_for_current_frame();

    assert!(context.is_active());
    assert_eq!(invalidations.get(), 1);
    runtime.handle().drain_frame_callbacks(1);
    assert!(
        context.is_active(),
        "wheel scroll motion must not register a clear callback that schedules another frame"
    );

    crate::render_state::clear_transient_scroll_motion_contexts();

    assert!(!context.is_active());
    assert_eq!(
        invalidations.get(),
        2,
        "clearing transient wheel motion must rebuild modifier slices so retained scenes stop treating rested scroll content as animated"
    );
}

#[test]
fn scroll_motion_frame_clear_preserves_persistent_gesture_motion() {
    let _app_context = crate::render_state::app_context_test_scope();
    let key = ScrollMotionContextKey::ScrollState {
        state_id: 8,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let context = scroll_motion_context_for_key(key);

    context.set_active(true);
    context.activate_for_current_frame();
    crate::render_state::clear_transient_scroll_motion_contexts();

    assert!(
        context.is_active(),
        "frame-boundary transient clear must not end drag/fling motion"
    );

    context.set_active(false);
    assert!(!context.is_active());
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

#[test]
fn lazy_wheel_scroll_preserves_input_delta_after_viewport_measurement() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let measure_wheel_delta = |list_height: f32, root_height: f32, wheel_delta: f32| {
            let mut list_state = None;
            let mut composition = crate::run_test_composition(|| {
                let state = remember_lazy_list_state();
                list_state = Some(state);
                LazyColumn(
                    Modifier::empty().fill_max_width().height(list_height),
                    state,
                    LazyColumnSpec::default(),
                    |scope| {
                        scope.items(
                            100,
                            None::<fn(usize) -> u64>,
                            None::<fn(usize) -> u64>,
                            |_| {
                                Spacer(Size {
                                    width: 0.0,
                                    height: 48.0,
                                });
                            },
                        );
                    },
                );
            });
            let list_state = list_state.expect("lazy list state should be created");
            let root = composition.root().expect("lazy list root");
            let handle = composition.runtime_handle();
            {
                let mut applier = composition.applier_mut();
                applier.set_runtime_handle(handle.clone());
                let _ = applier
                    .compute_layout(
                        root,
                        Size {
                            width: 320.0,
                            height: root_height,
                        },
                    )
                    .expect("layout");
                applier.clear_runtime_handle();
            }
            assert!(
                list_state.layout_info().viewport_size > 0.0,
                "lazy list must have measured viewport before wheel budgeting"
            );

            let (handler, _chain) =
                pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));
            let event = scroll_wheel_event(0.0, wheel_delta);
            handler(event.clone());
            assert!(event.is_consumed(), "wheel event should scroll lazy list");
            list_state.peek_scroll_delta()
        };

        let ordinary_delta = measure_wheel_delta(240.0, 260.0, -32.0);
        assert!(
            (ordinary_delta + 32.0).abs() < 0.001,
            "ordinary lazy wheel delta must not be downscaled"
        );

        let bounded_small_viewport_delta = measure_wheel_delta(240.0, 260.0, -620.0);
        assert!(
            (bounded_small_viewport_delta + 620.0).abs() < 0.001,
            "large lazy wheel delta must not be downscaled"
        );

        let bounded_large_viewport_delta = measure_wheel_delta(800.0, 820.0, -620.0);
        assert!(
            (bounded_large_viewport_delta + 620.0).abs() < 0.001,
            "large lazy wheel delta must not be downscaled on tall viewports"
        );
    });
}
