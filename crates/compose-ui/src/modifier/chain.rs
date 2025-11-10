use compose_foundation::{
    BasicModifierNodeContext, InvalidationKind, ModifierNode, ModifierNodeChain, NodeCapabilities,
};

use super::{
    local::ModifierLocalManager, Modifier, ModifierLocalAncestorResolver, ModifierLocalToken,
    ResolvedModifierLocal, ResolvedModifiers,
};
use crate::modifier_nodes::{BackgroundNode, CornerShapeNode, PaddingNode};
use std::cell::RefCell;
use std::rc::Rc;

/// Runtime helper that keeps a [`ModifierNodeChain`] in sync with a [`Modifier`].
///
/// This is the first step toward Jetpack Compose parity: callers can keep a handle
/// per layout node, feed it the latest `Modifier`, and then drive layout/draw/input
/// phases through the reconciled chain.
pub type ModifierLocalsHandle = Rc<RefCell<ModifierLocalManager>>;

#[allow(dead_code)]
pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    context: BasicModifierNodeContext,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
    aggregate_child_capabilities: NodeCapabilities,
    modifier_locals: ModifierLocalsHandle,
}

impl Default for ModifierChainHandle {
    fn default() -> Self {
        Self {
            chain: ModifierNodeChain::new(),
            context: BasicModifierNodeContext::new(),
            resolved: ResolvedModifiers::default(),
            capabilities: NodeCapabilities::default(),
            aggregate_child_capabilities: NodeCapabilities::default(),
            modifier_locals: Rc::new(RefCell::new(ModifierLocalManager::new())),
        }
    }
}

#[allow(dead_code)]
impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements stored in `modifier`.
    pub fn update(&mut self, modifier: &Modifier) -> Vec<InvalidationKind> {
        let mut resolver = |_: ModifierLocalToken| None;
        self.update_with_resolver(modifier, &mut resolver)
    }

    pub fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
    ) -> Vec<InvalidationKind> {
        self.chain
            .update_from_slice(modifier.elements(), &mut self.context);
        self.capabilities = self.chain.capabilities();
        self.aggregate_child_capabilities = self.chain.head().aggregate_child_capabilities();
        let modifier_local_invalidations = self
            .modifier_locals
            .borrow_mut()
            .sync(&self.chain, resolver);
        if std::env::var_os("COMPOSE_DEBUG_MODIFIERS").is_some() {
            crate::debug::log_modifier_chain(self.chain());
        }
        self.resolved = self.compute_resolved(modifier);
        modifier_local_invalidations
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Returns the aggregated capability mask for the reconciled chain.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.capabilities
    }

    /// Returns the aggregate child capability mask rooted at the sentinel head.
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.aggregate_child_capabilities
    }

    pub fn has_layout_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::DRAW)
    }

    pub fn has_pointer_input_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::POINTER_INPUT)
    }

    pub fn has_semantics_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::SEMANTICS)
    }

    /// Drains invalidations requested during the last update cycle.
    pub fn take_invalidations(&mut self) -> Vec<InvalidationKind> {
        self.context.take_invalidations()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved
    }

    pub fn resolve_modifier_local(
        &self,
        token: ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        self.modifier_locals.borrow().resolve(token)
    }

    pub fn modifier_locals_handle(&self) -> ModifierLocalsHandle {
        Rc::clone(&self.modifier_locals)
    }

    fn compute_resolved(&self, modifier: &Modifier) -> ResolvedModifiers {
        let mut resolved = ResolvedModifiers::default();
        let layout = modifier.layout_properties();
        resolved.set_layout_properties(layout);
        resolved.set_padding(layout.padding());
        resolved.set_offset(modifier.total_offset());
        resolved.set_graphics_layer(modifier.graphics_layer_values());

        if let Some(color) = modifier.background_color() {
            resolved.set_background_color(color);
        } else {
            resolved.clear_background();
        }
        resolved.set_corner_shape(modifier.corner_shape());

        if self.has_layout_nodes() {
            self.chain
                .for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
                    if let Some(node) = node_ref.node() {
                        if let Some(padding) = node.as_any().downcast_ref::<PaddingNode>() {
                            resolved.add_padding(padding.padding());
                        }
                    }
                });
        }

        if self.has_draw_nodes() {
            self.chain
                .for_each_forward_matching(NodeCapabilities::DRAW, |node_ref| {
                    if let Some(node) = node_ref.node() {
                        let any = node.as_any();
                        if let Some(background) = any.downcast_ref::<BackgroundNode>() {
                            resolved.set_background_color(background.color());
                        } else if let Some(shape) = any.downcast_ref::<CornerShapeNode>() {
                            resolved.set_corner_shape(Some(shape.shape()));
                        }
                    }
                });
        }

        resolved
    }
}

#[cfg(test)]
mod tests {
    use compose_foundation::{ModifierNode, NodeCapabilities};

    use super::*;
    use crate::modifier::{Color, RoundedCornerShape};
    use crate::modifier_nodes::PaddingNode;

    #[test]
    fn attaches_padding_node_and_invalidates_layout() {
        let mut handle = ModifierChainHandle::new();

        let _ = handle.update(&Modifier::padding(8.0));

        assert_eq!(handle.chain().len(), 1);

        let invalidations = handle.take_invalidations();
        assert_eq!(invalidations, vec![InvalidationKind::Layout]);
    }

    #[test]
    fn reuses_nodes_between_updates() {
        let mut handle = ModifierChainHandle::new();

        let _ = handle.update(&Modifier::padding(12.0));
        let first_ptr = node_ptr::<PaddingNode>(&handle);
        handle.take_invalidations();

        let _ = handle.update(&Modifier::padding(12.0));
        let second_ptr = node_ptr::<PaddingNode>(&handle);

        assert_eq!(first_ptr, second_ptr, "expected the node to be reused");
        assert!(
            handle.take_invalidations().is_empty(),
            "no additional invalidations should be issued for a pure update"
        );
    }

    #[test]
    fn resolved_modifiers_capture_background_and_shape() {
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(
            &Modifier::background(Color(0.2, 0.3, 0.4, 1.0)).then(Modifier::rounded_corners(8.0)),
        );
        let resolved = handle.resolved_modifiers();
        let background = resolved
            .background()
            .expect("expected resolved background entry");
        assert_eq!(background.color(), Color(0.2, 0.3, 0.4, 1.0));
        assert_eq!(
            resolved.corner_shape(),
            Some(RoundedCornerShape::uniform(8.0))
        );

        let _ = handle.update(
            &Modifier::rounded_corners(4.0).then(Modifier::background(Color(0.9, 0.1, 0.1, 1.0))),
        );
        let resolved = handle.resolved_modifiers();
        let background = resolved
            .background()
            .expect("background should be tracked after update");
        assert_eq!(background.color(), Color(0.9, 0.1, 0.1, 1.0));
        assert_eq!(
            resolved.corner_shape(),
            Some(RoundedCornerShape::uniform(4.0))
        );

        let _ = handle.update(&Modifier::empty());
        let resolved = handle.resolved_modifiers();
        assert!(resolved.background().is_none());
        assert!(resolved.corner_shape().is_none());
    }

    #[test]
    fn capability_mask_updates_with_chain() {
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(&Modifier::padding(4.0));
        assert_eq!(handle.capabilities(), NodeCapabilities::LAYOUT);
        assert!(handle.has_layout_nodes());
        assert!(!handle.has_draw_nodes());
        handle.take_invalidations();

        let color = Color(0.5, 0.6, 0.7, 1.0);
        let _ = handle.update(&Modifier::background(color));
        assert_eq!(handle.capabilities(), NodeCapabilities::DRAW);
        assert!(handle.has_draw_nodes());
        assert!(!handle.has_layout_nodes());
    }

    fn node_ptr<N: ModifierNode + 'static>(handle: &ModifierChainHandle) -> *const N {
        handle
            .chain()
            .node::<N>(0)
            .map(|node| node as *const N)
            .expect("expected node to exist")
    }
}
