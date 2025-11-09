use super::*;
use compose_core::NodeId;
use compose_foundation::{
    modifier_element, BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons,
    PointerPhase,
};
use compose_ui_layout::Placeable;
use std::cell::Cell;

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
    assert_eq!(chain.layout_nodes().count(), 1); // padding
    assert_eq!(chain.draw_nodes().count(), 2); // alpha + background
    assert_eq!(chain.pointer_input_nodes().count(), 1); // clickable
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
