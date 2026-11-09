// The type-erased modifier element a chain reconciles with: what it tells of
// the node it makes, and how two compare.

use std::{
    any::{Any, TypeId, type_name},
    cell::{Cell, RefCell},
    fmt,
    hash::Hasher,
    marker::PhantomData,
    rc::Rc,
};

use cranpose_core::{PackedU64, ProvidedValue, hash::default};

use super::{InvalidationKind, ModifierNode, ModifierNodeElement, NodeCapabilities};

/// Type-erased modifier element used by the runtime to reconcile chains.
pub trait AnyModifierElement: fmt::Debug {
    /// The type of node that the element creates.
    fn node_type(&self) -> TypeId;

    /// The type of the element itself.
    fn element_type(&self) -> TypeId;

    /// A new node for this element, in the shared cell its chain entry keeps.
    fn create_node(&self) -> Rc<RefCell<dyn ModifierNode>>;

    /// Whether `node` is of the type that this element updates.
    fn can_update_node(&self, node: &dyn ModifierNode) -> bool;

    /// Brings `node` up to date with the data of this element.
    fn update_node(&self, node: &mut dyn ModifierNode);

    /// The key that tells apart several elements of one type, or `None` when the element has none.
    fn key(&self) -> Option<u64>;

    /// The capabilities of the nodes that the element creates.
    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }

    /// The hash of the element, computed once when the element was wrapped.
    fn hash_code(&self) -> u64;

    /// Whether `other` is an element of the same type with equal data.
    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool;

    /// The name that inspector tooling shows for the element.
    fn inspector_name(&self) -> &'static str;

    /// Reports each inspector property of the element to `visitor` as a name and a value.
    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String));

    /// Whether the node must be updated even when the new element equals the old one.
    fn requires_update(&self) -> bool;

    /// Whether updating an existing node invalidates all pipeline stages that its capabilities
    /// cover.
    fn auto_invalidates_on_update(&self) -> bool;

    /// The one pipeline stage that an update invalidates, or `None` to invalidate by capabilities.
    fn update_invalidation_kind(&self) -> Option<InvalidationKind>;

    /// Whether this element has composition locals for its content.
    fn provides_composition_locals(&self) -> bool {
        false
    }

    /// The composition locals this element hands to its content.
    fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        Vec::new()
    }

    /// Returns the element as `Any`, for downcasting to its concrete type.
    fn as_any(&self) -> &dyn Any;

    /// What stands for the element beside its node when the node keeps
    /// every value of it: see [`ModifierNodeElement::FROM_NODE`].
    fn held_by_node(&self) -> Option<NodeHeld> {
        None
    }

    /// Whether `node` is of this element's node type and keeps an element
    /// equal to this one: always `false` for an element whose node keeps
    /// none.
    fn equals_node(&self, _node: &dyn ModifierNode) -> bool {
        false
    }
}

/// What a chain keeps of an element whose node keeps every value of it, in
/// place of the element: its type, and how to read it from the node. One
/// word, so an entry keeps it where it keeps an element.
#[derive(Clone, Copy)]
pub struct NodeHeld(&'static HeldFns);

impl fmt::Debug for NodeHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeHeld").finish_non_exhaustive()
    }
}

impl NodeHeld {
    pub(crate) fn element_type(self) -> TypeId {
        (self.0.element_type)()
    }

    pub(crate) fn node_type(self) -> TypeId {
        (self.0.node_type)()
    }

    // The hash of the element `node` holds, as the element itself hashes.
    pub(crate) fn hash_code(self, node: &dyn ModifierNode) -> u64 {
        (self.0.hash_code)(node)
    }

    // The element `node` holds, made again.
    pub(crate) fn element(self, node: &dyn ModifierNode) -> DynModifierElement {
        (self.0.element)(node)
    }
}

struct HeldFns {
    element_type: fn() -> TypeId,
    node_type: fn() -> TypeId,
    hash_code: fn(&dyn ModifierNode) -> u64,
    element: fn(&dyn ModifierNode) -> DynModifierElement,
}

struct HeldBy<E>(PhantomData<fn() -> E>);

impl<E: ModifierNodeElement> HeldBy<E> {
    const FNS: HeldFns = HeldFns {
        element_type: TypeId::of::<E>,
        node_type: TypeId::of::<E::Node>,
        hash_code: Self::hash_code,
        element: Self::element,
    };

    // The element `node` holds. A chain keeps a `NodeHeld` only beside a
    // node of the element's node type.
    fn of(node: &dyn ModifierNode) -> E {
        let from_node = E::FROM_NODE.expect("a node-held element makes itself from its node");
        from_node(
            node.as_any()
                .downcast_ref::<E::Node>()
                .expect("a node-held element's node is of its node type"),
        )
    }

    fn hash_code(node: &dyn ModifierNode) -> u64 {
        let mut hasher = default::new();
        Self::of(node).hash(&mut hasher);
        hasher.finish()
    }

    fn element(node: &dyn ModifierNode) -> DynModifierElement {
        modifier_element(Self::of(node))
    }
}

struct TypedModifierElement<E: ModifierNodeElement> {
    element: E,
    /// The element's hash, worked out the first time a chain rebuilding
    /// asks for it: most elements are built for a recomposition and only
    /// compared with the last ones.
    hash: Cell<Option<PackedU64>>,
    provides_locals: bool,
}

impl<E: ModifierNodeElement> TypedModifierElement<E> {
    fn new(element: E) -> Self {
        let provides_locals = !element.provided_composition_locals().is_empty();
        Self {
            element,
            hash: Cell::new(None),
            provides_locals,
        }
    }
}

impl<E> fmt::Debug for TypedModifierElement<E>
where
    E: ModifierNodeElement,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedModifierElement")
            .field("type", &type_name::<E>())
            .finish()
    }
}

impl<E> AnyModifierElement for TypedModifierElement<E>
where
    E: ModifierNodeElement,
{
    fn node_type(&self) -> TypeId {
        TypeId::of::<E::Node>()
    }

    fn element_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn create_node(&self) -> Rc<RefCell<dyn ModifierNode>> {
        Rc::new(RefCell::new(self.element.create()))
    }

    fn can_update_node(&self, node: &dyn ModifierNode) -> bool {
        node.as_any().is::<E::Node>()
    }

    fn update_node(&self, node: &mut dyn ModifierNode) {
        if let Some(typed) = node.as_any_mut().downcast_mut::<E::Node>() {
            self.element.update(typed);
        }
    }

    fn key(&self) -> Option<u64> {
        self.element.key()
    }

    fn capabilities(&self) -> NodeCapabilities {
        self.element.capabilities()
    }

    fn provides_composition_locals(&self) -> bool {
        self.provides_locals
    }

    fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        self.element.provided_composition_locals()
    }

    fn hash_code(&self) -> u64 {
        if let Some(hash) = self.hash.get() {
            return hash.get();
        }
        let mut hasher = default::new();
        self.element.hash(&mut hasher);
        let hash = hasher.finish();
        self.hash.set(Some(PackedU64::new(hash)));
        hash
    }

    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|typed| typed.element == self.element)
    }

    fn inspector_name(&self) -> &'static str {
        self.element.inspector_name()
    }

    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String)) {
        self.element.inspector_properties(visitor);
    }

    fn requires_update(&self) -> bool {
        self.element.always_update()
    }

    fn auto_invalidates_on_update(&self) -> bool {
        self.element.auto_invalidate_on_update()
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        self.element.update_invalidation_kind()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn held_by_node(&self) -> Option<NodeHeld> {
        E::FROM_NODE.map(|_| NodeHeld(&HeldBy::<E>::FNS))
    }

    fn equals_node(&self, node: &dyn ModifierNode) -> bool {
        node.as_any()
            .downcast_ref::<E::Node>()
            .is_some_and(|node| self.element.equals_node(node))
    }
}

/// Convenience helper for callers to construct a type-erased modifier
/// element without having to mention the internal wrapper type.
pub fn modifier_element<E: ModifierNodeElement>(element: E) -> DynModifierElement {
    Rc::new(TypedModifierElement::new(element))
}

/// Boxed type-erased modifier element.
pub type DynModifierElement = Rc<dyn AnyModifierElement>;
