use super::{
    inspector_metadata, Alignment, Color, DimensionConstraint, EdgeInsets, GraphicsLayer,
    HorizontalAlignment, Modifier, ModifierChainHandle, Point, SemanticsConfiguration, Size,
    VerticalAlignment,
};
use compose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState,
};

#[test]
fn padding_nodes_resolve_padding_values() {
    let modifier = Modifier::empty()
        .padding(4.0)
        .then(Modifier::empty().padding_horizontal(2.0))
        .then(Modifier::empty().padding_each(1.0, 3.0, 5.0, 7.0));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);
    let padding = handle.resolved_modifiers().padding();
    assert_eq!(
        padding,
        EdgeInsets {
            left: 7.0,
            top: 7.0,
            right: 11.0,
            bottom: 11.0,
        }
    );
}

#[test]
fn fill_max_size_sets_fraction_constraints() {
    let modifier = Modifier::empty().fill_max_size_fraction(0.75);
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
    assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
}

#[test]
fn weight_tracks_fill_flag() {
    let modifier = Modifier::empty().weight_with_fill(2.0, false);
    let props = modifier.resolved_modifiers().layout_properties();
    let weight = props.weight().expect("weight to be recorded");
    assert_eq!(weight.weight, 2.0);
    assert!(!weight.fill);
}

#[test]
fn offset_accumulates_across_chain() {
    let modifier = Modifier::empty()
        .offset(4.0, 6.0)
        .then(Modifier::empty().absolute_offset(-1.5, 2.5))
        .then(Modifier::empty().offset(0.5, -3.0));
    let total = modifier.resolved_modifiers().offset();
    assert_eq!(total, Point { x: 3.0, y: 5.5 });
}

#[test]
fn then_short_circuits_empty_modifiers() {
    let padding = Modifier::empty().padding(4.0);
    assert_eq!(Modifier::empty().then(padding.clone()), padding);

    let background = Modifier::empty().background(Color::rgba(0.2, 0.4, 0.6, 1.0));
    assert_eq!(background.then(Modifier::empty()), background);
}

#[test]
fn required_size_sets_explicit_constraints() {
    let modifier = Modifier::empty().required_size(Size {
        width: 32.0,
        height: 18.0,
    });
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Points(32.0));
    assert_eq!(props.height(), DimensionConstraint::Points(18.0));
    assert_eq!(props.min_width(), Some(32.0));
    assert_eq!(props.max_width(), Some(32.0));
    assert_eq!(props.min_height(), Some(18.0));
    assert_eq!(props.max_height(), Some(18.0));
}

#[test]
fn alignment_modifiers_record_values() {
    let modifier = Modifier::empty()
        .align(Alignment::BOTTOM_END)
        .alignInColumn(HorizontalAlignment::CenterHorizontally)
        .alignInRow(VerticalAlignment::Top);
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.box_alignment(), Some(Alignment::BOTTOM_END));
    assert_eq!(
        props.column_alignment(),
        Some(HorizontalAlignment::CenterHorizontally)
    );
    assert_eq!(props.row_alignment(), Some(VerticalAlignment::Top));
}

#[test]
fn graphics_layer_modifier_creates_node() {
    use crate::modifier::ModifierChainHandle;
    use crate::modifier_nodes::GraphicsLayerNode;

    let layer = GraphicsLayer {
        alpha: 0.5,
        ..Default::default()
    };
    let modifier = Modifier::empty().graphics_layer(layer);

    // Graphics layer is now tracked in the modifier node chain, not ResolvedModifiers
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    // Verify the node exists in the chain by checking for DRAW capability
    let chain = handle.chain();
    let mut has_graphics_layer = false;
    chain.for_each_node_with_capability(
        compose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if node.as_any().downcast_ref::<GraphicsLayerNode>().is_some() {
                has_graphics_layer = true;
            }
        },
    );
    assert!(has_graphics_layer, "Expected GraphicsLayerNode in chain");
}

#[test]
fn collect_inspector_records_include_weight_and_pointer_input_metadata() {
    let modifier = Modifier::empty()
        .padding(2.0)
        .then(Modifier::empty().weight_with_fill(3.5, false))
        .then(Modifier::empty().pointer_input(7u64, |_| async move {}));

    let records = modifier.collect_inspector_records();

    let weight = records
        .iter()
        .find(|record| record.name == "weight")
        .expect("missing weight inspector record");
    assert!(weight
        .properties
        .iter()
        .any(|prop| prop.name == "weight" && prop.value == 3.5f32.to_string()));
    assert!(weight
        .properties
        .iter()
        .any(|prop| prop.name == "fill" && prop.value == "false"));

    let pointer = records
        .iter()
        .find(|record| record.name == "pointerInput")
        .expect("missing pointerInput inspector record");
    assert!(pointer
        .properties
        .iter()
        .any(|prop| prop.name == "keyCount" && prop.value == "1"));
    assert!(pointer
        .properties
        .iter()
        .any(|prop| prop.name == "handlerId"));
}

#[test]
fn semantics_modifier_populates_inspector_metadata() {
    let modifier = Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
        config.content_description = Some("Submit".into());
        config.is_button = true;
    });

    let records = modifier.collect_inspector_records();
    let semantics = records
        .first()
        .expect("expected semantics inspector record");
    assert_eq!(semantics.name, "semantics");
    assert!(semantics
        .properties
        .iter()
        .any(|prop| prop.name == "contentDescription" && prop.value == "Submit"));
    assert!(semantics
        .properties
        .iter()
        .any(|prop| prop.name == "isButton" && prop.value == "true"));
}

#[test]
fn inspector_snapshot_includes_delegate_depth_and_capabilities() {
    let modifier = Modifier::empty().padding(4.0).then(
        Modifier::with_element(TestDelegatingElement)
            .with_inspector_metadata(inspector_metadata("delegating", |info| {
                info.add_property("tag", "root")
            })),
    );
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let snapshot = handle.inspector_snapshot();
    assert!(snapshot.iter().any(|node| node.depth > 0));
    let padding_entry = snapshot
        .iter()
        .find(|node| {
            node.inspector
                .as_ref()
                .map(|record| record.name == "padding")
                .unwrap_or(false)
        })
        .expect("expected padding inspector entry");
    assert!(padding_entry
        .capabilities
        .contains(NodeCapabilities::LAYOUT));
}

#[test]
fn modifier_chain_trace_runs_only_when_debug_flag_set() {
    let modifier = Modifier::empty().padding(1.0);
    let mut handle = ModifierChainHandle::new();
    let invocations = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    {
        let counter = invocations.clone();
        let _guard = crate::debug::install_modifier_chain_trace(move |_nodes| {
            *counter.lock().unwrap() += 1;
        });

        let _ = handle.update(&modifier);
        assert_eq!(
            *invocations.lock().unwrap(),
            0,
            "trace should be gated by debug flag"
        );

        handle.set_debug_logging(true);
        let _ = handle.update(&modifier);
    }

    assert_eq!(*invocations.lock().unwrap(), 1);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TestDelegatingElement;

struct TestDelegatingNode {
    state: NodeState,
    delegate: TestDelegateLeaf,
}

impl TestDelegatingNode {
    fn new() -> Self {
        let node = Self {
            state: NodeState::new(),
            delegate: TestDelegateLeaf::new(),
        };
        node.state
            .set_capabilities(NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS);
        node.delegate
            .node_state()
            .set_capabilities(NodeCapabilities::LAYOUT);
        node
    }
}

impl DelegatableNode for TestDelegatingNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestDelegatingNode {
    fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
        visitor(&self.delegate);
    }
}

struct TestDelegateLeaf {
    state: NodeState,
}

impl TestDelegateLeaf {
    fn new() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for TestDelegateLeaf {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestDelegateLeaf {}

impl ModifierNodeElement for TestDelegatingElement {
    type Node = TestDelegatingNode;

    fn create(&self) -> Self::Node {
        TestDelegatingNode::new()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
