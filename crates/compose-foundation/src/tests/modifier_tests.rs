use super::*;
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

fn count_nodes_with_capability(chain: &ModifierNodeChain, capability: NodeCapabilities) -> usize {
    let mut count = 0;
    chain.for_each_forward_matching(capability, |_node_ref| {
        // for_each_forward_matching only visits non-sentinel nodes, so we always count
        count += 1;
    });
    count
}

#[derive(Clone, Default)]
struct TestContext {
    invalidations: Rc<RefCell<Vec<ModifierInvalidation>>>,
    updates: Rc<RefCell<usize>>,
    active: Vec<NodeCapabilities>,
}

impl ModifierNodeContext for TestContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        let mut capabilities = self
            .active
            .last()
            .copied()
            .unwrap_or_else(NodeCapabilities::empty);
        capabilities.insert(NodeCapabilities::for_invalidation(kind));
        self.invalidations
            .borrow_mut()
            .push(ModifierInvalidation::new(kind, capabilities));
    }

    fn request_update(&mut self) {
        *self.updates.borrow_mut() += 1;
    }

    fn push_active_capabilities(&mut self, capabilities: NodeCapabilities) {
        self.active.push(capabilities);
    }

    fn pop_active_capabilities(&mut self) {
        self.active.pop();
    }
}

#[derive(Debug)]
struct LoggingNode {
    id: &'static str,
    log: Rc<RefCell<Vec<String>>>,
    value: i32,
    state: NodeState,
}

impl DelegatableNode for LoggingNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for LoggingNode {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
        self.log.borrow_mut().push(format!("attach:{}", self.id));
    }

    fn on_detach(&mut self) {
        self.log.borrow_mut().push(format!("detach:{}", self.id));
    }

    fn on_reset(&mut self) {
        self.log.borrow_mut().push(format!("reset:{}", self.id));
    }
}

#[derive(Debug, Clone)]
struct LoggingElement {
    id: &'static str,
    value: i32,
    log: Rc<RefCell<Vec<String>>>,
}

impl PartialEq for LoggingElement {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.value == other.value
    }
}

impl Eq for LoggingElement {}

impl Hash for LoggingElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.value.hash(state);
    }
}

impl ModifierNodeElement for LoggingElement {
    type Node = LoggingNode;

    fn create(&self) -> Self::Node {
        LoggingNode {
            id: self.id,
            log: self.log.clone(),
            value: self.value,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.value = self.value;
        self.log
            .borrow_mut()
            .push(format!("update:{}:{}", self.id, self.value));
    }

    fn key(&self) -> Option<u64> {
        let mut hasher = std::hash::DefaultHasher::new();
        self.id.hash(&mut hasher);
        Some(hasher.finish())
    }
}

#[test]
fn chain_attaches_updates_and_detaches_nodes() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let initial = vec![
        modifier_element(LoggingElement {
            id: "a",
            value: 1,
            log: log.clone(),
        }),
        modifier_element(LoggingElement {
            id: "b",
            value: 2,
            log: log.clone(),
        }),
    ];
    chain.update_from_slice(&initial, &mut context);
    assert_eq!(chain.len(), 2);
    assert_eq!(
        &*log.borrow(),
        &["attach:a", "update:a:1", "attach:b", "update:b:2"]
    );

    log.borrow_mut().clear();
    let updated = vec![
        modifier_element(LoggingElement {
            id: "a",
            value: 7,
            log: log.clone(),
        }),
        modifier_element(LoggingElement {
            id: "b",
            value: 9,
            log: log.clone(),
        }),
    ];
    chain.update_from_slice(&updated, &mut context);
    assert_eq!(chain.len(), 2);
    assert_eq!(&*log.borrow(), &["update:a:7", "update:b:9"]);
    assert_eq!(chain.node::<LoggingNode>(0).unwrap().value, 7);
    assert_eq!(chain.node::<LoggingNode>(1).unwrap().value, 9);

    log.borrow_mut().clear();
    let trimmed = vec![modifier_element(LoggingElement {
        id: "a",
        value: 11,
        log: log.clone(),
    })];
    chain.update_from_slice(&trimmed, &mut context);
    assert_eq!(chain.len(), 1);
    assert_eq!(&*log.borrow(), &["update:a:11", "detach:b"]);

    log.borrow_mut().clear();
    chain.reset();
    assert_eq!(&*log.borrow(), &["reset:a"]);

    log.borrow_mut().clear();
    chain.detach_all();
    assert!(chain.is_empty());
    assert_eq!(&*log.borrow(), &["detach:a"]);
}

#[test]
fn chain_reuses_nodes_when_reordered() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let initial = vec![
        modifier_element(LoggingElement {
            id: "a",
            value: 1,
            log: log.clone(),
        }),
        modifier_element(LoggingElement {
            id: "b",
            value: 2,
            log: log.clone(),
        }),
    ];
    chain.update_from_slice(&initial, &mut context);
    log.borrow_mut().clear();

    let reordered = vec![
        modifier_element(LoggingElement {
            id: "b",
            value: 5,
            log: log.clone(),
        }),
        modifier_element(LoggingElement {
            id: "a",
            value: 3,
            log: log.clone(),
        }),
    ];
    chain.update_from_slice(&reordered, &mut context);
    assert_eq!(&*log.borrow(), &["update:b:5", "update:a:3"]);
    assert_eq!(chain.node::<LoggingNode>(0).unwrap().id, "b");
    assert_eq!(chain.node::<LoggingNode>(1).unwrap().id, "a");

    log.borrow_mut().clear();
    chain.detach_all();
    assert_eq!(&*log.borrow(), &["detach:b", "detach:a"]);
}

#[derive(Debug)]
struct EqualityNode {
    value: i32,
    state: NodeState,
}

impl DelegatableNode for EqualityNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for EqualityNode {}

#[derive(Debug, Clone)]
struct EqualityElement {
    value: i32,
    updates: Rc<Cell<usize>>,
}

impl PartialEq for EqualityElement {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for EqualityElement {}

impl Hash for EqualityElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl ModifierNodeElement for EqualityElement {
    type Node = EqualityNode;

    fn create(&self) -> Self::Node {
        EqualityNode {
            value: self.value,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.value = self.value;
        self.updates.set(self.updates.get() + 1);
    }
}

#[test]
fn element_equality_controls_node_reuse() {
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let updates = Rc::new(Cell::new(0));
    let initial = vec![modifier_element(EqualityElement {
        value: 1,
        updates: updates.clone(),
    })];
    chain.update_from_slice(&initial, &mut context);
    let first_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };
    updates.set(0);

    let same = vec![modifier_element(EqualityElement {
        value: 1,
        updates: updates.clone(),
    })];
    chain.update_from_slice(&same, &mut context);
    let reused_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };
    assert_eq!(
        first_ptr, reused_ptr,
        "nodes should be reused when elements are equal"
    );
    assert_eq!(updates.get(), 0, "equal elements skip update invocations");

    let different = vec![modifier_element(EqualityElement {
        value: 2,
        updates: updates.clone(),
    })];
    chain.update_from_slice(&different, &mut context);
    let updated_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };
    assert_eq!(
        first_ptr, updated_ptr,
        "nodes should be reused even when element data changes"
    );
    assert_eq!(
        updates.get(),
        1,
        "value changes trigger a single update call"
    );
    assert_eq!(chain.node::<EqualityNode>(0).unwrap().value, 2);
}

#[test]
fn equality_matching_prefers_identical_elements_over_type_matches() {
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let updates = Rc::new(Cell::new(0));
    let initial = vec![
        modifier_element(EqualityElement {
            value: 1,
            updates: updates.clone(),
        }),
        modifier_element(EqualityElement {
            value: 2,
            updates: updates.clone(),
        }),
    ];
    chain.update_from_slice(&initial, &mut context);
    updates.set(0);

    let reordered = vec![
        modifier_element(EqualityElement {
            value: 2,
            updates: updates.clone(),
        }),
        modifier_element(EqualityElement {
            value: 3,
            updates: updates.clone(),
        }),
    ];
    chain.update_from_slice(&reordered, &mut context);

    assert_eq!(
        updates.get(),
        1,
        "only the node whose element changed should be updated"
    );
    assert_eq!(chain.node::<EqualityNode>(0).unwrap().value, 2);
    assert_eq!(chain.node::<EqualityNode>(1).unwrap().value, 3);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct KeyedElement {
    key: u64,
    value: i32,
}

impl ModifierNodeElement for KeyedElement {
    type Node = EqualityNode;

    fn create(&self) -> Self::Node {
        EqualityNode {
            value: self.value,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.value = self.value;
    }

    fn key(&self) -> Option<u64> {
        Some(self.key)
    }
}

#[test]
fn element_keys_gate_node_reuse() {
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let initial = vec![modifier_element(KeyedElement { key: 7, value: 1 })];
    chain.update_from_slice(&initial, &mut context);
    let first_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };

    let same_key = vec![modifier_element(KeyedElement { key: 7, value: 1 })];
    chain.update_from_slice(&same_key, &mut context);
    let reused_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };
    assert_eq!(first_ptr, reused_ptr, "matching keys should reuse nodes");

    let different_key = vec![modifier_element(KeyedElement { key: 8, value: 1 })];
    chain.update_from_slice(&different_key, &mut context);
    let replaced_ptr = {
        let node_ref = chain.node::<EqualityNode>(0).unwrap();
        &*node_ref as *const EqualityNode
    };
    assert_ne!(
        first_ptr, replaced_ptr,
        "changing keys should force recreation"
    );
}

#[test]
fn different_element_types_replace_nodes() {
    let mut chain = ModifierNodeChain::new();
    let mut context = TestContext::default();

    let layout = vec![modifier_element(TestLayoutElement)];
    chain.update_from_slice(&layout, &mut context);
    assert!(chain.node::<TestLayoutNode>(0).is_some());

    let draw = vec![modifier_element(TestDrawElement)];
    chain.update_from_slice(&draw, &mut context);
    assert!(chain.node::<TestLayoutNode>(0).is_none());
    assert!(chain.node::<TestDrawNode>(0).is_some());
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InspectorElement {
    label: &'static str,
    value: i32,
}

impl ModifierNodeElement for InspectorElement {
    type Node = EqualityNode;

    fn create(&self) -> Self::Node {
        EqualityNode {
            value: self.value,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.value = self.value;
    }

    fn inspector_name(&self) -> &'static str {
        self.label
    }

    fn inspector_properties(&self, inspector: &mut dyn FnMut(&'static str, String)) {
        inspector("value", self.value.to_string());
    }
}

#[test]
fn modifier_elements_expose_inspector_metadata() {
    let element = modifier_element(InspectorElement {
        label: "TestInspector",
        value: 42,
    });

    assert_eq!(element.inspector_name(), "TestInspector");

    let mut props = Vec::new();
    element.record_inspector_properties(&mut |name, value| props.push((name, value)));
    assert_eq!(props, vec![("value", "42".to_string())]);
}

#[derive(Debug, Clone)]
struct InvalidationElement {
    attach_count: Rc<Cell<usize>>,
}

impl PartialEq for InvalidationElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.attach_count, &other.attach_count)
    }
}

impl Hash for InvalidationElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.attach_count.as_ptr() as usize).hash(state);
    }
}

#[derive(Debug)]
struct InvalidationNode {
    state: NodeState,
}

impl Default for InvalidationNode {
    fn default() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for InvalidationNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNodeElement for InvalidationElement {
    type Node = InvalidationNode;

    fn create(&self) -> Self::Node {
        self.attach_count.set(self.attach_count.get() + 1);
        InvalidationNode::default()
    }

    fn update(&self, _node: &mut Self::Node) {}
}

impl ModifierNode for InvalidationNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Layout);
        context.invalidate(InvalidationKind::Draw);
        // Duplicate invalidations should be coalesced.
        context.invalidate(InvalidationKind::Layout);
        context.request_update();
    }
}

#[test]
fn basic_context_records_invalidations_and_updates() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let attaches = Rc::new(Cell::new(0));

    let elements = vec![modifier_element(InvalidationElement {
        attach_count: attaches.clone(),
    })];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(attaches.get(), 1);
    assert_eq!(
        context.invalidations(),
        &[
            ModifierInvalidation::new(InvalidationKind::Layout, NodeCapabilities::LAYOUT),
            ModifierInvalidation::new(InvalidationKind::Draw, NodeCapabilities::DRAW)
        ]
    );
    assert!(context.update_requested());

    let drained = context.take_invalidations();
    assert_eq!(
        drained,
        vec![
            ModifierInvalidation::new(InvalidationKind::Layout, NodeCapabilities::LAYOUT),
            ModifierInvalidation::new(InvalidationKind::Draw, NodeCapabilities::DRAW)
        ]
    );
    assert!(context.invalidations().is_empty());
    assert!(context.update_requested());
    assert!(context.take_update_requested());
    assert!(!context.update_requested());

    // Detach the existing chain to force new nodes on the next update.
    chain.detach_all();

    context.clear_invalidations();
    let elements = vec![modifier_element(InvalidationElement {
        attach_count: attaches.clone(),
    })];
    chain.update_from_slice(&elements, &mut context);
    assert_eq!(attaches.get(), 2);
    assert_eq!(
        context.invalidations(),
        &[
            ModifierInvalidation::new(InvalidationKind::Layout, NodeCapabilities::LAYOUT),
            ModifierInvalidation::new(InvalidationKind::Draw, NodeCapabilities::DRAW)
        ]
    );
}

// Test for specialized node traits
#[derive(Debug)]
struct TestLayoutNode {
    measure_count: Cell<usize>,
    state: NodeState,
}

impl Default for TestLayoutNode {
    fn default() -> Self {
        Self {
            measure_count: Cell::new(0),
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for TestLayoutNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestLayoutNode {
    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for TestLayoutNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        _constraints: Constraints,
    ) -> compose_ui_layout::LayoutModifierMeasureResult {
        self.measure_count.set(self.measure_count.get() + 1);
        compose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: 100.0,
            height: 100.0,
        })
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        50.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestLayoutElement;

impl ModifierNodeElement for TestLayoutElement {
    type Node = TestLayoutNode;

    fn create(&self) -> Self::Node {
        TestLayoutNode::default()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[derive(Debug)]
struct TestDrawNode {
    draw_count: Cell<usize>,
    state: NodeState,
}

impl Default for TestDrawNode {
    fn default() -> Self {
        Self {
            draw_count: Cell::new(0),
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for TestDrawNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestDrawNode {
    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for TestDrawNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        self.draw_count.set(self.draw_count.get() + 1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestDrawElement;

impl ModifierNodeElement for TestDrawElement {
    type Node = TestDrawNode;

    fn create(&self) -> Self::Node {
        TestDrawNode::default()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

#[derive(Debug)]
struct MaskOnlyNode {
    state: NodeState,
}

impl DelegatableNode for MaskOnlyNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for MaskOnlyNode {}

impl Default for MaskOnlyNode {
    fn default() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MaskOnlyElement;

impl ModifierNodeElement for MaskOnlyElement {
    type Node = MaskOnlyNode;

    fn create(&self) -> Self::Node {
        MaskOnlyNode::default()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct DelegatedDrawNode {
    id: &'static str,
    state: NodeState,
}

impl DelegatedDrawNode {
    fn new(id: &'static str) -> Self {
        let node = Self {
            id,
            state: NodeState::new(),
        };
        node.state
            .set_capabilities(NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS);
        node
    }
}

impl DelegatableNode for DelegatedDrawNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DelegatedDrawNode {
    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for DelegatedDrawNode {}

#[derive(Debug)]
#[allow(dead_code)]
struct DelegatingHostNode {
    id: &'static str,
    state: NodeState,
    delegate: DelegatedDrawNode,
}

impl DelegatingHostNode {
    fn new(id: &'static str, delegate_id: &'static str) -> Self {
        let node = Self {
            id,
            state: NodeState::new(),
            delegate: DelegatedDrawNode::new(delegate_id),
        };
        node.state
            .set_capabilities(NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS);
        node
    }

    fn delegate(&self) -> &DelegatedDrawNode {
        &self.delegate
    }
}

impl DelegatableNode for DelegatingHostNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DelegatingHostNode {
    fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
        visitor(&self.delegate);
    }

    fn for_each_delegate_mut<'a>(&'a mut self, visitor: &mut dyn FnMut(&'a mut dyn ModifierNode)) {
        visitor(&mut self.delegate);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DelegatingElement {
    host_id: &'static str,
    delegate_id: &'static str,
}

impl ModifierNodeElement for DelegatingElement {
    type Node = DelegatingHostNode;

    fn create(&self) -> Self::Node {
        DelegatingHostNode::new(self.host_id, self.delegate_id)
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS
    }
}

#[derive(Debug)]
struct DelegatedSemanticsNode {
    label: &'static str,
    state: NodeState,
}

impl DelegatedSemanticsNode {
    fn new(label: &'static str) -> Self {
        let node = Self {
            label,
            state: NodeState::new(),
        };
        node.state.set_capabilities(NodeCapabilities::SEMANTICS);
        node
    }
}

impl DelegatableNode for DelegatedSemanticsNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DelegatedSemanticsNode {
    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> {
        Some(self)
    }
}

impl SemanticsNode for DelegatedSemanticsNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        config.content_description = Some(self.label.to_string());
        config.is_clickable = true;
    }
}

#[derive(Debug)]
struct SemanticsDelegatingHostNode {
    state: NodeState,
    delegate: DelegatedSemanticsNode,
}

impl SemanticsDelegatingHostNode {
    fn new(_host_id: &'static str, label: &'static str) -> Self {
        let node = Self {
            state: NodeState::new(),
            delegate: DelegatedSemanticsNode::new(label),
        };
        node.state
            .set_capabilities(NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS);
        node.state.set_parent_link(None);
        node.state.set_child_link(None);
        node
    }
}

impl DelegatableNode for SemanticsDelegatingHostNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SemanticsDelegatingHostNode {
    fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
        visitor(&self.delegate);
    }

    fn for_each_delegate_mut<'a>(&'a mut self, visitor: &mut dyn FnMut(&'a mut dyn ModifierNode)) {
        visitor(&mut self.delegate);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SemanticsDelegatingElement {
    host_id: &'static str,
    label: &'static str,
}

impl ModifierNodeElement for SemanticsDelegatingElement {
    type Node = SemanticsDelegatingHostNode;

    fn create(&self) -> Self::Node {
        SemanticsDelegatingHostNode::new(self.host_id, self.label)
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct DelegatedPointerNode {
    label: &'static str,
    state: NodeState,
}

impl DelegatedPointerNode {
    fn new(label: &'static str) -> Self {
        let node = Self {
            label,
            state: NodeState::new(),
        };
        node.state.set_capabilities(NodeCapabilities::POINTER_INPUT);
        node
    }
}

impl DelegatableNode for DelegatedPointerNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DelegatedPointerNode {
    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }
}

impl PointerInputNode for DelegatedPointerNode {}

#[derive(Debug)]
struct PointerDelegatingHostNode {
    state: NodeState,
    delegate: DelegatedPointerNode,
}

impl PointerDelegatingHostNode {
    fn new(_host_id: &'static str, label: &'static str) -> Self {
        let node = Self {
            state: NodeState::new(),
            delegate: DelegatedPointerNode::new(label),
        };
        node.state
            .set_capabilities(NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS);
        node
    }
}

impl DelegatableNode for PointerDelegatingHostNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for PointerDelegatingHostNode {
    fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
        visitor(&self.delegate);
    }

    fn for_each_delegate_mut<'a>(&'a mut self, visitor: &mut dyn FnMut(&'a mut dyn ModifierNode)) {
        visitor(&mut self.delegate);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PointerDelegatingElement {
    host_id: &'static str,
    label: &'static str,
}

impl ModifierNodeElement for PointerDelegatingElement {
    type Node = PointerDelegatingHostNode;

    fn create(&self) -> Self::Node {
        PointerDelegatingHostNode::new(self.host_id, self.label)
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[test]
fn chain_tracks_node_capabilities() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 2);
    assert!(chain.has_nodes_for_invalidation(InvalidationKind::Layout));
    assert!(chain.has_nodes_for_invalidation(InvalidationKind::Draw));
    assert!(!chain.has_nodes_for_invalidation(InvalidationKind::PointerInput));
    assert!(!chain.has_nodes_for_invalidation(InvalidationKind::Semantics));
    assert_eq!(
        chain.capabilities(),
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW
    );

    // Verify we can iterate over layout and draw nodes separately
    assert_eq!(
        count_nodes_with_capability(&chain, NodeCapabilities::LAYOUT),
        1
    );
    assert_eq!(
        count_nodes_with_capability(&chain, NodeCapabilities::DRAW),
        1
    );
    assert_eq!(
        count_nodes_with_capability(&chain, NodeCapabilities::POINTER_INPUT),
        0
    );
    assert_eq!(
        count_nodes_with_capability(&chain, NodeCapabilities::SEMANTICS),
        0
    );
}

#[test]
fn for_each_node_with_capability_visits_mask_only_nodes() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(
        &[modifier_element(MaskOnlyElement)],
        &mut context as &mut dyn ModifierNodeContext,
    );
    let mut visited = 0;
    chain.for_each_node_with_capability(NodeCapabilities::DRAW, |_node_ref, _node| {
        visited += 1;
    });
    assert_eq!(visited, 1, "mask-only nodes should still be visited");
}

#[test]
fn sentinel_links_follow_chain_order() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let head = chain.head();
    assert!(head.is_head());
    let first = head.child().expect("head should link to first node");
    assert!(!first.is_sentinel());
    assert!(first.parent().unwrap().is_head());

    // Verify first node is layout using with_node closure
    let is_layout = first
        .with_node(|node| node.as_any().downcast_ref::<TestLayoutNode>().is_some())
        .unwrap_or(false);
    assert!(is_layout, "first node should be layout");

    let second = first.child().expect("first should link to draw node");

    // Verify second node is draw using with_node closure
    let is_draw = second
        .with_node(|node| node.as_any().downcast_ref::<TestDrawNode>().is_some())
        .unwrap_or(false);
    assert!(is_draw,
        "second node should be draw"
    );
    assert!(second.child().unwrap().is_tail());

    let tail = chain.tail();
    assert!(tail.is_tail());
    let tail_parent = tail.parent().expect("tail should have parent");

    let is_draw = tail_parent
        .with_node(|node| node.as_any().downcast_ref::<TestDrawNode>().is_some())
        .unwrap_or(false);
    assert!(is_draw, "tail parent should be the draw node");
    assert!(tail.child().is_none());
}

#[test]
fn aggregated_child_capabilities_match_descendants() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let expected = NodeCapabilities::LAYOUT | NodeCapabilities::DRAW;
    assert_eq!(chain.head().aggregate_child_capabilities(), expected);

    let first = chain.head().child().unwrap();
    assert_eq!(
        first.aggregate_child_capabilities(),
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW
    );

    let second = first.child().unwrap();
    assert_eq!(
        second.aggregate_child_capabilities(),
        NodeCapabilities::DRAW
    );

    assert_eq!(
        chain.tail().aggregate_child_capabilities(),
        NodeCapabilities::empty()
    );
}

#[test]
fn delegate_nodes_participate_in_traversal() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(DelegatingElement {
            host_id: "host",
            delegate_id: "delegate",
        }),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let order: Vec<&'static str> = chain
        .head_to_tail()
        .map(|node_ref| {
            node_ref.with_node(|node| {
                if node.as_any().downcast_ref::<DelegatingHostNode>().is_some() {
                    "host"
                } else if node.as_any().downcast_ref::<DelegatedDrawNode>().is_some() {
                    "delegate"
                } else if node.as_any().downcast_ref::<TestDrawNode>().is_some() {
                    "draw"
                } else {
                    "unknown"
                }
            }).unwrap_or("unknown")
        })
        .collect();

    assert_eq!(order, vec!["host", "delegate", "draw"]);
}

#[test]
fn delegate_capabilities_propagate() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![modifier_element(DelegatingElement {
        host_id: "host",
        delegate_id: "delegate",
    })];
    chain.update_from_slice(&elements, &mut context);

    assert!(chain.capabilities().contains(NodeCapabilities::LAYOUT));
    assert!(chain.capabilities().contains(NodeCapabilities::DRAW));

    let mut draw_nodes = 0;
    chain.for_each_forward_matching(NodeCapabilities::DRAW, |node_ref| {
        let is_delegated = node_ref
            .with_node(|node| node.as_any().downcast_ref::<DelegatedDrawNode>().is_some())
            .unwrap_or(false);
        if is_delegated {
            draw_nodes += 1;
        }
    });
    assert_eq!(draw_nodes, 1, "expected delegated draw node to be visited");
}

#[test]
fn semantics_delegates_are_visited_by_forward_matching() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![modifier_element(SemanticsDelegatingElement {
        host_id: "host",
        label: "delegate",
    })];
    chain.update_from_slice(&elements, &mut context);

    assert!(
        chain.capabilities().contains(NodeCapabilities::SEMANTICS),
        "semantics capability should propagate from delegate"
    );

    let mut visits = 0;
    chain.for_each_forward_matching(NodeCapabilities::SEMANTICS, |node_ref| {
        let is_delegated = node_ref
            .with_node(|node| node.as_any().downcast_ref::<DelegatedSemanticsNode>().is_some())
            .unwrap_or(false);
        if is_delegated {
            visits += 1;
        }
    });
    assert_eq!(visits, 1, "delegated semantics node should be visited");

    let mut config = SemanticsConfiguration {
        content_description: None,
        is_button: false,
        is_clickable: false,
    };
    chain.for_each_forward_matching(NodeCapabilities::SEMANTICS, |node_ref| {
        node_ref.with_node(|node| {
            if let Some(semantics_node) = node.as_semantics_node() {
                semantics_node.merge_semantics(&mut config);
            }
        });
    });
    assert_eq!(
        config.content_description.as_deref(),
        Some("delegate"),
        "delegate semantics should merge into config"
    );
    assert!(config.is_clickable, "semantics merge should set flag");
}

#[test]
fn pointer_delegates_are_visited_by_forward_matching() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![modifier_element(PointerDelegatingElement {
        host_id: "host",
        label: "delegate",
    })];
    chain.update_from_slice(&elements, &mut context);

    assert!(
        chain
            .capabilities()
            .contains(NodeCapabilities::POINTER_INPUT),
        "pointer capability should include delegated nodes"
    );

    let mut visits = 0;
    chain.for_each_forward_matching(NodeCapabilities::POINTER_INPUT, |node_ref| {
        let is_delegated = node_ref
            .with_node(|node| node.as_any().downcast_ref::<DelegatedPointerNode>().is_some())
            .unwrap_or(false);
        if is_delegated {
            visits += 1;
        }
    });
    assert_eq!(visits, 1, "delegated pointer node should be visited");
}

#[test]
fn delegate_parent_links_owner() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(DelegatingElement {
            host_id: "host",
            delegate_id: "delegate",
        }),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let delegate_ptr = {
        let host = chain
            .node::<DelegatingHostNode>(0)
            .expect("host node should exist");
        host.delegate() as *const dyn ModifierNode as *const ()
    };

    // Find delegate by comparing pointers since we can't hold the reference
    let delegate_ref = chain
        .head_to_tail()
        .find(|node_ref| {
            node_ref.with_node(|node| {
                node as *const dyn ModifierNode as *const () == delegate_ptr
            }).unwrap_or(false)
        })
        .expect("delegate should be discoverable");

    let parent = delegate_ref.parent().expect("delegate should have parent");
    let is_host = parent
        .with_node(|node| node.as_any().downcast_ref::<DelegatingHostNode>().is_some())
        .unwrap_or(false);
    assert!(is_host);

    let after_delegate = delegate_ref
        .child()
        .expect("delegate should have next node");
    let is_draw = after_delegate
        .with_node(|node| node.as_any().downcast_ref::<TestDrawNode>().is_some())
        .unwrap_or(false);
    assert!(is_draw);
}

#[test]
fn sentinel_links_exist_for_empty_chain() {
    let chain = ModifierNodeChain::new();
    let head_child = chain.head().child().expect("head should link to tail");
    assert!(head_child.is_tail());
    assert!(chain.tail().parent().unwrap().is_head());
    assert_eq!(
        chain.head().aggregate_child_capabilities(),
        NodeCapabilities::empty()
    );
}

#[test]
fn chain_iterators_follow_expected_order() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let forward: Vec<&'static str> = chain
        .head_to_tail()
        .map(|node_ref| {
            node_ref.with_node(|node| {
                if node.as_any().downcast_ref::<TestLayoutNode>().is_some() {
                    "layout"
                } else if node.as_any().downcast_ref::<TestDrawNode>().is_some() {
                    "draw"
                } else {
                    "unknown"
                }
            }).unwrap_or("unknown")
        })
        .collect();
    assert_eq!(forward, vec!["layout", "draw"]);

    let backward: Vec<&'static str> = chain
        .tail_to_head()
        .map(|node_ref| {
            node_ref.with_node(|node| {
                if node.as_any().downcast_ref::<TestLayoutNode>().is_some() {
                    "layout"
                } else if node.as_any().downcast_ref::<TestDrawNode>().is_some() {
                    "draw"
                } else {
                    "unknown"
                }
            }).unwrap_or("unknown")
        })
        .collect();
    assert_eq!(backward, vec!["draw", "layout"]);
}

#[test]
fn chain_can_find_node_refs() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let layout_node_guard = chain.node::<TestLayoutNode>(0).expect("layout node exists");
    let draw_node_guard = chain.node::<TestDrawNode>(1).expect("draw node exists");

    let layout_ref = chain
        .find_node_ref(&*layout_node_guard as &dyn ModifierNode)
        .expect("should resolve layout node ref");
    // Use entry_index() for navigation
    assert_eq!(layout_ref.entry_index(), Some(0));

    let draw_ref = chain
        .find_node_ref(&*draw_node_guard as &dyn ModifierNode)
        .expect("should resolve draw node ref");
    assert_eq!(draw_ref.entry_index(), Some(1));
}

#[test]
fn visit_descendants_matching_short_circuits() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let first = chain.head().child().expect("layout node present");

    let mut visited = Vec::new();
    first
        .clone()
        .visit_descendants_matching(false, NodeCapabilities::DRAW, |node| {
            node.with_node(|n| {
                if n.as_any().downcast_ref::<TestDrawNode>().is_some() {
                    visited.push("draw");
                }
            });
        });
    assert_eq!(visited, vec!["draw"]);

    let mut skipped = false;
    first.visit_descendants_matching(false, NodeCapabilities::SEMANTICS, |_| {
        skipped = true;
    });
    assert!(!skipped, "semantics mask should skip traversal");
}

#[test]
fn visit_ancestors_matching_includes_self() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![
        modifier_element(TestLayoutElement),
        modifier_element(TestDrawElement),
    ];
    chain.update_from_slice(&elements, &mut context);

    let draw_ref = chain
        .head()
        .child()
        .unwrap()
        .child()
        .expect("draw node present");
    let mut order = Vec::new();
    draw_ref.visit_ancestors(true, |node| {
        node.with_node(|n| {
            if n.as_any().downcast_ref::<TestDrawNode>().is_some() {
                order.push("draw");
            } else if n.as_any().downcast_ref::<TestLayoutNode>().is_some() {
                order.push("layout");
            }
        });
    });
    assert_eq!(order, vec!["draw", "layout"]);
}
