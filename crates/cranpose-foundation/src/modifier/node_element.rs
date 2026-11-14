// The typed modifier element: what makes and updates a node of a chain.

use std::{any::type_name, fmt, hash::Hash};

use cranpose_core::ProvidedValue;

use super::{InvalidationKind, ModifierNode, NodeCapabilities};

/// Strongly typed modifier elements that can create and update nodes while
/// exposing equality/hash/inspector contracts that mirror Jetpack Compose.
pub trait ModifierNodeElement: fmt::Debug + Hash + PartialEq + Sized + 'static {
    /// The type of node that the element creates and updates.
    type Node: ModifierNode;

    /// Makes the element again from a node it created or last updated, for
    /// an element whose node keeps every value of it and that asks for no
    /// update when equal. A chain keeps no such element beside its node: it
    /// compares a new element with the node. `None`, the default, keeps the
    /// element.
    const FROM_NODE: Option<fn(&Self::Node) -> Self> = None;

    /// Whether `node` keeps an element equal to this one, for an element
    /// with [`Self::FROM_NODE`]. An element whose node holds values a copy
    /// would count references of compares them in place.
    fn equals_node(&self, node: &Self::Node) -> bool {
        Self::FROM_NODE.is_some_and(|from_node| from_node(node) == *self)
    }

    /// Creates a new modifier node instance for this element.
    fn create(&self) -> Self::Node;

    /// Brings an existing modifier node up to date with the element's data.
    fn update(&self, node: &mut Self::Node);

    /// Optional key used to disambiguate multiple instances of the same element type.
    fn key(&self) -> Option<u64> {
        None
    }

    /// Human readable name surfaced to inspector tooling.
    fn inspector_name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Records inspector properties for tooling.
    fn inspector_properties(&self, _inspector: &mut dyn FnMut(&'static str, String)) {}

    /// Returns the capabilities of nodes created by this element.
    /// Override this to indicate which specialized traits the node implements.
    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }

    /// Whether this element requires `update` to be called even if `eq` returns true.
    ///
    /// This is useful for elements that ignore certain fields in `eq` (e.g. closures)
    /// to allow node reuse, but still need those fields updated in the existing node.
    /// Defaults to `false`.
    fn always_update(&self) -> bool {
        false
    }

    /// Whether modifier reconciliation should request capability-wide invalidations
    /// after updating an existing node.
    fn auto_invalidate_on_update(&self) -> bool {
        true
    }

    /// Optional targeted invalidation requested after updating an existing node.
    ///
    /// This is for nodes whose attach/remove capability is broader than the
    /// work needed for a value-only update. For example, an offset node
    /// participates in layout on attach but an x/y change only needs placement
    /// data and draw output refreshed.
    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        None
    }

    /// Composition locals this element hands to the content composed inside
    /// the node it decorates.
    ///
    /// A modifier that stands for something its subtree lives in, the way a
    /// window does, passes that thing down here instead of asking every
    /// composable in between to carry it as an argument.
    fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        Vec::new()
    }
}
