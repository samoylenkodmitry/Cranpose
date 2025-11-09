use super::*;
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Default)]
struct TestContext {
    invalidations: Rc<RefCell<Vec<InvalidationKind>>>,
    updates: Rc<RefCell<usize>>,
}

impl ModifierNodeContext for TestContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        self.invalidations.borrow_mut().push(kind);
    }

    fn request_update(&mut self) {
        *self.updates.borrow_mut() += 1;
    }
}

#[derive(Debug)]
struct LoggingNode {
    id: &'static str,
    log: Rc<RefCell<Vec<String>>>,
    value: i32,
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

#[derive(Debug, Default, PartialEq)]
struct EqualityNode {
    value: i32,
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
        EqualityNode { value: self.value }
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
    let first_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;
    updates.set(0);

    let same = vec![modifier_element(EqualityElement {
        value: 1,
        updates: updates.clone(),
    })];
    chain.update_from_slice(&same, &mut context);
    let reused_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;
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
    let updated_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct KeyedElement {
    key: u64,
    value: i32,
}

impl ModifierNodeElement for KeyedElement {
    type Node = EqualityNode;

    fn create(&self) -> Self::Node {
        EqualityNode { value: self.value }
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
    let first_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;

    let same_key = vec![modifier_element(KeyedElement { key: 7, value: 1 })];
    chain.update_from_slice(&same_key, &mut context);
    let reused_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;
    assert_eq!(first_ptr, reused_ptr, "matching keys should reuse nodes");

    let different_key = vec![modifier_element(KeyedElement { key: 8, value: 1 })];
    chain.update_from_slice(&different_key, &mut context);
    let replaced_ptr = chain.node::<EqualityNode>(0).unwrap() as *const EqualityNode;
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
        EqualityNode { value: self.value }
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

#[derive(Debug, Default)]
struct InvalidationNode;

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
        &[InvalidationKind::Layout, InvalidationKind::Draw]
    );
    assert!(context.update_requested());

    let drained = context.take_invalidations();
    assert_eq!(
        drained,
        vec![InvalidationKind::Layout, InvalidationKind::Draw]
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
        &[InvalidationKind::Layout, InvalidationKind::Draw]
    );
}

// Test for specialized node traits
#[derive(Debug, Default)]
struct TestLayoutNode {
    measure_count: Cell<usize>,
}

impl ModifierNode for TestLayoutNode {}

impl LayoutModifierNode for TestLayoutNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        _constraints: Constraints,
    ) -> Size {
        self.measure_count.set(self.measure_count.get() + 1);
        Size {
            width: 100.0,
            height: 100.0,
        }
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
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

#[derive(Debug, Default)]
struct TestDrawNode {
    draw_count: Cell<usize>,
}

impl ModifierNode for TestDrawNode {}

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
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
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

    // Verify we can iterate over layout and draw nodes separately
    assert_eq!(chain.layout_nodes().count(), 1);
    assert_eq!(chain.draw_nodes().count(), 1);
    assert_eq!(chain.pointer_input_nodes().count(), 0);
    assert_eq!(chain.semantics_nodes().count(), 0);
}
