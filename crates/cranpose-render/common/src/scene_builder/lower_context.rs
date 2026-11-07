// What a layout node lowers with from the nodes above it.

use cranpose_ui::{ModifierNodeSlices, Point};

use super::{AbsOrigin, RecordingStore, TranslateAncestorContext, TranslateGeometry};
use crate::graph::LayerNode;

/// What a layout node lowers with from the nodes above it: where its
/// parent lies in the window and places its content, the motion and
/// translated content its ancestors set, and where its draws record.
#[derive(Clone, Copy)]
pub struct LowerContext {
    pub(super) inherited_motion_context_animated: bool,
    pub(super) inherited_translated_content_context: bool,
    pub(super) parent_abs: Option<AbsOrigin>,
    pub(super) parent_content_offset: Point,
    pub(super) store: RecordingStore,
}

impl LowerContext {
    pub(super) const ROOT: Self = Self::root(RecordingStore::Command);

    /// What a window's root node lowers with, its draws recording into
    /// `store`.
    pub const fn root(store: RecordingStore) -> Self {
        Self {
            inherited_motion_context_animated: false,
            inherited_translated_content_context: false,
            parent_abs: Some(AbsOrigin::ROOT),
            parent_content_offset: Point { x: 0.0, y: 0.0 },
            store,
        }
    }

    // What a child lowers with beneath `ancestors`.
    pub(super) fn beneath(ancestors: TranslateAncestorContext) -> Self {
        Self {
            inherited_motion_context_animated: ancestors.inherited_motion_context_animated,
            inherited_translated_content_context: ancestors.inherited_translated_content_context,
            parent_abs: Some(ancestors.parent_abs),
            parent_content_offset: ancestors.parent_content_offset,
            store: RecordingStore::Command,
        }
    }

    /// Whether the context knows where the parent lies in the window: a
    /// context made by [`LowerContext::without_window_origin`] does not.
    pub fn has_window_origin(&self) -> bool {
        self.parent_abs.is_some()
    }

    /// This context with no place in the window: a subtree whose nodes
    /// publish no window geometry lowers the same under it, and skips
    /// working out where each node lies in the window.
    pub fn without_window_origin(&self) -> Self {
        Self {
            parent_abs: None,
            ..*self
        }
    }

    // What the children of `layer`, the layer a node lowered under this
    // context and placed since where it now lies, lower with: what
    // `for_children` gives for the node, read from its layer.
    #[inline]
    pub(super) fn beneath_layer(&self, layer: &LayerNode) -> Self {
        Self {
            inherited_motion_context_animated: layer.motion_context_animated,
            inherited_translated_content_context: self.inherited_translated_content_context
                || layer.translated_content_context,
            parent_abs: self
                .parent_abs
                .as_ref()
                .map(|parent_abs| parent_abs.children_of(layer)),
            parent_content_offset: layer.content_offset,
            store: self.store,
        }
    }

    pub(super) fn for_children(
        &self,
        slices: &ModifierNodeSlices,
        content_offset: Point,
        geometry: Option<&TranslateGeometry>,
    ) -> Self {
        Self {
            inherited_motion_context_animated: self.inherited_motion_context_animated
                || slices.motion_context_animated(),
            inherited_translated_content_context: self.inherited_translated_content_context
                || slices.translated_content_context(),
            parent_abs: geometry.map(TranslateGeometry::child_abs),
            parent_content_offset: content_offset,
            store: self.store,
        }
    }
}
