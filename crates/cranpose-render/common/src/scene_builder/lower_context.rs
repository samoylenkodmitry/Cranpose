// What a layout node lowers with from the nodes above it.

use cranpose_ui::{ModifierNodeSlices, Point};

use super::{AbsOrigin, RecordingStore, TranslateAncestorContext};

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

    pub(super) fn for_children(
        self,
        slices: &ModifierNodeSlices,
        content_offset: Point,
        parent_abs: Option<AbsOrigin>,
    ) -> Self {
        Self {
            inherited_motion_context_animated: self.inherited_motion_context_animated
                || slices.motion_context_animated(),
            inherited_translated_content_context: self.inherited_translated_content_context
                || slices.translated_content_context(),
            parent_abs,
            parent_content_offset: content_offset,
            store: self.store,
        }
    }
}
