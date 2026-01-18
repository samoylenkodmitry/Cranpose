use super::*;
use crate::modifier::{collect_slices_from_modifier, Modifier, PointerInputScope};
use cranpose_core::NodeId;
use cranpose_foundation::{
    modifier_element, BasicModifierNodeContext, ModifierNodeChain, NodeCapabilities, PointerButton,
    PointerButtons, PointerEvent, PointerEventKind,
};
use cranpose_ui_layout::Placeable;
use std::cell::{Cell, RefCell};
use std::future::pending;
use std::rc::Rc;

struct TestPlaceable {
    width: f32,
    height: f32,
    node_id: NodeId,
}

impl Placeable for TestPlaceable {
    fn place(&self, _x: f32, _y: f32) {}

    fn width(&self) -> f32 {
        self.width
    }

    fn height(&self) -> f32 {
        self.height
    }

    fn node_id(&self) -> NodeId {
        self.node_id
    }
}

struct TestMeasurable {
    intrinsic_width: f32,
    intrinsic_height: f32,
}

impl Measurable for TestMeasurable {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        Box::new(TestPlaceable {
            width: constraints.max_width.min(self.intrinsic_width),
            height: constraints.max_height.min(self.intrinsic_height),
            node_id: 0,
        })
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
fn padding_node_respects_intrinsics() {
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
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(AlphaElement::new(0.5))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Layout));
}

#[test]
fn mixed_modifier_chain_tracks_all_capabilities() {
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
fn pointer_input_restarts_on_key_change() {
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

/// Regression test for pointer input handlers from temporary chains.
///
/// This test validates that pointer input handlers extracted via `collect_slices_from_modifier()`
/// continue to work correctly even after the temporary modifier chain is dropped.
///
/// Before the fix, the global task registry used weak references and tasks were removed in `Drop`,
/// causing handlers to fail when the temporary chain was dropped. This led to the bug where
/// mouse events weren't being delivered to async pointer input handlers in the desktop app.
///
/// The fix changed the registry to use strong `Rc` references and only remove tasks on explicit
/// cancellation, allowing handlers from temporary chains to remain functional.
#[test]
fn pointer_input_handlers_survive_temporary_chain_drop() {
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

    // Collect slices from the modifier - this creates a TEMPORARY chain
    // The chain will be dropped when this function returns, but the handler should still work!
    let slices = collect_slices_from_modifier(&modifier);

    // Verify we got a handler
    assert_eq!(
        slices.pointer_inputs().len(),
        1,
        "Should have extracted one pointer input handler"
    );

    // Extract the handler - this is what the renderer does
    let handler = slices.pointer_inputs()[0].clone();

    // At this point, the temporary ModifierChainHandle created by collect_slices_from_modifier
    // has been dropped. Before the fix, this would have removed the task from the global registry.

    // Now send events through the handler - this should work even though the chain is dropped!
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

/// Test that custom user-defined layout modifiers can work with the coordinator chain
/// via the measurement proxy API.
///
/// This test validates Phase 1 of the modifier system migration: generic extensibility.
/// Custom layout modifiers can now provide their own measurement proxies, enabling them
/// to participate in the layout process without being hardcoded into the coordinator.
#[test]
fn custom_layout_modifier_works_via_proxy() {
    use cranpose_foundation::{
        DelegatableNode, LayoutModifierNode, Measurable, MeasurementProxy, ModifierNode,
        ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
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

        // This is the key: provide a measurement proxy
        fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
            Some(Box::new(CustomWidthProxy {
                extra_width: self.extra_width,
            }))
        }
    }

    // Define the measurement proxy
    struct CustomWidthProxy {
        extra_width: f32,
    }

    impl MeasurementProxy for CustomWidthProxy {
        fn measure_proxy(
            &self,
            context: &mut dyn ModifierNodeContext,
            wrapped: &dyn Measurable,
            constraints: Constraints,
        ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
            let node = CustomWidthNode::new(self.extra_width);

            node.measure(context, wrapped, constraints)
        }

        fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
            let node = CustomWidthNode::new(self.extra_width);
            node.min_intrinsic_width(wrapped, height)
        }

        fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
            let node = CustomWidthNode::new(self.extra_width);
            node.max_intrinsic_width(wrapped, height)
        }

        fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
            let node = CustomWidthNode::new(self.extra_width);
            node.min_intrinsic_height(wrapped, width)
        }

        fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
            let node = CustomWidthNode::new(self.extra_width);
            node.max_intrinsic_height(wrapped, width)
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
    use crate::draw::DrawCommand;
    use cranpose_ui_graphics::{DrawPrimitive, Size};

    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let executed = Rc::new(Cell::new(0));

    // Element 1: Increments executed by 1
    let executed_1 = executed.clone();
    let element_1 = modifier_element(DrawCommandElement::new(DrawCommand::Behind(Rc::new(
        move |_size: Size| -> Vec<DrawPrimitive> {
            executed_1.set(executed_1.get() + 1);
            Vec::new()
        },
    ))));

    // Element 2: Increments executed by 10
    let executed_2 = executed.clone();
    let element_2 = modifier_element(DrawCommandElement::new(DrawCommand::Behind(Rc::new(
        move |_size: Size| -> Vec<DrawPrimitive> {
            executed_2.set(executed_2.get() + 10);
            Vec::new()
        },
    ))));

    // Verify elements are "equal" (PartialEq ignores closures)

    // Initial update
    chain.update_from_slice(&[element_1], &mut context);

    // Execute command from node
    {
        let node = chain.node::<DrawCommandNode>(0).unwrap();
        if let DrawCommand::Behind(ref func) = node.commands()[0] {
            func(Size::ZERO);
        }
    }
    assert_eq!(executed.get(), 1);

    // Second update with different closure
    executed.set(0);
    chain.update_from_slice(&[element_2], &mut context);

    // Verify node updated to new closure despite equality
    let node = chain.node::<DrawCommandNode>(0).unwrap();
    if let DrawCommand::Behind(ref func) = node.commands()[0] {
        func(Size::ZERO);
    }
    assert_eq!(
        executed.get(),
        10,
        "Node should have updated to the new closure"
    );
}

/// Test that exposes state fidelity issue: proxies that reconstruct nodes lose state.
///
/// This test demonstrates the limitation of Phase 1's proxy implementation where
/// `Node::new()` reconstruction loses any internal state accumulated during the
/// node's lifecycle. A stateful modifier (one that tracks measure count) will have
/// its state reset on each proxy invocation.
///
/// Phase 2 will fix this by making proxies snapshot live node state instead.
#[test]
fn stateful_measure_exposes_proxy_reconstruction_issue() {
    use cranpose_foundation::{
        Constraints, DelegatableNode, LayoutModifierNode, Measurable, MeasurementProxy,
        ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
    };
    use std::hash::{Hash, Hasher};

    /// A layout modifier node that counts how many times it's been measured.
    /// This demonstrates state that should be preserved across proxy invocations.
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

        fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
            // Phase 1 approach: Reconstruct node (LOSES STATE!)
            // This creates a fresh node with initial_value=10 but measure_count=0
            Some(Box::new(StatefulMeasureProxy {
                initial_value: self.initial_value,
            }))
        }
    }

    struct StatefulMeasureProxy {
        initial_value: i32,
    }

    impl MeasurementProxy for StatefulMeasureProxy {
        fn measure_proxy(
            &self,
            context: &mut dyn ModifierNodeContext,
            wrapped: &dyn Measurable,
            constraints: Constraints,
        ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
            // Phase 1: Reconstruct the node (simulates current implementation pattern)
            // This creates a fresh node, losing measure_count state from the original
            let node = StatefulMeasureNode::new(self.initial_value);
            node.measure(context, wrapped, constraints)
        }

        fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
            wrapped.min_intrinsic_width(height) + self.initial_value as f32
        }

        fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
            wrapped.max_intrinsic_width(height) + self.initial_value as f32
        }

        fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, _width: f32) -> f32 {
            wrapped.min_intrinsic_height(_width)
        }

        fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, _width: f32) -> f32 {
            wrapped.max_intrinsic_height(_width)
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

    // Second measurement: This time via the proxy
    // Phase 1's proxy will reconstruct the node, resetting measure_count to 0
    let proxy = node.create_measurement_proxy().expect("Should have proxy");
    let size2 = proxy.measure_proxy(&mut context, &measurable, constraints);
    assert_eq!(size2.size.width, 110.0); // Still 100 + 10 (initial_value preserved)
    assert_eq!(size2.size.height, 50.0);

    // The original node's count should still be 1 (proxy didn't touch it)
    let count_after_proxy = node.measure_count.get();
    assert_eq!(
        count_after_proxy, 1,
        "Original node count unchanged - proxy creates fresh node"
    );

    // This demonstrates the Phase 1 limitation:
    // - The proxy correctly preserves initial_value (constructor parameter)
    // - But it LOSES measure_count state (internal node state)
    //
    // Phase 2 will make proxies snapshot ALL live node state, not just constructor params.
}
