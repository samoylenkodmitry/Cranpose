//! The frame's layout phase: lays out what composition left, then
//! recomposes the scopes the layout invalidated and lays those out.

use std::fmt::Debug;

use cranpose_core::{MemoryApplier, NodeError, NodeId};
use cranpose_render_common::Renderer;
use cranpose_ui::{
    LayoutNode, MeasureLayoutOptions, SubcomposeLayoutNode, request_render_invalidation,
};

use super::{Composed, FrameStages, LaidOut};
use crate::{AppShell, surface::route_geometry_scene_nodes};

fn pending_repass_nodes(context: &cranpose_ui::AppContext) -> Vec<NodeId> {
    let mut nodes = if context.has_pending_layout_repasses() {
        context.pending_layout_repass_nodes_snapshot()
    } else {
        Vec::new()
    };
    if context.has_pending_measure_repasses() {
        for node in context.pending_measure_repass_nodes_snapshot() {
            if !nodes.contains(&node) {
                nodes.push(node);
            }
        }
    }
    nodes
}

fn mark_root_for_layout(applier: &mut MemoryApplier, root: NodeId) {
    match applier.with_node::<LayoutNode, _>(root, |node| {
        node.mark_needs_measure();
        node.mark_needs_layout();
    }) {
        Ok(()) | Err(NodeError::Missing { .. }) => {}
        Err(NodeError::TypeMismatch { .. }) => {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(root, |node| {
                node.mark_needs_measure();
                node.mark_needs_layout_flag();
            });
        }
        Err(_) => {}
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    #[cfg(test)]
    pub(crate) fn run_layout_phase(&mut self) {
        let _app_context = self.app.app_context.enter_scope();
        self.run_layout_phase_in_context();
    }

    pub(crate) fn run_layout_phase_in_context(&mut self) {
        let context = &self.app.app_context;
        let has_scoped_repasses =
            context.has_pending_layout_repasses() || context.has_pending_measure_repasses();
        let scoped_layout_nodes = pending_repass_nodes(context);
        let global_layout_invalidation = context.take_layout_invalidation();
        let force_layout_pass = self.app.force_layout_pass;

        if global_layout_invalidation {
            cranpose_ui::layout::invalidate_all_layout_caches();
            if let Some(root) = self.app.composition.root() {
                mark_root_for_layout(&mut self.app.composition.applier_mut(), root);
            }
            self.app.request_forced_layout_pass();
        } else if has_scoped_repasses {
            self.app.request_layout_pass();
        }

        if !self.app.layout_requested {
            return;
        }

        let Some(root) = self.app.composition.root() else {
            self.reset_layout_snapshots();
            return;
        };
        let viewport_size = self.surfaces[0].viewport_size();
        let handle = self.app.composition.runtime_handle();
        let mut applier = self.app.composition.applier_mut();
        applier.set_runtime_handle(handle);

        let tree_needs_layout_check = cranpose_ui::tree_needs_layout(&mut *applier, root)
            .unwrap_or_else(|err| {
                log::warn!("Cannot check layout dirty status for root #{root}: {err}");
                true
            });
        let needs_layout =
            self.app.force_layout_pass || has_scoped_repasses || tree_needs_layout_check;
        self.app.layout_requested = false;
        self.app.force_layout_pass = false;

        if !needs_layout {
            log::trace!("Skipping layout: tree is clean");
            applier.clear_runtime_handle();
            return;
        }

        let measured = cranpose_ui::measure_layout_with_options(
            &mut applier,
            root,
            viewport_size,
            MeasureLayoutOptions {
                collect_semantics: false,
                build_layout_tree: false,
            },
        );
        applier.clear_runtime_handle();
        drop(applier);
        match measured {
            Ok(_measurements) => {
                self.forget_frame_snapshots();
                self.record_layout_scene_nodes(
                    global_layout_invalidation || force_layout_pass,
                    has_scoped_repasses,
                    scoped_layout_nodes,
                );
            }
            Err(err) => {
                log::error!("failed to compute layout: {err}");
                self.reset_layout_snapshots();
            }
        }
    }

    fn forget_frame_snapshots(&mut self) {
        self.app.semantics_snapshot_revision = self.app.semantics_snapshot_revision.wrapping_add(1);
        for surface in &mut self.surfaces {
            surface.forget_snapshots();
        }
    }

    fn reset_layout_snapshots(&mut self) {
        self.forget_frame_snapshots();
        for surface in &mut self.surfaces {
            surface.invalidate_scene_root(surface.root_node(&self.app));
        }
        let _ = cranpose_ui::take_geometry_scene_nodes();
        self.app.layout_requested = false;
        self.app.force_layout_pass = false;
    }

    fn record_layout_scene_nodes(
        &mut self,
        global: bool,
        has_scoped_repasses: bool,
        scoped_layout_nodes: Vec<NodeId>,
    ) {
        self.app
            .app_context
            .take_geometry_scene_nodes_into(&mut self.geometry_scene_nodes);
        self.app.content_moved |= !self.geometry_scene_nodes.is_empty();
        if global {
            for surface in &mut self.surfaces {
                surface.invalidate_scene_root(surface.root_node(&self.app));
            }
        }
        // The scene updates the root of a global pass in place and keeps the
        // layers below it, so the nodes the pass reshaped or moved go to the
        // scene too: a padding that grew moves its node's children although
        // no child moved in its parent.
        let scoped = (has_scoped_repasses && !global)
            .then_some(scoped_layout_nodes)
            .into_iter()
            .flatten();
        route_geometry_scene_nodes(
            &mut self.app,
            &mut self.surfaces,
            scoped.chain(self.geometry_scene_nodes.reshaped.iter().copied()),
            &self.geometry_scene_nodes.moved,
            &mut self.routing_scratch,
        );
    }

    // Layout, the second phase: lays out what composition left, then
    // recomposes the scopes the layout pass invalidated and lays those out.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a phase takes the token of the phase before it, which only that phase makes"
    )]
    pub(super) fn lay_out(&mut self, composed: Composed, stages: &mut FrameStages) -> LaidOut {
        self.run_layout_phase_in_context();
        stages.reached();
        let recomposed = composed.recomposed || self.run_post_layout_recomposition();
        stages.reached();
        LaidOut { recomposed }
    }

    fn run_post_layout_recomposition(&mut self) -> bool {
        self.app.composition.should_recompose() && self.recompose_after_layout()
    }

    /// Recomposes the scopes the layout pass invalidated and lays out again
    /// what they changed. Out of line, as a layout pass seldom invalidates
    /// any.
    #[inline(never)]
    fn recompose_after_layout(&mut self) -> bool {
        let Some(root_key) = self.app.composition.root_key() else {
            return false;
        };

        match self
            .app
            .composition
            .reconcile(root_key, &mut *self.app.content)
        {
            Ok(changed) => {
                if !changed {
                    return false;
                }
                self.app.fps_monitor.record_recomposition();
                self.sync_window_roots_in_context();
                if self.app.composition_tree_needs_layout() {
                    self.app.request_layout_pass();
                    self.run_layout_phase_in_context();
                }
                request_render_invalidation();
                true
            }
            Err(NodeError::Missing { id }) => {
                log::debug!("Post-layout recomposition skipped: node {id} no longer exists");
                self.app.request_layout_pass();
                request_render_invalidation();
                true
            }
            Err(err) => {
                log::error!("post-layout recomposition failed: {err}");
                self.app.request_layout_pass();
                request_render_invalidation();
                true
            }
        }
    }
}
