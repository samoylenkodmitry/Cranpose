use crate::scroll::{scroll_motion_context_for_key, ScrollMotionContextKey};
use crate::{
    collect_modifier_slices, measure_layout, Column, ColumnSpec, LayoutEngine, LazyColumn,
    LazyColumnSpec, Modifier, ScrollState, Size, Spacer,
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

/// Dispatches an event leaf-first (child, then parent), mirroring the
/// shell's hit-path dispatch order for nested scrollables.
fn dispatch_nested(
    child: &Rc<dyn Fn(PointerEvent)>,
    parent: &Rc<dyn Fn(PointerEvent)>,
    event: PointerEvent,
) -> PointerEvent {
    child(event.clone());
    parent(event.clone());
    event
}

#[test]
fn horizontal_drag_with_jitter_scrolls_nested_horizontal_not_vertical_parent() {
    // A chips row (horizontal scroll) nested in a screen list (vertical
    // scroll): a mostly-horizontal drag with vertical jitter must scroll the
    // chips row by the horizontal drag distance and leave the screen alone.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let vertical = ScrollState::new(100.0);
        vertical.set_max_value(400.0);
        let (child, _child_chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal.clone(), false));
        let (parent, _parent_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(vertical.clone(), false));

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0),
        );
        // Below the slop on both axes: nobody captures yet.
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 106.0, 96.0),
        );
        // dx = 14 crosses the slop and dominates dy = 6: the child captures.
        let capture = dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 114.0, 106.0),
        );
        assert!(
            capture.is_consumed(),
            "the horizontal child must capture a mostly-horizontal drag"
        );
        // Keep dragging right with vertical jitter.
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 140.0, 103.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 170.0, 97.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Up, 170.0, 97.0),
        );

        // Content follows the finger from the position preceding the
        // capturing move (106): 170 - 106 = 64 to the right.
        assert!(
            (horizontal.value_non_reactive() - 36.0).abs() < 1e-3,
            "the horizontal scrollable must scroll by the horizontal drag \
             distance (100 - 64 = 36), got {}",
            horizontal.value_non_reactive()
        );
        assert_eq!(
            vertical.value_non_reactive(),
            100.0,
            "the vertical parent must not steal a mostly-horizontal drag"
        );
    });
}

#[test]
fn vertical_drag_with_horizontal_jitter_scrolls_parent_not_nested_child() {
    // The same nesting dragged mostly vertically, with sideways jitter that
    // crosses the slop: the vertical parent must win even though the
    // horizontal child sees every event first.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let vertical = ScrollState::new(100.0);
        vertical.set_max_value(400.0);
        let (child, _child_chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal.clone(), false));
        let (parent, _parent_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(vertical.clone(), false));

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0),
        );
        // dx = 12 crosses the slop too, but dy = 30 dominates: the child must
        // decline and the parent must capture.
        let capture = dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 112.0, 130.0),
        );
        assert!(
            capture.is_consumed(),
            "the vertical parent must capture a mostly-vertical drag"
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 115.0, 170.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Up, 115.0, 170.0),
        );

        assert_eq!(
            horizontal.value_non_reactive(),
            100.0,
            "the horizontal child must not steal a mostly-vertical drag"
        );
        // Finger moved down by 70 from the down position: content follows.
        assert!(
            (vertical.value_non_reactive() - 30.0).abs() < 1e-3,
            "the vertical parent must scroll by the vertical drag distance \
             (100 - 70 = 30), got {}",
            vertical.value_non_reactive()
        );
    });
}

#[test]
fn cross_axis_drag_locks_scrollable_out_for_the_rest_of_the_gesture() {
    // A drag that decisively starts vertical must never be captured by a
    // horizontal scrollable later in the gesture, even when its horizontal
    // component eventually exceeds the vertical one.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal.clone(), false));

        handler(scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0));
        // Decisively vertical: dy = 20 crosses the slop while dx = 4.
        handler(scroll_pointer_event(PointerEventKind::Move, 104.0, 120.0));
        // The drag drifts sideways: dx = 60 now exceeds dy = 25.
        let late_move = scroll_pointer_event(PointerEventKind::Move, 160.0, 125.0);
        handler(late_move.clone());
        handler(scroll_pointer_event(PointerEventKind::Up, 160.0, 125.0));

        assert_eq!(
            horizontal.value_non_reactive(),
            100.0,
            "a locked-out scrollable must stay inert for the whole gesture"
        );
        assert!(
            !late_move.is_consumed(),
            "a locked-out scrollable must not consume later moves"
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

// ─────────────────────────────────────────────────────────────────────────────
// Android batched input delivery regression tests.
//
// Android delivers MotionEvents batched/frame-aligned: several touch samples
// (with their own timestamps) are processed back-to-back in one main-loop
// iteration. Velocity must be computed from the events' timestamps, NOT the
// delivery time, otherwise dt collapses to ~1ms per sample and every release
// produces a near-MAX_FLING_VELOCITY fling whose direction is dominated by
// release jitter ("fling gestures act almost randomly").
// ─────────────────────────────────────────────────────────────────────────────

fn timed_pointer_event(kind: PointerEventKind, x: f32, y: f32, time_ms: i64) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
        .with_buttons(primary_buttons())
        .with_time_ms(Some(time_ms))
}

#[test]
fn batched_touch_delivery_computes_real_finger_velocity() {
    // Finger moves down 8dp every 8ms => real velocity is exactly 1000 dp/s.
    // All events are handed over back-to-back (delivery dt ~ 0), mimicking
    // Android's batched frame-aligned input delivery.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        let t0 = 1_234_567i64; // arbitrary uptime base
        handler(timed_pointer_event(PointerEventKind::Down, 0.0, 100.0, t0));
        for i in 1..=12i64 {
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                100.0 + i as f32 * 8.0,
                t0 + i * 8,
            ));
        }
        handler(timed_pointer_event(
            PointerEventKind::Up,
            0.0,
            196.0,
            t0 + 104,
        ));

        let velocity = crate::render_state::debug_last_fling_velocity();
        assert!(
            velocity > 0.0,
            "finger moving down must produce a downward (positive) velocity, got {velocity}"
        );
        assert!(
            (800.0..=1200.0).contains(&velocity),
            "batched delivery must not distort the real ~1000 dp/s finger velocity, got {velocity}"
        );
    });
}

#[test]
fn release_jitter_does_not_reverse_or_inflate_fling() {
    // Steady 1000 dp/s downward drag, but the finger rolls back 2dp right at
    // release (very common on lift-off). The computed fling must stay in the
    // finger's travel direction with a sane magnitude.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        let t0 = 987_654i64;
        handler(timed_pointer_event(PointerEventKind::Down, 0.0, 100.0, t0));
        let mut y = 100.0;
        for i in 1..=12i64 {
            y += 8.0;
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                y,
                t0 + i * 8,
            ));
        }
        // Lift-off jitter: 2dp back, one input period later.
        handler(timed_pointer_event(
            PointerEventKind::Move,
            0.0,
            y - 2.0,
            t0 + 13 * 8,
        ));
        handler(timed_pointer_event(
            PointerEventKind::Up,
            0.0,
            y - 2.0,
            t0 + 13 * 8,
        ));

        let velocity = crate::render_state::debug_last_fling_velocity();
        assert!(
            velocity > 0.0,
            "small release jitter must not reverse the fling direction, got {velocity}"
        );
        assert!(
            velocity <= 1200.0,
            "release jitter must not inflate the fling velocity, got {velocity}"
        );
    });
}

#[test]
fn batched_same_millisecond_samples_do_not_explode_velocity() {
    // Two historical samples of one batched MotionEvent can land in the same
    // millisecond once truncated to ms. The tracker must tolerate equal
    // timestamps without producing huge/NaN velocities.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

        let t0 = 500_000i64;
        handler(timed_pointer_event(PointerEventKind::Down, 0.0, 100.0, t0));
        let mut y = 100.0;
        for i in 1..=6i64 {
            y += 8.0;
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                y,
                t0 + i * 8,
            ));
            // duplicate-timestamp sample (ms truncation of a 120Hz pair)
            y += 0.5;
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                y,
                t0 + i * 8,
            ));
        }
        handler(timed_pointer_event(
            PointerEventKind::Up,
            0.0,
            y,
            t0 + 7 * 8,
        ));

        let velocity = crate::render_state::debug_last_fling_velocity();
        assert!(
            velocity.is_finite(),
            "velocity must be finite, got {velocity}"
        );
        assert!(
            velocity > 0.0 && velocity < 2500.0,
            "same-ms batched samples must not explode the velocity, got {velocity}"
        );
    });
}

#[test]
fn lazy_list_fling_velocity_uses_event_timestamps() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(remember_lazy_list_state());
        });
        let list_state = list_state.expect("lazy list state should be created");

        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));

        let t0 = 42_000i64;
        handler(timed_pointer_event(PointerEventKind::Down, 0.0, 400.0, t0));
        // Finger up (towards later items) 8dp every 8ms => -1000 dp/s.
        for i in 1..=12i64 {
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                400.0 - i as f32 * 8.0,
                t0 + i * 8,
            ));
        }
        handler(timed_pointer_event(
            PointerEventKind::Up,
            0.0,
            304.0,
            t0 + 104,
        ));

        let velocity = crate::render_state::debug_last_fling_velocity();
        assert!(
            velocity < 0.0,
            "finger moving up must produce a negative gesture velocity, got {velocity}"
        );
        assert!(
            (800.0..=1200.0).contains(&velocity.abs()),
            "lazy fling velocity must match the ~1000 dp/s finger speed, got {velocity}"
        );
    });
}

#[test]
fn three_consecutive_flings_compute_the_same_velocity_sign() {
    // Device report: "several flings in one direction — one of them wrongly
    // goes to the opposite direction". This locks the detector-level
    // invariants for consecutive gestures:
    // - velocity samples of the previous gesture never leak into the next one
    //   (the tracker is reset on pointer down), and
    // - the fling animation still running from the previous gesture never
    //   mixes its decaying velocity into the newly computed one.
    //
    // Realistic Android timing: 8ms input samples, ~104ms gesture, ~300ms
    // between gestures with the previous fling still animating (spline decay
    // of a ~1000 dp/s fling lasts >500ms) when the next finger lands.
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    crate::render_state::debug_reset_last_fling_velocity();

    let scroll_state = ScrollState::new(5_000.0);
    scroll_state.set_max_value(100_000.0);
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));

    let mut event_ms = 7_777_000i64; // arbitrary uptime base
    let mut frame_ns = 0u64;
    let mut velocities = Vec::new();

    for gesture in 0..3 {
        handler(timed_pointer_event(
            PointerEventKind::Down,
            0.0,
            600.0,
            event_ms,
        ));
        // Finger flicks UP 8dp every 8ms (~ -1000 dp/s).
        let mut y = 600.0;
        for _ in 0..12 {
            event_ms += 8;
            y -= 8.0;
            handler(timed_pointer_event(
                PointerEventKind::Move,
                0.0,
                y,
                event_ms,
            ));
        }
        event_ms += 8;
        handler(timed_pointer_event(PointerEventKind::Up, 0.0, y, event_ms));

        let velocity = crate::render_state::debug_last_fling_velocity();
        velocities.push(velocity);

        // Drive ~300ms of fling frames; the fling from this gesture is still
        // animating when the next Down lands and cancels it.
        let offset_before_frames = scroll_state.value_non_reactive();
        for _ in 0..19 {
            frame_ns += 16_000_000;
            handle.drain_frame_callbacks(frame_ns);
        }
        assert!(
            scroll_state.value_non_reactive() > offset_before_frames,
            "gesture {gesture}: the fling animation must scroll further after release"
        );
        event_ms += 304 - 104;
    }

    assert!(
        velocities.iter().all(|v| *v < 0.0),
        "three identical upward flicks must all compute negative velocities, got {velocities:?}"
    );
    let reference = velocities[0];
    for (gesture, velocity) in velocities.iter().enumerate() {
        assert!(
            (velocity - reference).abs() <= reference.abs() * 0.05,
            "gesture {gesture}: identical gestures must compute identical velocities \
             (previous gesture/fling state leaked in), got {velocities:?}"
        );
    }
}

#[test]
fn vertical_scroll_box_bottom_reachable_at_fractional_density() {
    // Phone-like geometry: 2280 physical px viewport at density 2.75 and item
    // heights that are whole physical pixels (177px) but fractional in dp.
    // The bottom of the content must remain exactly reachable.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let density = 2.75f32;
        let viewport_dp = 2280.0 / density; // 829.0909...
        let item_dp = 177.0 / density; // 64.3636...
        let item_count = 30usize;

        let scroll_state = ScrollState::new(0.0);
        let scroll_state_for_content = scroll_state.clone();
        let mut composition = crate::run_test_composition(move || {
            Column(
                Modifier::empty().vertical_scroll(scroll_state_for_content.clone(), false),
                ColumnSpec::default(),
                move || {
                    for _ in 0..item_count {
                        Spacer(Size {
                            width: 100.0,
                            height: item_dp,
                        });
                    }
                },
            );
        });

        let root = composition.root().expect("scroll column root");
        let handle = composition.runtime_handle();
        {
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            measure_layout(
                &mut applier,
                root,
                cranpose_ui_graphics::Size {
                    width: 320.0,
                    height: viewport_dp,
                },
            )
            .expect("layout measurement");
            applier.clear_runtime_handle();
        }

        let content_dp = item_dp * item_count as f32;
        let expected_max = content_dp - viewport_dp;
        assert!(
            (scroll_state.max_value() - expected_max).abs() < 0.01,
            "max scroll must equal content minus viewport ({expected_max}), got {}",
            scroll_state.max_value()
        );

        // Drag to the bottom with realistic touch gestures (finger up 200dp per
        // gesture, 8ms-per-8dp samples) until the offset stops moving.
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state.clone(), false));
        let mut time = 10_000i64;
        let mut last_value = -1.0f32;
        let mut gestures = 0;
        while (scroll_state.value_non_reactive() - last_value).abs() > 0.001 && gestures < 100 {
            last_value = scroll_state.value_non_reactive();
            handler(timed_pointer_event(
                PointerEventKind::Down,
                0.0,
                700.0,
                time,
            ));
            let mut y = 700.0;
            for _ in 0..25 {
                time += 8;
                y -= 8.0;
                handler(timed_pointer_event(PointerEventKind::Move, 0.0, y, time));
            }
            // Rest before release so no fling is started.
            time += 200;
            handler(timed_pointer_event(PointerEventKind::Up, 0.0, y, time));
            time += 100;
            gestures += 1;
        }

        assert!(
            (scroll_state.value_non_reactive() - scroll_state.max_value()).abs() < 0.001,
            "bottom must be reachable: value {} vs max {}",
            scroll_state.value_non_reactive(),
            scroll_state.max_value()
        );
    });
}

#[test]
fn drags_over_fully_realized_lazy_list_scroll_the_outer_container() {
    // Device repro: LazyColumn (with one item taller than the screen) nested
    // in a vertical_scroll Box. The unbounded inner list is realized in full
    // and cannot scroll itself, so its gesture detector must NOT capture
    // drags — the outer scrollable has to receive them and reach the true
    // bottom of the content.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(remember_lazy_list_state());
        });
        let list_state = list_state.expect("lazy list state should be created");

        // Simulate the unbounded measure result: all 15 items realized
        // (10x50 + 800 + 4x50 = 1500 content), no internal scrollability.
        cranpose_foundation::lazy::measure_lazy_list(
            15,
            &list_state,
            f32::INFINITY,
            320.0,
            &cranpose_foundation::lazy::LazyListMeasureConfig::default(),
            |index| {
                cranpose_foundation::lazy::LazyListMeasuredItem::new(
                    index,
                    index as u64,
                    None,
                    if index == 10 { 800.0 } else { 50.0 },
                    320.0,
                )
            },
        );
        assert!(!list_state.can_scroll_forward_non_reactive());

        // Outer vertical_scroll Box: viewport 500, content 1500 => max 1000.
        let outer_state = ScrollState::new(0.0);
        outer_state.set_max_value(1000.0);

        let (inner_handler, _inner_chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));
        let (outer_handler, _outer_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(outer_state.clone(), false));

        // The shell dispatches gesture events along the hit path innermost
        // first without stopping on consumption for moves; replicate that.
        let dispatch = |event: PointerEvent| {
            inner_handler(event.clone());
            outer_handler(event);
        };

        let mut time = 50_000i64;
        let mut gestures = 0;
        let mut last_value = -1.0f32;
        while (outer_state.value_non_reactive() - last_value).abs() > 0.001 && gestures < 50 {
            last_value = outer_state.value_non_reactive();
            dispatch(timed_pointer_event(
                PointerEventKind::Down,
                10.0,
                450.0,
                time,
            ));
            let mut y = 450.0;
            for _ in 0..35 {
                time += 8;
                y -= 8.0;
                dispatch(timed_pointer_event(PointerEventKind::Move, 10.0, y, time));
            }
            time += 200; // rest so release does not fling
            dispatch(timed_pointer_event(PointerEventKind::Up, 10.0, y, time));
            time += 100;
            gestures += 1;
        }

        assert!(
            (outer_state.value_non_reactive() - 1000.0).abs() < 0.001,
            "outer scroll must reach the true bottom (1000); stopped at {} after {} gestures — \
             the non-scrollable inner lazy list must not swallow drag gestures",
            outer_state.value_non_reactive(),
            gestures
        );
    });
}
