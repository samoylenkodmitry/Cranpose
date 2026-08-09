use super::*;
use crate::modifier::{
    collect_modifier_slices, collect_slices_from_modifier, Modifier, PointerInputScope,
};
use cranpose_foundation::{
    modifier_element, BasicModifierNodeContext, ModifierNodeChain, NodeCapabilities, PointerButton,
    PointerButtons, PointerEvent, PointerEventKind,
};
use cranpose_ui_layout::Placeable;
use std::cell::{Cell, RefCell};
use std::future::pending;
use std::rc::Rc;

#[test]
fn pointer_input_ids_do_not_use_process_global_counters() {
    let source = include_str!("../modifier/pointer_input.rs");
    let handler_counter = ["static ", "NEXT_HANDLER_ID"].concat();
    let task_counter = ["static ", "NEXT_TASK_ID"].concat();

    assert!(
        !source.contains(&handler_counter) && !source.contains(&task_counter),
        "pointer input handler/task ids must come from retained object identity, not process-global counters"
    );
}

#[test]
fn lazy_graphics_layer_scope_ids_do_not_use_process_global_counter() {
    let source = include_str!("../modifier_nodes.rs");
    let scope_counter = ["static ", "NEXT_LAZY_GRAPHICS_LAYER_SCOPE_ID"].concat();

    assert!(
        !source.contains(&scope_counter),
        "lazy graphics-layer observer scope ids must be owned by the observer/node, not a process-global counter"
    );
}

struct TestMeasurable {
    intrinsic_width: f32,
    intrinsic_height: f32,
}

impl Measurable for TestMeasurable {
    fn measure(&self, constraints: Constraints) -> Placeable {
        Placeable::value(
            constraints.max_width.min(self.intrinsic_width),
            constraints.max_height.min(self.intrinsic_height),
            0,
        )
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.intrinsic_width
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.intrinsic_width
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        self.intrinsic_height
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        self.intrinsic_height
    }
}

#[test]
fn padding_node_adds_space_to_content() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let padding = EdgeInsets::uniform(10.0);
    let elements = vec![modifier_element(PaddingElement::new(padding))];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 1);
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));

    // Test that padding node correctly implements layout
    let node = chain.node_mut::<PaddingNode>(0).unwrap();
    let measurable = TestMeasurable {
        intrinsic_width: 50.0,
        intrinsic_height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 200.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);
    // Content is 50x50, padding is 10 on each side, so total is 70x70
    assert_eq!(result.size.width, 70.0);
    assert_eq!(result.size.height, 70.0);
}

#[test]
fn padding_node_clamps_to_constraints() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut context = BasicModifierNodeContext::new();
    let node = PaddingNode::new(EdgeInsets::uniform(12.0));
    let measurable = TestMeasurable {
        intrinsic_width: 100.0,
        intrinsic_height: 100.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 16.0,
        min_height: 0.0,
        max_height: 16.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    assert_eq!(result.size.width, 16.0);
    assert_eq!(result.size.height, 16.0);
}

#[test]
fn fractional_offset_node_places_content_by_fraction_of_measured_size() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut context = BasicModifierNodeContext::new();
    let node = FractionalOffsetNode::new(0.25, -0.5);
    let measurable = TestMeasurable {
        intrinsic_width: 80.0,
        intrinsic_height: 40.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 200.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    // Offset never affects measurement, only placement.
    assert_eq!(result.size.width, 80.0);
    assert_eq!(result.size.height, 40.0);
    // Placement offset resolves the fractions against the measured size.
    assert_eq!(result.placement_offset_x, 0.25 * 80.0);
    assert_eq!(result.placement_offset_y, -0.5 * 40.0);
}

#[test]
fn fractional_offset_node_passes_intrinsics_through() {
    let node = FractionalOffsetNode::new(0.0, 1.0);
    let measurable = TestMeasurable {
        intrinsic_width: 64.0,
        intrinsic_height: 32.0,
    };

    assert_eq!(node.min_intrinsic_width(&measurable, 32.0), 64.0);
    assert_eq!(node.max_intrinsic_width(&measurable, 32.0), 64.0);
    assert_eq!(node.min_intrinsic_height(&measurable, 64.0), 32.0);
    assert_eq!(node.max_intrinsic_height(&measurable, 64.0), 32.0);
}

#[test]
fn padding_node_respects_intrinsics() {
    let _app_context = crate::render_state::app_context_test_scope();
    let padding = EdgeInsets::uniform(10.0);
    let node = PaddingNode::new(padding);
    let measurable = TestMeasurable {
        intrinsic_width: 50.0,
        intrinsic_height: 30.0,
    };

    // Intrinsic widths should include padding
    assert_eq!(node.min_intrinsic_width(&measurable, 100.0), 70.0); // 50 + 20
    assert_eq!(node.max_intrinsic_width(&measurable, 100.0), 70.0);

    // Intrinsic heights should include padding
    assert_eq!(node.min_intrinsic_height(&measurable, 100.0), 50.0); // 30 + 20
    assert_eq!(node.max_intrinsic_height(&measurable, 100.0), 50.0);
}

#[test]
fn background_node_is_draw_only() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let color = Color(1.0, 0.0, 0.0, 1.0);
    let elements = vec![modifier_element(BackgroundElement::new(color))];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 1);
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));
}

#[test]
fn corner_shape_node_is_draw_only() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(CornerShapeElement::new(
        RoundedCornerShape::uniform(6.0),
    ))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));
}

#[test]
fn modifier_chain_reuses_padding_nodes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    // Initial padding
    let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
        10.0,
    )))];
    chain.update_from_slice(&elements, &mut context);
    let initial_node = {
        let node_ref = chain.node::<PaddingNode>(0).unwrap();
        &*node_ref as *const _
    };

    context.clear_invalidations();

    // Update with different padding - should reuse the same node
    let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
        20.0,
    )))];
    chain.update_from_slice(&elements, &mut context);
    let updated_node = {
        let node_ref = chain.node::<PaddingNode>(0).unwrap();
        &*node_ref as *const _
    };

    // Same node instance should be reused
    assert_eq!(initial_node, updated_node);
    {
        let node_ref = chain.node::<PaddingNode>(0).unwrap();
        assert_eq!(node_ref.padding.left, 20.0);
    }
}

#[test]
fn size_node_enforces_dimensions() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(SizeElement::new(Some(100.0), Some(200.0)))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node_mut::<SizeNode>(0).unwrap();
    let measurable = TestMeasurable {
        intrinsic_width: 50.0,
        intrinsic_height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 500.0,
        min_height: 0.0,
        max_height: 500.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);
    assert_eq!(result.size.width, 100.0);
    assert_eq!(result.size.height, 200.0);
}

#[test]
fn clickable_node_handles_pointer_events() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let clicked = Rc::new(Cell::new(false));
    let clicked_clone = clicked.clone();

    let elements = vec![modifier_element(ClickableElement::new(move |_point| {
        clicked_clone.set(true);
    }))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::PointerInput));

    // Simulate a pointer Down event - should NOT fire click yet
    let mut node = chain.node_mut::<ClickableNode>(0).unwrap();
    let mut down_event = PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    );
    down_event.buttons = PointerButtons::new().with(PointerButton::Primary);

    let consumed = node.on_pointer_event(&mut context, &down_event);
    assert!(!consumed); // Down should NOT be consumed
    assert!(!clicked.get()); // Click should NOT fire yet

    // Simulate a pointer Up event - should fire click
    let mut up_event = PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    );
    up_event.buttons = PointerButtons::new().with(PointerButton::Primary);

    let consumed = node.on_pointer_event(&mut context, &up_event);
    assert!(consumed); // Up should be consumed
    assert!(clicked.get()); // Click should fire on Up
}

#[test]
fn clickable_node_cancels_click_on_drag() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let clicked = Rc::new(Cell::new(false));
    let clicked_clone = clicked.clone();

    let elements = vec![modifier_element(ClickableElement::new(move |_point| {
        clicked_clone.set(true);
    }))];
    chain.update_from_slice(&elements, &mut context);

    // Simulate a pointer Down event
    let mut node = chain.node_mut::<ClickableNode>(0).unwrap();
    let mut down_event = PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    );
    down_event.buttons = PointerButtons::new().with(PointerButton::Primary);
    node.on_pointer_event(&mut context, &down_event);
    assert!(!clicked.get());

    // Simulate a Move event that exceeds drag threshold (8px)
    let mut move_event = PointerEvent::new(
        PointerEventKind::Move,
        Point { x: 20.0, y: 20.0 }, // Moved 10px horizontally, beyond 8px threshold
        Point { x: 20.0, y: 20.0 },
    );
    move_event.buttons = PointerButtons::new().with(PointerButton::Primary);
    node.on_pointer_event(&mut context, &move_event);
    assert!(!clicked.get()); // Still no click

    // Simulate a pointer Up event - should NOT fire click because we dragged
    let mut up_event = PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 20.0, y: 20.0 },
        Point { x: 20.0, y: 20.0 },
    );
    up_event.buttons = PointerButtons::new().with(PointerButton::Primary);

    let consumed = node.on_pointer_event(&mut context, &up_event);
    assert!(!consumed); // Up should NOT be consumed (click cancelled)
    assert!(!clicked.get()); // Click should NOT fire because we dragged
}

#[test]
fn alpha_node_clamps_values() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    // Test clamping to valid range
    let elements = vec![modifier_element(AlphaElement::new(1.5))]; // > 1.0
    chain.update_from_slice(&elements, &mut context);

    {
        let node = chain.node::<AlphaNode>(0).unwrap();
        assert_eq!(node.alpha, 1.0);
    }

    context.clear_invalidations();

    // Test negative clamping
    let elements = vec![modifier_element(AlphaElement::new(-0.5))];
    chain.update_from_slice(&elements, &mut context);

    {
        let node = chain.node::<AlphaNode>(0).unwrap();
        assert_eq!(node.alpha, 0.0);
    }
}

#[test]
fn alpha_node_is_draw_only() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(AlphaElement::new(0.5))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));
}

#[test]
fn mixed_modifier_chain_tracks_all_capabilities() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let clicked = Rc::new(Cell::new(false));
    let clicked_clone = clicked.clone();

    // Create a chain with layout, draw, and pointer input nodes
    let elements = vec![
        modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
        modifier_element(AlphaElement::new(0.8)),
        modifier_element(ClickableElement::new(move |_| {
            clicked_clone.set(true);
        })),
        modifier_element(BackgroundElement::new(Color(1.0, 0.0, 0.0, 1.0))),
    ];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 4);
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::PointerInput));

    // Verify correct node counts by type
    let mut layout_nodes = 0;
    chain.for_each_forward_matching(NodeCapabilities::LAYOUT, |_| {
        layout_nodes += 1;
    });
    assert_eq!(layout_nodes, 1, "expected a single layout node");

    let mut draw_nodes = 0;
    chain.for_each_forward_matching(NodeCapabilities::DRAW, |_| {
        draw_nodes += 1;
    });
    assert_eq!(draw_nodes, 2, "expected alpha + background draw nodes");

    let mut pointer_nodes = 0;
    chain.for_each_forward_matching(NodeCapabilities::POINTER_INPUT, |_| {
        pointer_nodes += 1;
    });
    assert_eq!(pointer_nodes, 1, "expected exactly one pointer node");
}

#[test]
fn toggling_background_color_reuses_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    // This test verifies the gate condition:
    // "Toggling Modifier.background(color) allocates 0 new nodes; only update() runs"
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    // Initial background
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let elements = vec![modifier_element(BackgroundElement::new(red))];
    chain.update_from_slice(&elements, &mut context);

    // Get pointer to the node
    let initial_node_ptr = {
        let node_ref = chain.node::<BackgroundNode>(0).unwrap();
        &*node_ref as *const _
    };

    // Toggle to different color - should reuse same node
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let elements = vec![modifier_element(BackgroundElement::new(blue))];
    chain.update_from_slice(&elements, &mut context);

    // Verify same node instance (zero allocations)
    let updated_node_ptr = {
        let node_ref = chain.node::<BackgroundNode>(0).unwrap();
        &*node_ref as *const _
    };
    assert_eq!(initial_node_ptr, updated_node_ptr, "Node should be reused");

    // Verify color was updated
    {
        let node_ref = chain.node::<BackgroundNode>(0).unwrap();
        assert_eq!(node_ref.color, blue);
    }
}

#[test]
fn reordering_modifiers_with_stable_reuse() {
    let _app_context = crate::render_state::app_context_test_scope();
    // This test verifies the gate condition:
    // "Reordering modifiers: stable reuse when elements equal (by type + key)"
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let padding = EdgeInsets::uniform(10.0);
    let color = Color(1.0, 0.0, 0.0, 1.0);

    // Initial order: padding then background
    let elements = vec![
        modifier_element(PaddingElement::new(padding)),
        modifier_element(BackgroundElement::new(color)),
    ];
    chain.update_from_slice(&elements, &mut context);

    let (padding_ptr, background_ptr) = {
        let padding_ref = chain.node::<PaddingNode>(0).unwrap();
        let background_ref = chain.node::<BackgroundNode>(1).unwrap();
        (&*padding_ref as *const _, &*background_ref as *const _)
    };

    // Reverse order: background then padding
    let elements = vec![
        modifier_element(BackgroundElement::new(color)),
        modifier_element(PaddingElement::new(padding)),
    ];
    chain.update_from_slice(&elements, &mut context);

    // Nodes should still be reused (matched by type)
    let (new_background_ptr, new_padding_ptr) = {
        let background_ref = chain.node::<BackgroundNode>(0).unwrap();
        let padding_ref = chain.node::<PaddingNode>(1).unwrap();
        (&*background_ref as *const _, &*padding_ref as *const _)
    };

    assert_eq!(
        background_ptr, new_background_ptr,
        "Background node should be reused"
    );
    assert_eq!(
        padding_ptr, new_padding_ptr,
        "Padding node should be reused"
    );
}

#[test]
fn pointer_input_coroutine_receives_events() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let modifier = Modifier::empty().pointer_input((), {
        let recorded = recorded.clone();
        move |scope: PointerInputScope| {
            let recorded = recorded.clone();
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            recorded.borrow_mut().push(event.kind);
                        }
                    })
                    .await;
            }
        }
    });

    let elements = modifier.elements();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_slices_from_modifier(&modifier);
    assert_eq!(slices.pointer_inputs().len(), 1);
    let handler = slices.pointer_inputs()[0].clone();

    handler(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    ));
    handler(PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 1.0, y: 1.0 },
        Point { x: 1.0, y: 1.0 },
    ));

    let events = recorded.borrow();
    assert_eq!(*events, vec![PointerEventKind::Down, PointerEventKind::Up]);
}

#[test]
fn pointer_input_waker_reenters_owning_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let app_context = crate::AppContext::new_with_density(3.0);
    let recorded = Rc::new(RefCell::new(Vec::new()));

    let (handler, _chain) = app_context.enter(|| {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        let modifier = Modifier::empty().pointer_input((), {
            let recorded = recorded.clone();
            move |scope: PointerInputScope| {
                let recorded = recorded.clone();
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let event = await_scope.await_pointer_event().await;
                            recorded
                                .borrow_mut()
                                .push((event.kind, crate::current_density()));
                        })
                        .await;
                }
            }
        });

        let elements = modifier.elements();
        chain.update_from_slice(&elements, &mut context);
        let slices = collect_modifier_slices(&chain);
        assert_eq!(slices.pointer_inputs().len(), 1);
        (slices.pointer_inputs()[0].clone(), chain)
    });

    handler(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    ));

    assert_eq!(*recorded.borrow(), vec![(PointerEventKind::Down, 3.0)]);
}

#[test]
fn pointer_input_restarts_on_key_change() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let starts = Rc::new(Cell::new(0));

    let modifier = Modifier::empty().pointer_input(0u32, {
        let starts = starts.clone();
        move |_scope: PointerInputScope| {
            let starts = starts.clone();
            async move {
                starts.set(starts.get() + 1);
                pending::<()>().await;
            }
        }
    });

    let elements = modifier.elements();
    chain.update_from_slice(&elements, &mut context);
    assert_eq!(starts.get(), 1);

    let modifier_updated = Modifier::empty().pointer_input(1u32, {
        let starts = starts.clone();
        move |_scope: PointerInputScope| {
            let starts = starts.clone();
            async move {
                starts.set(starts.get() + 1);
                pending::<()>().await;
            }
        }
    });

    let elements_updated = modifier_updated.elements();
    chain.update_from_slice(&elements_updated, &mut context);
    assert_eq!(starts.get(), 2);
}

/// `PointerInputScope::size()` must report the node's real layout size.
///
/// Regression: the scope's size cell had no writer anywhere in the workspace,
/// so `size()` was permanently `0x0` on every platform. Anything deriving
/// geometry from it (a full-screen canvas taking its centre as `size / 2`) read
/// the node's top-left corner instead.
///
/// The size must be readable without any pointer event having arrived — Compose
/// handlers read `size` before they await — and must track resizes.
#[test]
fn pointer_input_scope_reports_published_layout_size() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let captured: Rc<RefCell<Option<PointerInputScope>>> = Rc::new(RefCell::new(None));
    let event_sizes: Rc<RefCell<Vec<Size>>> = Rc::new(RefCell::new(Vec::new()));
    let modifier = Modifier::empty().pointer_input((), {
        let captured = captured.clone();
        let event_sizes = event_sizes.clone();
        move |scope: PointerInputScope| {
            let captured = captured.clone();
            let event_sizes = event_sizes.clone();
            async move {
                *captured.borrow_mut() = Some(scope.clone());
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let _event = await_scope.await_pointer_event().await;
                            event_sizes.borrow_mut().push(await_scope.size());
                        }
                    })
                    .await;
            }
        }
    });

    let elements = modifier.elements();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_modifier_slices(&chain);

    assert_eq!(
        slices.pointer_input_size_sinks().len(),
        1,
        "the pointer input node must expose a size sink for the layout pass"
    );

    let scope = captured
        .borrow()
        .clone()
        .expect("pointer input handler should have started");
    assert_eq!(
        scope.size(),
        Size {
            width: 0.0,
            height: 0.0
        },
        "an unmeasured node has no size yet"
    );

    // The layout pass publishes the node's resolved size; no pointer event has
    // been dispatched at this point.
    slices.publish_pointer_input_size(Size {
        width: 240.0,
        height: 160.0,
    });
    assert_eq!(
        scope.size(),
        Size {
            width: 240.0,
            height: 160.0
        },
        "scope.size() must report the laid-out size before any pointer event"
    );

    // A resize must be observed by the same scope.
    slices.publish_pointer_input_size(Size {
        width: 100.0,
        height: 50.0,
    });
    assert_eq!(
        scope.size(),
        Size {
            width: 100.0,
            height: 50.0
        },
        "scope.size() must track resizes"
    );

    // The awaiting scope reports the same size as the outer scope.
    slices.pointer_inputs()[0](PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    ));
    assert_eq!(
        *event_sizes.borrow(),
        vec![Size {
            width: 100.0,
            height: 50.0
        }],
        "AwaitPointerEventScope::size() must report the same laid-out size"
    );
}

/// The published size lives on the node, not on the per-run scope, so a handler
/// restart (a key change) hands the fresh scope the size the node already has —
/// the node was not re-measured, so its size must not fall back to `0x0`.
#[test]
fn pointer_input_scope_size_survives_handler_restart() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let captured: Rc<RefCell<Option<PointerInputScope>>> = Rc::new(RefCell::new(None));
    let handler = {
        let captured = captured.clone();
        move |scope: PointerInputScope| {
            let captured = captured.clone();
            async move {
                *captured.borrow_mut() = Some(scope.clone());
                pending::<()>().await;
            }
        }
    };

    let elements = Modifier::empty()
        .pointer_input(0u32, handler.clone())
        .elements();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_modifier_slices(&chain);
    slices.publish_pointer_input_size(Size {
        width: 320.0,
        height: 180.0,
    });

    let elements = Modifier::empty().pointer_input(1u32, handler).elements();
    chain.update_from_slice(&elements, &mut context);

    let restarted_scope = captured
        .borrow()
        .clone()
        .expect("restarted handler should have started");
    assert_eq!(
        restarted_scope.size(),
        Size {
            width: 320.0,
            height: 180.0
        },
        "a scope created by a handler restart must keep the node's known size"
    );
}

/// End-to-end through the real layout pipeline: measuring and placing the tree
/// is what fills `PointerInputScope::size()`, and re-laying out at a new
/// viewport updates it.
#[test]
fn pointer_input_scope_size_is_filled_by_the_layout_pass() {
    let captured: Rc<RefCell<Option<PointerInputScope>>> = Rc::new(RefCell::new(None));
    let mut composition = crate::run_test_composition({
        let captured = captured.clone();
        move || {
            crate::Box(
                Modifier::empty().fill_max_size().pointer_input((), {
                    let captured = captured.clone();
                    move |scope: PointerInputScope| {
                        let captured = captured.clone();
                        async move {
                            *captured.borrow_mut() = Some(scope.clone());
                            pending::<()>().await;
                        }
                    }
                }),
                crate::BoxSpec::new(),
                || {},
            );
        }
    });
    let root = composition.root().expect("composition root");

    let scope = captured
        .borrow()
        .clone()
        .expect("pointer input handler should have started during composition");
    assert_eq!(
        scope.size(),
        Size {
            width: 0.0,
            height: 0.0
        },
        "no layout pass has run yet"
    );

    let layout_at = |composition: &mut crate::TestComposition, size: Size| {
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        crate::measure_layout(&mut applier, root, size).expect("layout measurement");
        crate::build_layout_tree_from_applier(&mut applier, root)
            .expect("layout tree build")
            .expect("layout tree present");
        applier.clear_runtime_handle();
    };

    layout_at(
        &mut composition,
        Size {
            width: 400.0,
            height: 320.0,
        },
    );
    assert_eq!(
        scope.size(),
        Size {
            width: 400.0,
            height: 320.0
        },
        "the layout pass must publish the node's measured size into the scope"
    );

    layout_at(
        &mut composition,
        Size {
            width: 240.0,
            height: 240.0,
        },
    );
    assert_eq!(
        scope.size(),
        Size {
            width: 240.0,
            height: 240.0
        },
        "a re-layout at a new viewport must update the scope size"
    );
}

/// Pointer input handlers extracted from a modifier slice keep their task alive
/// until explicit cancellation, even after the temporary collection chain drops.
#[test]
fn pointer_input_handlers_survive_temporary_chain_drop() {
    let _app_context = crate::render_state::app_context_test_scope();
    use std::cell::RefCell;
    use std::rc::Rc;

    // Track received events
    let received_events = Rc::new(RefCell::new(Vec::new()));

    // Create a modifier with pointer input
    let modifier = Modifier::empty().pointer_input(42u32, {
        let events = received_events.clone();
        move |scope: PointerInputScope| {
            let events = events.clone();
            async move {
                loop {
                    let event = scope
                        .await_pointer_event_scope(|s| async move { s.await_pointer_event().await })
                        .await;
                    events.borrow_mut().push(event.kind);
                }
            }
        }
    });

    let slices = collect_slices_from_modifier(&modifier);

    // Verify we got a handler
    assert_eq!(
        slices.pointer_inputs().len(),
        1,
        "Should have extracted one pointer input handler"
    );

    // Extract the handler - this is what the renderer does
    let handler = slices.pointer_inputs()[0].clone();

    handler(PointerEvent::new(
        PointerEventKind::Move,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    ));

    handler(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    ));

    handler(PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 10.0, y: 20.0 },
        Point { x: 10.0, y: 20.0 },
    ));

    // Verify all events were received by the async handler
    let events = received_events.borrow();
    assert_eq!(
        *events,
        vec![
            PointerEventKind::Move,
            PointerEventKind::Down,
            PointerEventKind::Up
        ],
        "All events should be received even after temporary chain is dropped"
    );
}

/// Test that multiple temporary chains can coexist without interfering with each other.
#[test]
fn multiple_temporary_chains_dont_interfere() {
    let _app_context = crate::render_state::app_context_test_scope();
    use std::cell::RefCell;
    use std::rc::Rc;

    let events1 = Rc::new(RefCell::new(Vec::new()));
    let events2 = Rc::new(RefCell::new(Vec::new()));

    // Create first modifier
    let modifier1 = Modifier::empty().pointer_input(1u32, {
        let events = events1.clone();
        move |scope: PointerInputScope| {
            let events = events.clone();
            async move {
                loop {
                    let event = scope
                        .await_pointer_event_scope(|s| async move { s.await_pointer_event().await })
                        .await;
                    events.borrow_mut().push(("handler1", event.kind));
                }
            }
        }
    });

    // Create second modifier
    let modifier2 = Modifier::empty().pointer_input(2u32, {
        let events = events2.clone();
        move |scope: PointerInputScope| {
            let events = events.clone();
            async move {
                loop {
                    let event = scope
                        .await_pointer_event_scope(|s| async move { s.await_pointer_event().await })
                        .await;
                    events.borrow_mut().push(("handler2", event.kind));
                }
            }
        }
    });

    // Collect slices from both modifiers
    let slices1 = collect_slices_from_modifier(&modifier1);
    let slices2 = collect_slices_from_modifier(&modifier2);

    let handler1 = slices1.pointer_inputs()[0].clone();
    let handler2 = slices2.pointer_inputs()[0].clone();

    // Send events to both handlers
    handler1(PointerEvent::new(
        PointerEventKind::Move,
        Point { x: 1.0, y: 1.0 },
        Point { x: 1.0, y: 1.0 },
    ));

    handler2(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 2.0, y: 2.0 },
        Point { x: 2.0, y: 2.0 },
    ));

    handler1(PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 1.0, y: 1.0 },
        Point { x: 1.0, y: 1.0 },
    ));

    // Verify each handler only received its own events
    let ev1 = events1.borrow();
    let ev2 = events2.borrow();

    assert_eq!(ev1.len(), 2, "Handler 1 should receive 2 events");
    assert_eq!(ev1[0], ("handler1", PointerEventKind::Move));
    assert_eq!(ev1[1], ("handler1", PointerEventKind::Up));

    assert_eq!(ev2.len(), 1, "Handler 2 should receive 1 event");
    assert_eq!(ev2[0], ("handler2", PointerEventKind::Down));
}

/// Custom user-defined layout modifiers participate in the retained coordinator
/// chain without being hardcoded into the framework.
#[test]
fn custom_layout_modifier_works_through_retained_chain() {
    let _app_context = crate::render_state::app_context_test_scope();
    use cranpose_foundation::{
        DelegatableNode, LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext,
        ModifierNodeElement, NodeCapabilities, NodeState,
    };
    use std::hash::{Hash, Hasher};

    // Define a custom layout modifier that adds extra width
    #[derive(Debug)]
    struct CustomWidthNode {
        extra_width: f32,
        state: NodeState,
    }

    impl CustomWidthNode {
        fn new(extra_width: f32) -> Self {
            Self {
                extra_width,
                state: NodeState::new(),
            }
        }
    }

    impl DelegatableNode for CustomWidthNode {
        fn node_state(&self) -> &NodeState {
            &self.state
        }
    }

    impl ModifierNode for CustomWidthNode {
        fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
            context.invalidate(cranpose_foundation::InvalidationKind::Layout);
        }

        fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
            Some(self)
        }

        fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
            Some(self)
        }
    }

    impl LayoutModifierNode for CustomWidthNode {
        fn measure(
            &self,
            _context: &mut dyn ModifierNodeContext,
            measurable: &dyn Measurable,
            constraints: Constraints,
        ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
            let placeable = measurable.measure(constraints);
            cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
                width: placeable.width() + self.extra_width,
                height: placeable.height(),
            })
        }

        fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
            measurable.min_intrinsic_width(height) + self.extra_width
        }

        fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
            measurable.max_intrinsic_width(height) + self.extra_width
        }

        fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
            measurable.min_intrinsic_height(width)
        }

        fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
            measurable.max_intrinsic_height(width)
        }
    }

    // Define the element
    #[derive(Debug, Clone, PartialEq)]
    struct CustomWidthElement {
        extra_width: f32,
    }

    impl Hash for CustomWidthElement {
        fn hash<H: Hasher>(&self, state: &mut H) {
            state.write_u32(self.extra_width.to_bits());
        }
    }

    impl ModifierNodeElement for CustomWidthElement {
        type Node = CustomWidthNode;

        fn create(&self) -> Self::Node {
            CustomWidthNode::new(self.extra_width)
        }

        fn update(&self, node: &mut Self::Node) {
            if node.extra_width != self.extra_width {
                node.extra_width = self.extra_width;
            }
        }

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities::LAYOUT
        }
    }

    // Test the custom modifier
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(CustomWidthElement { extra_width: 20.0 })];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 1);
    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));

    // Test that the custom modifier correctly adds width
    let node = chain.node_mut::<CustomWidthNode>(0).unwrap();
    let measurable = TestMeasurable {
        intrinsic_width: 100.0,
        intrinsic_height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);
    // Content is 100x50, we add 20 to width, so result is 120x50
    assert_eq!(result.size.width, 120.0);
    assert_eq!(result.size.height, 50.0);

    // Test intrinsics
    let intrinsic_width = node.min_intrinsic_width(&measurable, 100.0);
    assert_eq!(intrinsic_width, 120.0); // 100 + 20
}

#[test]
fn draw_command_updates_on_closure_change() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::draw::DrawCommand;
    use cranpose_ui_graphics::Size;

    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let executed = Rc::new(Cell::new(0));

    // Element 1: Increments executed by 1
    let executed_1 = executed.clone();
    let element_1 = modifier_element(DrawCommandElement::new(DrawCommand::Behind(Rc::new(
        move |_scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
            executed_1.set(executed_1.get() + 1);
        },
    ))));

    // Element 2: Increments executed by 10
    let executed_2 = executed.clone();
    let element_2 = modifier_element(DrawCommandElement::new(DrawCommand::Behind(Rc::new(
        move |_scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
            executed_2.set(executed_2.get() + 10);
        },
    ))));

    // Verify elements are "equal" (PartialEq ignores closures)

    // Initial update
    chain.update_from_slice(&[element_1], &mut context);

    // Execute command from node
    {
        let node = chain.node::<DrawCommandNode>(0).unwrap();
        if let DrawCommand::Behind(ref func) = node.commands()[0] {
            func(&mut crate::draw::command_draw_scope(Size::ZERO));
        }
    }
    assert_eq!(executed.get(), 1);

    // Second update with different closure
    executed.set(0);
    chain.update_from_slice(&[element_2], &mut context);

    // Verify node updated to new closure despite equality
    let node = chain.node::<DrawCommandNode>(0).unwrap();
    if let DrawCommand::Behind(ref func) = node.commands()[0] {
        func(&mut crate::draw::command_draw_scope(Size::ZERO));
    }
    assert_eq!(
        executed.get(),
        10,
        "Node should have updated to the new closure"
    );
}

/// Stateful layout modifier nodes retain their internal state across repeated
/// measurement because the coordinator chain invokes the live retained node.
#[test]
fn stateful_measure_uses_live_retained_node_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    use cranpose_foundation::{
        Constraints, DelegatableNode, LayoutModifierNode, Measurable, ModifierNode,
        ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
    };
    use std::hash::{Hash, Hasher};

    /// A layout modifier node that counts how many times it has been measured.
    #[derive(Debug)]
    struct StatefulMeasureNode {
        state: NodeState,
        /// Counter that tracks measure calls (simulates node internal state)
        measure_count: Cell<i32>,
        /// Initial value to add to width (demonstrates parameter capture)
        initial_value: i32,
    }

    impl StatefulMeasureNode {
        fn new(initial_value: i32) -> Self {
            Self {
                state: NodeState::new(),
                measure_count: Cell::new(0),
                initial_value,
            }
        }
    }

    impl DelegatableNode for StatefulMeasureNode {
        fn node_state(&self) -> &NodeState {
            &self.state
        }
    }

    impl ModifierNode for StatefulMeasureNode {
        fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
            Some(self)
        }

        fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
            Some(self)
        }
    }

    impl LayoutModifierNode for StatefulMeasureNode {
        fn measure(
            &self,
            _context: &mut dyn ModifierNodeContext,
            measurable: &dyn Measurable,
            constraints: Constraints,
        ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
            // Increment the measure count - this is the state we want to preserve
            let count = self.measure_count.get();
            self.measure_count.set(count + 1);

            // Measure wrapped content and add initial_value to demonstrate state capture
            let placeable = measurable.measure(constraints);
            cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
                width: placeable.width() + self.initial_value as f32,
                height: placeable.height(),
            })
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct StatefulMeasureElement {
        initial_value: i32,
    }

    impl Hash for StatefulMeasureElement {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.initial_value.hash(state);
        }
    }

    impl ModifierNodeElement for StatefulMeasureElement {
        type Node = StatefulMeasureNode;

        fn create(&self) -> Self::Node {
            StatefulMeasureNode::new(self.initial_value)
        }

        fn update(&self, node: &mut Self::Node) {
            node.initial_value = self.initial_value;
        }

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities::LAYOUT
        }
    }

    // Test setup: Create a node via the modifier chain
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let element = StatefulMeasureElement { initial_value: 10 };
    let elements = vec![modifier_element(element)];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 1);

    // First measurement: Measure directly through the node
    let node = chain.node::<StatefulMeasureNode>(0).unwrap();
    let measurable = TestMeasurable {
        intrinsic_width: 100.0,
        intrinsic_height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 200.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let size1 = node.measure(&mut context, &measurable, constraints);
    assert_eq!(size1.size.width, 110.0); // 100 + 10
    assert_eq!(size1.size.height, 50.0);

    // Check that measure_count was incremented
    let count_after_first = node.measure_count.get();
    assert_eq!(
        count_after_first, 1,
        "First measure should increment count to 1"
    );

    let size2 = node.measure(&mut context, &measurable, constraints);
    assert_eq!(size2.size.width, 110.0); // Still 100 + 10 (initial_value preserved)
    assert_eq!(size2.size.height, 50.0);

    let count_after_second = node.measure_count.get();
    assert_eq!(
        count_after_second, 2,
        "live retained node state should observe both measurements"
    );
}
