//! A layout node's shell: the node a recycle pool keeps, which an emit fills
//! where it lies.

use std::rc::Rc;

use cranpose_ui_layout::MeasurePolicy;

use super::LayoutNode;
use crate::{
    layout::LayoutRuntimeState,
    modifier::Modifier,
    widgets::{layout::LayoutEnvironment, nodes::LayoutParts},
};

impl LayoutNode {
    // A shell for an emit that finds none pooled to fill.
    pub(crate) fn bare_shell() -> Self {
        Self::new_recycled_shell(false)
    }

    // Makes this shell the node a composition makes in `environment` with
    // `modifier` and `measure_policy`, in the place it lies, so its box takes
    // no new node whole. A shell holds a made node's values in every field
    // but these.
    pub(crate) fn fill(
        &mut self,
        modifier: &Modifier,
        measure_policy: Rc<dyn MeasurePolicy>,
        environment: LayoutEnvironment,
    ) {
        let Self {
            parts,
            children,
            density,
            is_virtual,
            needs_semantics,
            descendant_needs_semantics,
            needs_redraw,
            needs_pointer_pass,
            needs_focus_sync,
            modifier_slices_dirty,
            debug_modifiers,
            id,
            parent,
            folded_parent,
            modifier_slices_snapshot: _,
            semantics_reach,
            owner_context_id,
            modifier_chain,
            #[cfg(feature = "inspection")]
            source_trace,
        } = self;
        debug_assert!(
            children.is_empty()
                && id.get().is_none()
                && parent.get().is_none()
                && folded_parent.get().is_none()
                && owner_context_id.get().is_none()
                && semantics_reach.get().is_none()
                && !descendant_needs_semantics.get()
                && !needs_pointer_pass.get()
                && !needs_focus_sync.get()
                && !debug_modifiers.get()
                && modifier_slices_dirty.get(),
            "only a shell fills"
        );
        *parts = Rc::new(LayoutParts::new(LayoutRuntimeState::new(measure_policy)));
        *density = crate::density::shared_density(environment.density);
        *is_virtual = false;
        needs_semantics.set(true);
        needs_redraw.set(true);
        modifier_chain.set_layout_direction(environment.direction);
        #[cfg(feature = "inspection")]
        {
            *source_trace = cranpose_core::source_trace::current_source_trace();
        }
        self.sync_modifier_chain(modifier, None);
    }
}
