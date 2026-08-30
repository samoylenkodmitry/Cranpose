use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind,
    lazy::{LazyListScope, rememberLazyListState},
};
use cranpose_ui_graphics::Point;

use crate::{
    Column, ColumnSpec, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, ScrollState, Size,
    Spacer, collect_modifier_slices, measure_layout,
    scroll::{OverscrollEffect, ScrollMotionContextKey, scroll_motion_context_for_key},
};

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

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
        let first_clone = first;
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

#[test]
fn overscroll_scroll_releases_before_consuming_target_delta() {
    let effect = OverscrollEffect::new();
    effect.set_dimension(200.0);
    effect.apply_drag_delta(60.0);
    let overscrolled = effect.offset();
    assert!(
        (overscrolled - 28.326_18).abs() < 0.001,
        "rubber-band(60, 200) should be ~28.326pt, got {overscrolled}"
    );
    let target_delta = Cell::new(0.0);

    let consumed = effect.apply_to_scroll(-100.0, |delta| {
        target_delta.set(delta);
        delta
    });

    assert!(
        (target_delta.get() - (-71.673_82)).abs() < 0.001,
        "the target should absorb 100 minus the released overscroll, got {}",
        target_delta.get()
    );
    assert_eq!(effect.offset(), 0.0);
    assert_eq!(consumed, -100.0);
}

#[test]
fn overscroll_settle_stops_at_edge_without_flipping_sign() {
    let effect = OverscrollEffect::new();
    effect.apply_drag_delta(30.0);
    let initial = effect.offset();

    let consumed = effect.apply_settle_delta(-80.0);

    assert_eq!(consumed, -initial);
    assert_eq!(effect.offset(), 0.0);
    assert_eq!(effect.apply_settle_delta(-1.0), 0.0);
    assert_eq!(effect.offset(), 0.0);
}

#[test]
fn overscroll_fling_reports_target_consumption_at_edge() {
    let effect = OverscrollEffect::new();
    let target_delta = Cell::new(0.0);

    let consumed = effect.apply_to_fling(80.0, |delta| {
        target_delta.set(delta * 0.25);
        delta * 0.25
    });

    assert_eq!(target_delta.get(), 20.0);
    assert_eq!(consumed, 20.0);
    assert!(effect.offset() < 0.0);
}

#[test]
fn first_edge_drag_consumes_event_with_effect_motion() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ScrollState::new(0.0);
        state.set_max_value(100.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
        let move_event = scroll_pointer_event(PointerEventKind::Move, 0.0, 160.0);
        handler(move_event.clone());
        move_event.finish_post_dispatch();

        assert!(move_event.is_consumed());
    });
}

#[test]
fn lazy_edge_drag_updates_shared_effect() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let mut composition = crate::run_test_composition(|| {
            let state = rememberLazyListState();
            list_state = Some(state);
            LazyColumn(
                Modifier::empty().width(320.0).height(240.0),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(40, |_| {
                        Spacer(Size {
                            width: 0.0,
                            height: 48.0,
                        });
                    });
                },
            );
        });
        let state = list_state.expect("lazy list state should be created");
        let root = composition.root().expect("lazy list root");
        let handle = composition.runtime_handle();
        {
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle.clone());
            applier
                .compute_layout(
                    root,
                    Size {
                        width: 320.0,
                        height: 240.0,
                    },
                )
                .expect("layout");
            applier.clear_runtime_handle();
        }
        let _ = crate::render_state::take_measure_repass_nodes();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(state, false));
        let context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
            state_identity: state.inner_ptr() as usize,
            is_vertical: true,
            reverse_scrolling: false,
        });
        let callback_count = Rc::new(Cell::new(0_u32));
        let callback_count_for_effect = callback_count.clone();
        context
            .overscroll()
            .add_invalidate_callback(Box::new(move || {
                callback_count_for_effect.set(callback_count_for_effect.get() + 1);
            }));
        handler(scroll_pointer_event(PointerEventKind::Down, 160.0, 40.0));
        let move_event = scroll_pointer_event(PointerEventKind::Move, 160.0, 100.0);
        handler(move_event.clone());
        move_event.finish_post_dispatch();

        assert!(context.overscroll().offset() > 0.0);
        assert!(callback_count.get() > 0);
        assert!(crate::render_state::has_pending_measure_repasses());
    });
}

#[test]
fn scroll_motion_context_keeps_effect_identity_for_layout_and_gesture_owners() {
    let _app_context = crate::render_state::app_context_test_scope();
    let key = ScrollMotionContextKey::LazyList {
        state_identity: 91,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let owner = scroll_motion_context_for_key(key);
    let effect = owner.overscroll();
    let context = scroll_motion_context_for_key(key);

    assert!(effect.ptr_eq(&context.overscroll()));
}

#[test]
fn scroll_motion_context_store_reclaims_disposed_contexts() {
    let store = crate::scroll::ScrollMotionContextStore::new();
    let first_key = ScrollMotionContextKey::LazyList {
        state_identity: 92,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let second_key = ScrollMotionContextKey::LazyList {
        state_identity: 93,
        is_vertical: true,
        reverse_scrolling: false,
    };
    let first_effect = store.context_for_key(first_key).overscroll();
    drop(first_effect);
    let _second_context = store.context_for_key(second_key);

    assert_eq!(store.contexts.borrow().len(), 1);
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
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
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
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 40.0));
        handler(scroll_pointer_event(PointerEventKind::Move, 0.0, 100.0));
        handler(scroll_pointer_event(PointerEventKind::Up, 0.0, 100.0));

        assert!(
            (scroll_state.value_non_reactive() - 40.0).abs() < 0.001,
            "finger down by 60 must decrease scroll offset by 60 (content follows finger), got {}",
            scroll_state.value_non_reactive()
        );
    });
}

fn dispatch_nested(
    child: &Rc<dyn Fn(PointerEvent)>,
    parent: &Rc<dyn Fn(PointerEvent)>,
    event: PointerEvent,
) -> PointerEvent {
    child(event.clone());
    parent(event.clone());
    event.finish_post_dispatch();
    event
}

#[test]
fn exhausted_inner_scrollable_yields_the_drag_to_its_parent() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let inner = ScrollState::new(400.0);
        inner.set_max_value(400.0);
        let outer = ScrollState::new(0.0);
        outer.set_max_value(600.0);
        let (child, _child_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(inner, false));
        let (parent, _parent_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(outer, false));

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 300.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 100.0, 240.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 100.0, 180.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Up, 100.0, 180.0),
        );

        assert_eq!(
            inner.value_non_reactive(),
            400.0,
            "the exhausted inner list has nothing to consume"
        );
        assert_eq!(
            scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
                state_id: inner.id(),
                is_vertical: true,
                reverse_scrolling: false,
            })
            .overscroll()
            .offset(),
            0.0,
            "a rejected inner edge candidate must not start overscroll"
        );
        assert!(
            outer.value_non_reactive() > 50.0,
            "the enclosing page must receive the drag the inner list cannot              consume, got {}",
            outer.value_non_reactive()
        );

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 100.0, 160.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Up, 100.0, 160.0),
        );
        assert!(
            inner.value_non_reactive() < 400.0,
            "a drag back toward the inner list's range must scroll the inner              list again, got {}",
            inner.value_non_reactive()
        );
    });
}

#[test]
fn horizontal_drag_with_jitter_scrolls_nested_horizontal_not_vertical_parent() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let vertical = ScrollState::new(100.0);
        vertical.set_max_value(400.0);
        let (child, _child_chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal, false));
        let (parent, _parent_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(vertical, false));

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0),
        );
        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 106.0, 96.0),
        );
        let capture = dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Move, 114.0, 106.0),
        );
        assert!(
            capture.is_consumed(),
            "the horizontal child must capture a mostly-horizontal drag"
        );
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let vertical = ScrollState::new(100.0);
        vertical.set_max_value(400.0);
        let (child, _child_chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal, false));
        let (parent, _parent_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(vertical, false));

        dispatch_nested(
            &child,
            &parent,
            scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0),
        );
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let horizontal = ScrollState::new(100.0);
        horizontal.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll(horizontal, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 100.0, 100.0));
        handler(scroll_pointer_event(PointerEventKind::Move, 104.0, 120.0));
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(rememberLazyListState());
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
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

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
            pointer_handler_for(Modifier::empty().horizontal_scroll(scroll_state, false));

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
        let state_for_callback = scroll_state;
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
        let first = Modifier::empty().vertical_scroll(scroll_state, false);
        chain.update_from_slice(&first.elements(), &mut context);
        let stale_handler = collect_modifier_slices(&chain)
            .pointer_inputs()
            .first()
            .cloned()
            .expect("scroll modifier should provide pointer input handler");

        let recomposed = Modifier::empty().vertical_scroll(scroll_state, false);
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
            list_state = Some(cranpose_foundation::lazy::rememberLazyListState());
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
                let state = rememberLazyListState();
                list_state = Some(state);
                LazyColumn(
                    Modifier::empty().fill_max_width().height(list_height),
                    state,
                    LazyColumnSpec::default(),
                    |scope| {
                        scope.items(100, |_| {
                            Spacer(Size {
                                width: 0.0,
                                height: 48.0,
                            });
                        });
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

fn timed_pointer_event(kind: PointerEventKind, x: f32, y: f32, time_ms: i64) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
        .with_buttons(primary_buttons())
        .with_time_ms(Some(time_ms))
}

#[test]
fn batched_touch_delivery_computes_real_finger_velocity() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

        let t0 = 1_234_567i64;
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        crate::render_state::debug_reset_last_fling_velocity();
        let scroll_state = ScrollState::new(200.0);
        scroll_state.set_max_value(4000.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

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
            list_state = Some(rememberLazyListState());
        });
        let list_state = list_state.expect("lazy list state should be created");

        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));

        let t0 = 42_000i64;
        handler(timed_pointer_event(PointerEventKind::Down, 0.0, 400.0, t0));
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
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    crate::render_state::debug_reset_last_fling_velocity();

    let scroll_state = ScrollState::new(5_000.0);
    scroll_state.set_max_value(100_000.0);
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    let mut event_ms = 7_777_000i64;
    let mut frame_ns = 0u64;
    let mut velocities = Vec::new();

    for gesture in 0..3 {
        handler(timed_pointer_event(
            PointerEventKind::Down,
            0.0,
            600.0,
            event_ms,
        ));
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let density = 2.75f32;
        let viewport_dp = 2280.0 / density;
        let item_dp = 177.0 / density;
        let item_count = 30usize;

        let scroll_state = ScrollState::new(0.0);
        let scroll_state_for_content = scroll_state;
        let mut composition = crate::run_test_composition(move || {
            Column(
                Modifier::empty().vertical_scroll(scroll_state_for_content, false),
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

        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));
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
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut list_state = None;
        let _composition = crate::run_test_composition(|| {
            list_state = Some(rememberLazyListState());
        });
        let list_state = list_state.expect("lazy list state should be created");

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

        let outer_state = ScrollState::new(0.0);
        outer_state.set_max_value(1000.0);

        let (inner_handler, _inner_chain) =
            pointer_handler_for(Modifier::empty().lazy_vertical_scroll(list_state, false));
        let (outer_handler, _outer_chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(outer_state, false));

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
            time += 200;
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

#[test]
fn drag_release_inside_settle_band_springs_to_policy_edge() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(400.0);
    scroll_state.set_settle_policy(Some(Rc::new(|proposed, _velocity| {
        if proposed <= 0.0 || proposed >= 52.0 {
            proposed
        } else if proposed < 26.0 {
            0.0
        } else {
            52.0
        }
    })));
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    handler(timed_pointer_event(PointerEventKind::Down, 0.0, 200.0, 0));
    handler(timed_pointer_event(PointerEventKind::Move, 0.0, 168.0, 16));
    handler(timed_pointer_event(PointerEventKind::Up, 0.0, 168.0, 500));
    assert!(
        (scroll_state.value_non_reactive() - 32.0).abs() < 0.5,
        "drag must land mid-band before the settle runs, got {}",
        scroll_state.value_non_reactive()
    );

    for frame in 0..600u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if (scroll_state.value_non_reactive() - 52.0).abs() < 0.25 {
            break;
        }
    }
    assert!(
        (scroll_state.value_non_reactive() - 52.0).abs() < 0.25,
        "release at 32 (above half of the 52 band) must spring to 52, got {}",
        scroll_state.value_non_reactive()
    );
}

#[test]
fn drag_release_below_band_midpoint_springs_back_to_zero() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(400.0);
    scroll_state.set_settle_policy(Some(Rc::new(|proposed, _velocity| {
        if proposed <= 0.0 || proposed >= 52.0 {
            proposed
        } else if proposed < 26.0 {
            0.0
        } else {
            52.0
        }
    })));
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    handler(timed_pointer_event(PointerEventKind::Down, 0.0, 200.0, 0));
    handler(timed_pointer_event(PointerEventKind::Move, 0.0, 182.0, 16));
    handler(timed_pointer_event(PointerEventKind::Up, 0.0, 182.0, 500));

    for frame in 0..600u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if scroll_state.value_non_reactive() < 0.25 {
            break;
        }
    }
    assert!(
        scroll_state.value_non_reactive() < 0.25,
        "release at 18 (below half of the 52 band) must spring back to 0, got {}",
        scroll_state.value_non_reactive()
    );
}

#[test]
fn fling_release_is_retargeted_by_settle_policy() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(10_000.0);
    scroll_state.set_settle_policy(Some(Rc::new(|proposed, _velocity| {
        if proposed > 0.0 && proposed < 9_000.0 {
            123.0
        } else {
            proposed
        }
    })));
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    handler(timed_pointer_event(PointerEventKind::Down, 0.0, 400.0, 0));
    let mut y = 400.0;
    let mut time = 0i64;
    for _ in 0..12 {
        y -= 8.0;
        time += 8;
        handler(timed_pointer_event(PointerEventKind::Move, 0.0, y, time));
    }
    handler(timed_pointer_event(PointerEventKind::Up, 0.0, y, time));

    for frame in 0..900u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if (scroll_state.value_non_reactive() - 123.0).abs() < 0.25 {
            break;
        }
    }
    assert!(
        (scroll_state.value_non_reactive() - 123.0).abs() < 0.25,
        "the fling's predicted rest must be remapped to 123 by the policy, got {}",
        scroll_state.value_non_reactive()
    );
}

#[test]
fn wheel_idle_inside_settle_band_snaps_to_policy_edge() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(400.0);
    scroll_state.set_settle_policy(Some(Rc::new(|proposed, _velocity| {
        if proposed <= 0.0 || proposed >= 52.0 {
            proposed
        } else if proposed < 26.0 {
            0.0
        } else {
            52.0
        }
    })));
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    handler(scroll_wheel_event(0.0, -30.0));
    assert!(
        (scroll_state.value_non_reactive() - 30.0).abs() < 0.5,
        "wheel must land mid-band before the settle runs, got {}",
        scroll_state.value_non_reactive()
    );

    for frame in 1..600u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if (scroll_state.value_non_reactive() - 52.0).abs() < 0.25 {
            break;
        }
    }
    assert!(
        (scroll_state.value_non_reactive() - 52.0).abs() < 0.25,
        "wheel idle inside the band must snap to 52, got {}",
        scroll_state.value_non_reactive()
    );
}

#[test]
fn wheel_edge_overscroll_settles_without_custom_policy() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(400.0);
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));
    for _ in 0..20 {
        let event = scroll_wheel_event(0.0, 200.0);
        handler(event.clone());
        event.finish_post_dispatch();
    }

    let context = scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
        state_id: scroll_state.id(),
        is_vertical: true,
        reverse_scrolling: false,
    });
    assert!(context.overscroll().offset().abs() > 0.001);
    for frame in 1..600u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if context.overscroll().offset().abs() <= 0.001 {
            break;
        }
    }
    assert!(context.overscroll().offset().abs() <= 0.001);
}

#[test]
fn wheel_overscroll_rearms_after_interrupted_settle() {
    let _app_context = crate::render_state::app_context_test_scope();
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let scroll_state = ScrollState::new(0.0);
    scroll_state.set_max_value(400.0);
    let (handler, _chain) =
        pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

    for _ in 0..20 {
        let event = scroll_wheel_event(0.0, 200.0);
        handler(event.clone());
        event.finish_post_dispatch();
    }

    for frame in 1..=12u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
    }

    let context = scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
        state_id: scroll_state.id(),
        is_vertical: true,
        reverse_scrolling: false,
    });
    assert!(context.overscroll().offset().abs() > 0.001);

    let event = scroll_wheel_event(0.0, 200.0);
    handler(event.clone());
    event.finish_post_dispatch();
    assert!(event.is_consumed());

    for frame in 13..120u64 {
        runtime.handle().drain_frame_callbacks(frame * 16_000_000);
        if context.overscroll().offset().abs() <= 0.001 {
            break;
        }
    }
    assert!(context.overscroll().offset().abs() <= 0.001);
}

// The guarded scroll variants were removed in 9af4604b as dead code because
// nothing in this repository called them -- a first-party downstream app did,
// and lost its drag-reorder gesture to the row's own scroll. These tests are
// that missing caller as much as they are a regression guard.

#[test]
fn a_scroll_without_gestures_leaves_the_drag_for_its_owner() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ScrollState::new(0.0);
        state.set_max_value(100.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll_without_gestures(state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
        let move_event = scroll_pointer_event(PointerEventKind::Move, 0.0, 160.0);
        handler(move_event.clone());
        move_event.finish_post_dispatch();

        assert!(
            !move_event.is_consumed(),
            "an unconsumed drag is the whole point: the owner's own pointer_input \
             only sees the event if this scroll declines it"
        );
        assert_eq!(
            state.value(),
            0.0,
            "a scroll that declines the gesture must not move on a drag"
        );
    });
}

#[test]
fn a_scroll_without_gestures_still_moves_programmatically() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ScrollState::new(0.0);
        state.set_max_value(100.0);
        let (_handler, _chain) =
            pointer_handler_for(Modifier::empty().horizontal_scroll_without_gestures(state, false));

        state.scroll_to(40.0);

        assert!(
            (state.value() - 40.0).abs() < 0.001,
            "declining pointer gestures must not disable ScrollState itself, got {}",
            state.value()
        );
    });
}

#[test]
fn an_ordinary_scroll_still_claims_the_drag() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ScrollState::new(0.0);
        state.set_max_value(100.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(state, false));

        handler(scroll_pointer_event(PointerEventKind::Down, 0.0, 100.0));
        let move_event = scroll_pointer_event(PointerEventKind::Move, 0.0, 160.0);
        handler(move_event.clone());
        move_event.finish_post_dispatch();

        assert!(
            move_event.is_consumed(),
            "the control for the two tests above: without this, they would pass \
             even if the scroll gesture had stopped working entirely"
        );
    });
}
