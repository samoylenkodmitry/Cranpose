use compose_foundation::{
    BasicModifierNodeContext, InvalidationKind, ModifierNode, ModifierNodeChain,
};

use super::{Color, Modifier, ResolvedModifiers, RoundedCornerShape};
use crate::modifier_nodes::{BackgroundNode, CornerShapeNode, PaddingNode};

/// Runtime helper that keeps a [`ModifierNodeChain`] in sync with a [`Modifier`].
///
/// This is the first step toward Jetpack Compose parity: callers can keep a handle
/// per layout node, feed it the latest `Modifier`, and then drive layout/draw/input
/// phases through the reconciled chain.
#[allow(dead_code)]
#[derive(Default)]
pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    context: BasicModifierNodeContext,
}

#[allow(dead_code)]
impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements stored in `modifier`.
    pub fn update(&mut self, modifier: &Modifier) {
        self.chain
            .update_from_slice(modifier.elements(), &mut self.context);
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Drains invalidations requested during the last update cycle.
    pub fn take_invalidations(&mut self) -> Vec<InvalidationKind> {
        self.context.take_invalidations()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        let mut resolved = ResolvedModifiers::default();
        for node in self.chain.layout_nodes() {
            if let Some(padding) = node.as_any().downcast_ref::<PaddingNode>() {
                resolved.add_padding(padding.padding());
            }
        }
        for node in self.chain.draw_nodes() {
            let any = node.as_any();
            if let Some(background) = any.downcast_ref::<BackgroundNode>() {
                resolved.set_background_color(background.color());
            } else if let Some(shape) = any.downcast_ref::<CornerShapeNode>() {
                resolved.set_corner_shape(Some(shape.shape()));
            }
        }

        resolved
    }
}

#[cfg(test)]
mod tests {
    use compose_foundation::ModifierNode;

    use super::*;
    use crate::modifier_nodes::PaddingNode;

    #[test]
    fn attaches_padding_node_and_invalidates_layout() {
        let mut handle = ModifierChainHandle::new();

        handle.update(&Modifier::padding(8.0));

        assert_eq!(handle.chain().len(), 1);

        let invalidations = handle.take_invalidations();
        assert_eq!(invalidations, vec![InvalidationKind::Layout]);
    }

    #[test]
    fn reuses_nodes_between_updates() {
        let mut handle = ModifierChainHandle::new();

        handle.update(&Modifier::padding(12.0));
        let first_ptr = node_ptr::<PaddingNode>(&handle);
        handle.take_invalidations();

        handle.update(&Modifier::padding(12.0));
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
        handle.update(
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

        handle.update(
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

        handle.update(&Modifier::empty());
        let resolved = handle.resolved_modifiers();
        assert!(resolved.background().is_none());
        assert!(resolved.corner_shape().is_none());
    }

    fn node_ptr<N: ModifierNode + 'static>(handle: &ModifierChainHandle) -> *const N {
        handle
            .chain()
            .node::<N>(0)
            .map(|node| node as *const N)
            .expect("expected node to exist")
    }
}
