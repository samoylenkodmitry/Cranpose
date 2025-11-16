use super::*;
use crate::modifier::{collect_slices_from_modifier, Modifier, PointerInputScope};
use compose_core::NodeId;
use compose_foundation::{
    modifier_element, BasicModifierNodeContext, ModifierNodeChain, NodeCapabilities, PointerButton,
    PointerButtons, PointerEvent, PointerEventKind, PointerPhase,
};
use compose_ui_layout::Placeable;
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
    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));

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
    assert_eq!(result.width, 70.0);
    assert_eq!(result.height, 70.0);
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
    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
}

#[test]
fn corner_shape_node_is_draw_only() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(CornerShapeElement::new(
        RoundedCornerShape::uniform(6.0),
    ))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
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
    let initial_node = chain.node::<PaddingNode>(0).unwrap() as *const _;

    context.clear_invalidations();

    // Update with different padding - should reuse the same node
    let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
        20.0,
    )))];
    chain.update_from_slice(&elements, &mut context);
    let updated_node = chain.node::<PaddingNode>(0).unwrap() as *const _;

    // Same node instance should be reused
    assert_eq!(initial_node, updated_node);
    assert_eq!(chain.node::<PaddingNode>(0).unwrap().padding.left, 20.0);
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
    assert_eq!(result.width, 100.0);
    assert_eq!(result.height, 200.0);
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

    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::PointerInput));

    // Simulate a pointer event
    let node = chain.node_mut::<ClickableNode>(0).unwrap();
    let event = PointerEvent {
        id: 0,
        kind: PointerEventKind::Down,
        phase: PointerPhase::Start,
        position: Point { x: 10.0, y: 20.0 },
        global_position: Point { x: 10.0, y: 20.0 },
        buttons: PointerButtons::new().with(PointerButton::Primary),
    };

    let consumed = node.on_pointer_event(&mut context, &event);
    assert!(consumed);
    assert!(clicked.get());
}

#[test]
fn alpha_node_clamps_values() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    // Test clamping to valid range
    let elements = vec![modifier_element(AlphaElement::new(1.5))]; // > 1.0
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node::<AlphaNode>(0).unwrap();
    assert_eq!(node.alpha, 1.0);

    context.clear_invalidations();

    // Test negative clamping
    let elements = vec![modifier_element(AlphaElement::new(-0.5))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node::<AlphaNode>(0).unwrap();
    assert_eq!(node.alpha, 0.0);
}

#[test]
fn alpha_node_is_draw_only() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(AlphaElement::new(0.5))];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
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
    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
    assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::PointerInput));

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
    let initial_node_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;

    // Toggle to different color - should reuse same node
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let elements = vec![modifier_element(BackgroundElement::new(blue))];
    chain.update_from_slice(&elements, &mut context);

    // Verify same node instance (zero allocations)
    let updated_node_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;
    assert_eq!(initial_node_ptr, updated_node_ptr, "Node should be reused");

    // Verify color was updated
    assert_eq!(chain.node::<BackgroundNode>(0).unwrap().color, blue);
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

    let padding_ptr = chain.node::<PaddingNode>(0).unwrap() as *const _;
    let background_ptr = chain.node::<BackgroundNode>(1).unwrap() as *const _;

    // Reverse order: background then padding
    let elements = vec![
        modifier_element(BackgroundElement::new(color)),
        modifier_element(PaddingElement::new(padding)),
    ];
    chain.update_from_slice(&elements, &mut context);

    // Nodes should still be reused (matched by type)
    let new_background_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;
    let new_padding_ptr = chain.node::<PaddingNode>(1).unwrap() as *const _;

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
