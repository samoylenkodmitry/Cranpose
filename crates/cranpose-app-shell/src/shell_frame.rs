use super::*;

mod dev_overlay;
use dev_overlay::draw_dev_overlay;

const DEFAULT_FRAME_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

#[derive(Copy, Clone, Debug)]
enum DispatchInvalidationKind {
    Pointer,
    Focus,
}

fn frame_stage_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        let explicit = std::env::var("CRANPOSE_FRAME_STAGE_TELEMETRY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0);
        explicit.or_else(|| {
            std::env::var_os("CRANPOSE_FRAME_STAGE_TELEMETRY")
                .is_some()
                .then_some(DEFAULT_FRAME_STAGE_TELEMETRY_THRESHOLD_MS)
        })
    })
}

/// The stages of a frame that its telemetry times: start, initial layout,
/// post-layout recomposition, dispatch and render.
type FrameStages = cranpose_core::StageTimes<5>;

fn log_frame_stage_telemetry(stages: &FrameStages) {
    let Some(threshold_ms) = frame_stage_telemetry_threshold_ms() else {
        return;
    };
    let Some(
        [
            frame_start,
            after_initial_layout,
            after_layout,
            after_dispatch,
            after_render,
        ],
    ) = stages.times()
    else {
        return;
    };
    let ms = cranpose_core::stage_ms;
    let total_ms = ms(frame_start, after_render);
    if total_ms < threshold_ms {
        return;
    }
    log::warn!(
        "[frame-stage-telemetry] total_ms={total_ms:.2} layout_ms={:.2} initial_layout_ms={:.2} post_layout_ms={:.2} dispatch_ms={:.2} scene_ms={:.2}",
        ms(frame_start, after_layout),
        ms(frame_start, after_initial_layout),
        ms(after_initial_layout, after_layout),
        ms(after_layout, after_dispatch),
        ms(after_dispatch, after_render),
    );
}

fn render_phase_dirty_diagnostics_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_RENDER_PHASE_DIRTY_DIAG")
}

struct RenderPhaseDirtyDiagnostics<'a> {
    render_dirty: bool,
    pointer_dirty: bool,
    scene_dirty: bool,
    draw_repass_pending: bool,
    draw_dirty_nodes: usize,
    layer_property_nodes: usize,
    layout_dirty_nodes: usize,
    moved_nodes: usize,
    structural_dirty_nodes: usize,
    partial_dirty_nodes: usize,
    dirty_node_ids: Option<String>,
    render_only_dirty: bool,
    recomposed_this_frame: bool,
    path: &'a str,
}

fn log_render_phase_dirty_diagnostics(diagnostics: RenderPhaseDirtyDiagnostics<'_>) {
    if !render_phase_dirty_diagnostics_enabled() {
        return;
    }

    let RenderPhaseDirtyDiagnostics {
        render_dirty,
        pointer_dirty,
        scene_dirty,
        draw_repass_pending,
        draw_dirty_nodes,
        layer_property_nodes,
        layout_dirty_nodes,
        moved_nodes,
        structural_dirty_nodes,
        partial_dirty_nodes,
        dirty_node_ids,
        render_only_dirty,
        recomposed_this_frame,
        path,
    } = diagnostics;
    if let Some(dirty_node_ids) = dirty_node_ids {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layer_property_nodes={layer_property_nodes} layout_dirty_nodes={layout_dirty_nodes} moved_nodes={moved_nodes} structural_dirty_nodes={structural_dirty_nodes} partial_dirty_nodes={partial_dirty_nodes} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame} ids={dirty_node_ids}",
        );
    } else {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layer_property_nodes={layer_property_nodes} layout_dirty_nodes={layout_dirty_nodes} moved_nodes={moved_nodes} structural_dirty_nodes={structural_dirty_nodes} partial_dirty_nodes={partial_dirty_nodes} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame}",
        );
    }
}

#[derive(Clone, Copy)]
struct FrameDirt {
    render_dirty: bool,
    pointer_dirty: bool,
    attributed: bool,
    layer_properties_pending: bool,
    recomposed_this_frame: bool,
}

struct SurfaceDirt {
    draw_dirty_nodes: Vec<NodeId>,
    layer_property_nodes: Vec<NodeId>,
    layout_dirty_nodes: Vec<NodeId>,
    moved_nodes: Vec<NodeId>,
    structural_parents: Vec<NodeId>,
    partial_dirty_nodes: Vec<NodeId>,
    draw_repass_pending: bool,
    render_only_dirty: bool,
    visual_only_partial_dirty: bool,
    full_scene_dirty: bool,
    needs_scene_rebuild: bool,
}

struct ScopedVisualNodes {
    draw: Vec<NodeId>,
    layers: Vec<NodeId>,
}

/// The nodes a surface's layout lanes hold: laid out again, and only moved.
struct ScopedLayoutNodes {
    reshaped: Vec<NodeId>,
    moved: Vec<NodeId>,
}

impl SurfaceDirt {
    fn classify(
        frame: &FrameDirt,
        scene_dirty: bool,
        draw_repass_pending: bool,
        visual_nodes: ScopedVisualNodes,
        layout: ScopedLayoutNodes,
        structural_parents: Vec<NodeId>,
        mut partial_dirty_nodes: Vec<NodeId>,
    ) -> Self {
        let ScopedVisualNodes {
            draw: draw_dirty_nodes,
            layers: layer_property_nodes,
        } = visual_nodes;
        let ScopedLayoutNodes {
            reshaped: layout_dirty_nodes,
            moved: moved_nodes,
        } = layout;
        let structural_dirty = !structural_parents.is_empty();
        // A node in two of the lists stays in both: the scene keys its
        // updates by node, so sorting every id to drop the second copy would
        // do that work twice.
        partial_dirty_nodes.clear();
        partial_dirty_nodes.extend(draw_dirty_nodes.iter().copied());
        partial_dirty_nodes.extend(layout_dirty_nodes.iter().copied());
        partial_dirty_nodes.extend(structural_parents.iter().copied());

        // Moved nodes are not part of the partial list: the scene moves them.
        let layout_changed = !layout_dirty_nodes.is_empty() || !moved_nodes.is_empty();
        let partial_scene_dirty = !partial_dirty_nodes.is_empty() || !moved_nodes.is_empty();
        let render_only_dirty = frame.render_dirty
            && !frame.attributed
            && !partial_scene_dirty
            && layer_property_nodes.is_empty()
            && !draw_repass_pending
            && !structural_dirty;
        let visual_only_partial_dirty = (!draw_dirty_nodes.is_empty()
            || !layer_property_nodes.is_empty())
            && !layout_changed
            && !frame.pointer_dirty
            && !frame.recomposed_this_frame
            && !structural_dirty;
        let scoped_scene_dirty = scene_dirty && layout_changed;
        let full_scene_dirty = scene_dirty && !scoped_scene_dirty && !visual_only_partial_dirty;
        // A scoped scene change always has nodes, so `partial_scene_dirty`
        // covers it.
        let needs_scene_rebuild = full_scene_dirty
            || partial_scene_dirty
            || !layer_property_nodes.is_empty()
            || draw_repass_pending
            || structural_dirty;
        Self {
            draw_dirty_nodes,
            layer_property_nodes,
            layout_dirty_nodes,
            moved_nodes,
            structural_parents,
            partial_dirty_nodes,
            draw_repass_pending,
            render_only_dirty,
            visual_only_partial_dirty,
            full_scene_dirty,
            needs_scene_rebuild,
        }
    }

    fn use_partial_update(&self) -> bool {
        (!self.partial_dirty_nodes.is_empty()
            || !self.layer_property_nodes.is_empty()
            || !self.moved_nodes.is_empty())
            && !self.render_only_dirty
            && !self.full_scene_dirty
    }

    fn use_visual_update(&self) -> bool {
        self.use_partial_update() && self.visual_only_partial_dirty
    }

    fn rebuild_path(&self) -> &'static str {
        if self.use_visual_update() {
            "visual-update"
        } else if self.use_partial_update() {
            "update"
        } else {
            "rebuild"
        }
    }

    fn log(&self, frame: &FrameDirt, scene_dirty: bool, path: &str) {
        log_render_phase_dirty_diagnostics(RenderPhaseDirtyDiagnostics {
            render_dirty: frame.render_dirty,
            pointer_dirty: frame.pointer_dirty,
            scene_dirty,
            draw_repass_pending: self.draw_repass_pending,
            draw_dirty_nodes: self.draw_dirty_nodes.len(),
            layer_property_nodes: self.layer_property_nodes.len(),
            layout_dirty_nodes: self.layout_dirty_nodes.len(),
            moved_nodes: self.moved_nodes.len(),
            structural_dirty_nodes: self.structural_parents.len(),
            partial_dirty_nodes: self.partial_dirty_nodes.len(),
            dirty_node_ids: render_phase_dirty_diagnostics_enabled().then(|| {
                format!(
                    "draw={:?} layers={:?} layout={:?} moved={:?} structural={:?} partial={:?}",
                    self.draw_dirty_nodes,
                    self.layer_property_nodes,
                    self.layout_dirty_nodes,
                    self.moved_nodes,
                    self.structural_parents,
                    self.partial_dirty_nodes,
                )
            }),
            render_only_dirty: self.render_only_dirty,
            recomposed_this_frame: frame.recomposed_this_frame,
            path,
        });
    }
}

struct SurfaceFrame {
    result: FrameUpdateResult,
    rebuilt: bool,
}

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

pub(crate) const IDLE_PREFETCH_MAX_PASSES: usize = 4;

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Turns semantics tracking on or off. Turning it on forces a layout pass and marks every
    /// surface dirty. Turning it off drops the cached semantics trees.
    pub fn set_semantics_enabled(&mut self, enabled: bool) {
        if self.app.semantics_enabled == enabled {
            return;
        }
        self.app.semantics_enabled = enabled;
        self.app.semantics_snapshot_revision = self.app.semantics_snapshot_revision.wrapping_add(1);
        if enabled {
            self.app.request_forced_layout_pass();
            self.mark_all_dirty();
        } else {
            for surface in &mut self.surfaces {
                surface.semantics_tree = None;
            }
        }
    }

    /// Sets how the platform should vote the display's frame rate for this
    /// app. The default, [`FrameRatePreference::Auto`], mirrors Compose: the
    /// panel's fastest rate while frames are being produced, no preference
    /// when the scene is still.
    pub fn set_frame_rate_preference(&mut self, preference: FrameRatePreference) {
        self.surfaces[0].frame_rate_preference = preference;
    }

    /// The app's current display frame-rate preference. Read by platform
    /// backends each frame; the vote itself is applied by the backend that
    /// owns the native window.
    pub fn frame_rate_preference(&self) -> FrameRatePreference {
        self.surfaces[0].frame_rate_preference
    }

    pub(crate) fn process_frame(&mut self) -> FrameUpdateResult {
        let _app_context = self.app.app_context.enter_scope();
        self.process_frame_in_context(false)
    }

    pub(crate) fn process_frame_in_context(
        &mut self,
        recomposed_before_frame: bool,
    ) -> FrameUpdateResult {
        let mut stages = FrameStages::start(frame_stage_telemetry_threshold_ms().is_some());

        self.app.app_context.start_text_frame();
        #[cfg(feature = "installed-fonts")]
        self.apply_installed_fonts();
        self.sync_window_roots_in_context();
        self.run_layout_phase_in_context();
        stages.reached();

        let recomposed_this_frame = recomposed_before_frame || self.run_post_layout_recomposition();

        stages.reached();

        self.run_dispatch_queues();
        self.update_modal_focus();
        self.reveal_new_focus();

        stages.reached();

        self.app
            .app_context
            .clear_transient_scroll_motion_contexts();

        let result = self.run_render_phase_in_context(recomposed_this_frame);
        stages.reached();
        log_frame_stage_telemetry(&stages);
        result
    }

    /// Whether a lazy list waits for [`Self::run_idle_prefetch`].
    pub fn has_idle_prefetch_work(&self) -> bool {
        self.app
            .app_context
            .enter(cranpose_ui::has_lazy_prefetch_requests)
    }

    /// Composes lazy list items that frames left beyond their viewports for
    /// later, in layout passes, at most four of them. A pass runs while
    /// `deadline`, if any, still leaves room for the recent cost of one full pass
    /// and `frame_can_start`, asked before each pass, says the next frame
    /// cannot start yet.
    /// Call it while waiting for the next frame. Returns whether a pass ran.
    pub fn run_idle_prefetch(
        &mut self,
        deadline: Option<Instant>,
        mut frame_can_start: impl FnMut(&mut R) -> bool,
    ) -> bool {
        let _app_context = self.app.app_context.enter_scope();
        let mut passes = 0;
        while passes < IDLE_PREFETCH_MAX_PASSES
            && cranpose_ui::has_lazy_prefetch_requests()
            && deadline.is_none_or(|deadline| {
                Instant::now() + cranpose_ui::lazy_prefetch_pass_cost() <= deadline
            })
            && !frame_can_start(self.renderer())
        {
            cranpose_ui::with_lazy_prefetch_pass(|| {
                cranpose_ui::drain_lazy_prefetch_requests(cranpose_ui::schedule_measure_repass);
                self.run_layout_phase_in_context();
                cranpose_ui::warm_prefetched_slices(&mut self.app.composition.applier_mut());
            });
            passes += 1;
        }
        passes > 0
    }

    /// Gives every renderer the fonts installed since the last frame, before
    /// this frame lays text out.
    #[cfg(feature = "installed-fonts")]
    fn apply_installed_fonts(&mut self) {
        let generation = cranpose_render_common::installed_fonts::installed_font_generation();
        if generation == self.app.installed_fonts {
            return;
        }
        self.app.installed_fonts = generation;
        for surface in &mut self.surfaces {
            surface
                .renderer
                .apply_installed_fonts(&self.app.app_context);
        }
    }

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

    fn run_dispatch_queues(&mut self) {
        if self.app.app_context.has_pending_pointer_repasses() {
            self.clear_dispatch_flags(DispatchInvalidationKind::Pointer);
        }
        if self.app.app_context.has_pending_focus_invalidations() {
            self.clear_dispatch_flags(DispatchInvalidationKind::Focus);
        }
        self.app.flush_semantics_invalidations();
    }

    /// Clears the `kind` flag of the nodes its queue holds. Out of line: a
    /// frame checks the queues and seldom finds work in them, so the frame's
    /// own code stays small.
    #[inline(never)]
    fn clear_dispatch_flags(&mut self, kind: DispatchInvalidationKind) {
        let mut applier = self.app.composition.applier_mut();
        let clear = |node_id| match clear_dispatch_invalidation(&mut applier, node_id, kind) {
            Ok(true) => log::trace!("Cleared the {kind:?} flag of node #{node_id}"),
            Ok(false) => {}
            Err(err) => {
                log::debug!("Could not clear the {kind:?} flag of node #{node_id}: {err}");
            }
        };
        match kind {
            DispatchInvalidationKind::Pointer => process_pointer_repasses(clear),
            DispatchInvalidationKind::Focus => process_focus_invalidations(clear),
        }
    }

    fn take_structural_change_parents(&mut self) {
        self.pending_dirty_nodes.clear();
        if cranpose_core::env_flag!("CRANPOSE_DISABLE_STRUCTURAL_DIRT") {
            return;
        }
        if self.app.composition.root().is_none() {
            return;
        }
        self.app
            .composition
            .applier_mut()
            .take_structural_change_parents_into(&mut self.pending_dirty_nodes);
        // A child's z index orders it among its siblings, in its parent's
        // layer: the parent changed as a child list does.
        let reordered = self.pending_dirty_nodes.len();
        self.app
            .app_context
            .append_z_order_changes(&mut self.pending_dirty_nodes);
        if self.pending_dirty_nodes.len() > reordered {
            self.reordered_children_to_parents(reordered);
        }
    }

    /// Replaces each child from `from` on in the pending dirty nodes, whose z
    /// index changed, with its parent. Out of line, as z indexes seldom
    /// change.
    #[inline(never)]
    fn reordered_children_to_parents(&mut self, from: usize) {
        let mut applier = self.app.composition.applier_mut();
        for node in &mut self.pending_dirty_nodes[from..] {
            if let Some(parent) = applier.get_mut(*node).ok().and_then(|child| child.parent()) {
                *node = parent;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn run_render_phase(&mut self) -> FrameUpdateResult {
        let _app_context = self.app.app_context.enter_scope();
        self.run_render_phase_in_context(false)
    }

    fn run_render_phase_in_context(&mut self, recomposed_this_frame: bool) -> FrameUpdateResult {
        let inspector_revision = if self.app.inspector_projector.is_some()
            && self
                .surfaces
                .iter()
                .any(|surface| surface.inspector.state.open)
        {
            self.semantics_snapshot_revision()
        } else {
            0
        };
        let context = &self.app.app_context;
        context.tick_cursor_blink();
        let render_dirty = context.take_render_invalidation();
        let pointer_dirty = context.take_pointer_invalidation();
        context.take_focus_invalidation();
        for surface in &mut self.surfaces {
            surface.scoped_draw_nodes.clear();
            surface.scoped_layer_property_nodes.clear();
            surface.structural_scene_nodes.clear();
        }
        self.app
            .app_context
            .take_draw_repass_nodes_into(&mut self.pending_dirty_nodes);
        route_nodes_by_surface(
            &mut self.app,
            &mut self.surfaces,
            self.pending_dirty_nodes.iter().copied(),
            SurfaceDirtyLane::Draw,
            &mut self.routing_scratch,
        );
        self.app
            .app_context
            .take_layer_property_repass_nodes_into(&mut self.pending_layer_property_nodes);
        route_nodes_by_surface(
            &mut self.app,
            &mut self.surfaces,
            self.pending_layer_property_nodes.iter().copied(),
            SurfaceDirtyLane::LayerProperties,
            &mut self.routing_scratch,
        );
        self.take_structural_change_parents();
        route_nodes_by_surface(
            &mut self.app,
            &mut self.surfaces,
            self.pending_dirty_nodes.iter().copied(),
            SurfaceDirtyLane::Structural,
            &mut self.routing_scratch,
        );
        self.pending_dirty_nodes.clear();
        self.pending_layer_property_nodes.clear();
        let _ = self.app.app_context.has_focused_field();
        let attributed = self.surfaces.iter().any(|surface| {
            !surface.scoped_draw_nodes.is_empty()
                || !surface.scoped_layer_property_nodes.is_empty()
                || !surface.structural_scene_nodes.is_empty()
        }) || self.surfaces.iter().any(|surface| {
            !surface.scoped_layout_scene_nodes.is_empty()
                || !surface.scoped_moved_scene_nodes.is_empty()
        });
        let layer_properties_pending = self
            .surfaces
            .iter()
            .any(|surface| !surface.scoped_layer_property_nodes.is_empty());
        let frame = FrameDirt {
            render_dirty,
            pointer_dirty,
            attributed,
            layer_properties_pending,
            recomposed_this_frame,
        };

        let mut result = FrameUpdateResult::default();
        for surface in &mut self.surfaces {
            let mut frame = render_surface(&mut self.app, surface, &frame);
            frame.result.visual_changed |=
                inspector::refresh(&mut self.app, surface, inspector_revision);
            surface.last_update = frame.result;
            surface.frame_owed |= frame.result.visual_changed;
            result.visual_changed |= frame.result.visual_changed;
            result.structure_changed |= frame.result.structure_changed;
            result.content_redrawn |= frame.rebuilt;
        }
        if result.content_redrawn {
            self.prune_stale_draw_observations();
        }
        result.content_moved = std::mem::take(&mut self.app.content_moved);
        result
    }

    /// Drops the draw observations of nodes no surface's render graph holds
    /// any more, once every [`OBSERVATION_PRUNE_INTERVAL`] rebuilt frames:
    /// finding them walks every node of every graph, and an observation
    /// left behind in between costs no more than a redraw request for a
    /// node nothing draws.
    fn prune_stale_draw_observations(&mut self) {
        self.app.rebuilt_frames_since_observation_prune += 1;
        if self.app.rebuilt_frames_since_observation_prune < OBSERVATION_PRUNE_INTERVAL {
            return;
        }
        self.app.rebuilt_frames_since_observation_prune = 0;
        let mut retained_visual_nodes = HashSet::default();
        let mut any_graph = false;
        for surface in &mut self.surfaces {
            any_graph |= surface
                .renderer
                .scene()
                .collect_retained_visual_observation_nodes(&mut surface.retained_visual_nodes);
            retained_visual_nodes.extend(surface.retained_visual_nodes.iter().copied());
        }
        if any_graph {
            cranpose_ui::prune_draw_observations_to_nodes(&retained_visual_nodes);
        }
    }
}

/// Rebuilt frames between prunes of stale draw observations: once a second
/// at 60 Hz.
pub(crate) const OBSERVATION_PRUNE_INTERVAL: u32 = 60;

fn render_surface<R>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    frame: &FrameDirt,
) -> SurfaceFrame
where
    R: Renderer,
    R::Error: Debug,
{
    let draw_dirty_nodes = std::mem::take(&mut surface.scoped_draw_nodes);
    let layer_property_nodes = std::mem::take(&mut surface.scoped_layer_property_nodes);
    let structural_parents = std::mem::take(&mut surface.structural_scene_nodes);
    let draw_repass_pending = !draw_dirty_nodes.is_empty();
    let mut draw_dirty_nodes = refresh_draw_nodes(app, surface, draw_dirty_nodes);
    if frame.render_dirty && !frame.layer_properties_pending && !draw_repass_pending {
        draw_dirty_nodes = refresh_retained_redraw_nodes(app, surface, draw_dirty_nodes);
    }
    let layout = ScopedLayoutNodes {
        reshaped: std::mem::take(&mut surface.scoped_layout_scene_nodes),
        moved: std::mem::take(&mut surface.scoped_moved_scene_nodes),
    };
    let partial_dirty_nodes = std::mem::take(&mut surface.partial_scene_nodes);
    let scene_dirty = surface.scene_dirty;
    let mut dirt = SurfaceDirt::classify(
        frame,
        scene_dirty,
        draw_repass_pending,
        ScopedVisualNodes {
            draw: draw_dirty_nodes,
            layers: layer_property_nodes,
        },
        layout,
        structural_parents,
        partial_dirty_nodes,
    );

    if !dirt.needs_scene_rebuild {
        dirt.log(
            frame,
            scene_dirty,
            if dirt.render_only_dirty {
                "present-retained"
            } else {
                "skip"
            },
        );
        surface.scoped_layout_scene_nodes = dirt.layout_dirty_nodes;
        surface.scoped_moved_scene_nodes = dirt.moved_nodes;
        surface.scoped_draw_nodes = dirt.draw_dirty_nodes;
        surface.scoped_layer_property_nodes = dirt.layer_property_nodes;
        surface.structural_scene_nodes = dirt.structural_parents;
        surface.partial_scene_nodes = dirt.partial_dirty_nodes;
        if dirt.render_only_dirty && app.dev_options.fps_counter {
            draw_dev_overlay(app, surface);
        }
        return SurfaceFrame {
            result: FrameUpdateResult {
                visual_changed: dirt.render_only_dirty,
                ..FrameUpdateResult::default()
            },
            rebuilt: false,
        };
    }

    surface.scene_dirty = false;
    let structure_changed = !dirt.render_only_dirty && !dirt.use_visual_update();
    rebuild_surface_scene(app, surface, frame, scene_dirty, &dirt);
    if dev_overlay_belongs_on(surface.id, app.dev_options.fps_counter) {
        draw_dev_overlay(app, surface);
    }
    dirt.layout_dirty_nodes.clear();
    dirt.moved_nodes.clear();
    surface.scoped_moved_scene_nodes = dirt.moved_nodes;
    surface.scoped_draw_nodes = dirt.draw_dirty_nodes;
    surface.scoped_layer_property_nodes = dirt.layer_property_nodes;
    surface.structural_scene_nodes = dirt.structural_parents;
    surface.partial_scene_nodes = dirt.partial_dirty_nodes;
    surface.scoped_layout_scene_nodes = dirt.layout_dirty_nodes;
    SurfaceFrame {
        result: FrameUpdateResult {
            visual_changed: true,
            structure_changed,
            ..FrameUpdateResult::default()
        },
        rebuilt: true,
    }
}

fn rebuild_surface_scene<R>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    frame: &FrameDirt,
    scene_dirty: bool,
    dirt: &SurfaceDirt,
) where
    R: Renderer,
    R::Error: Debug,
{
    let viewport_size = surface.viewport_size();
    let Some(root) = surface.root_node(app) else {
        surface.renderer.scene_mut().clear();
        return;
    };
    let mut applier = app.composition.applier_mut();
    dirt.log(frame, scene_dirty, dirt.rebuild_path());
    let rebuild_result = if dirt.use_visual_update() {
        // Nothing was composed or laid out: the partial nodes are the draw
        // lane's, whose draws read state that changed.
        surface.renderer.update_visual_scene_from_applier(
            &mut applier,
            root,
            viewport_size,
            SceneUpdates {
                content: &[],
                draws: &dirt.draw_dirty_nodes,
                layers: &dirt.layer_property_nodes,
                moved: &dirt.moved_nodes,
            },
        )
    } else if dirt.use_partial_update() {
        surface.renderer.update_scene_from_applier(
            &mut applier,
            root,
            viewport_size,
            SceneUpdates {
                content: &dirt.partial_dirty_nodes,
                draws: &[],
                layers: &dirt.layer_property_nodes,
                moved: &dirt.moved_nodes,
            },
        )
    } else {
        surface
            .renderer
            .rebuild_scene_from_applier(&mut applier, root, viewport_size)
    };
    if let Err(err) = rebuild_result {
        log::error!("renderer rebuild failed: {err:?}");
        surface.renderer.scene_mut().clear();
    }
}

fn dev_overlay_belongs_on(id: RootId, fps_counter: bool) -> bool {
    fps_counter && matches!(id, RootId::Primary)
}

fn refresh_draw_nodes<R: Renderer>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    mut dirty_nodes: Vec<NodeId>,
) -> Vec<NodeId> {
    if dirty_nodes.is_empty() {
        return dirty_nodes;
    }
    let Some(layout_tree) = surface.layout_tree.as_mut() else {
        return dirty_nodes;
    };

    dirty_nodes.sort_unstable();
    dirty_nodes.dedup();
    let dirty_set: HashSet<NodeId> = dirty_nodes.iter().copied().collect();
    let mut applier = app.composition.applier_mut();
    let refresh_scope = build_draw_refresh_scope(&mut applier, &dirty_set);
    refresh_layout_box_data(
        &mut applier,
        layout_tree.root_mut(),
        &refresh_scope,
        &dirty_set,
    );
    dirty_nodes
}

fn refresh_retained_redraw_nodes<R: Renderer>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    mut dirty_nodes: Vec<NodeId>,
) -> Vec<NodeId> {
    dirty_nodes.clear();
    let Some(root) = surface.root_node(app) else {
        return dirty_nodes;
    };
    {
        let mut applier = app.composition.applier_mut();
        collect_retained_redraw_nodes(
            &mut applier,
            &app.app_context,
            root,
            &mut dirty_nodes,
            &mut surface.redraw_walk,
        );
    }
    refresh_draw_nodes(app, surface, dirty_nodes)
}

fn clear_dispatch_invalidation(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    invalidation: DispatchInvalidationKind,
) -> Result<bool, NodeError> {
    match invalidation {
        DispatchInvalidationKind::Pointer => {
            match applier.with_node::<LayoutNode, _>(node_id, |node| {
                let needs_pointer_pass = node.needs_pointer_pass();
                if needs_pointer_pass {
                    node.clear_needs_pointer_pass();
                }
                needs_pointer_pass
            }) {
                Ok(cleared) => Ok(cleared),
                Err(NodeError::TypeMismatch { .. }) => applier
                    .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                        let needs_pointer_pass = node.needs_pointer_pass();
                        if needs_pointer_pass {
                            node.clear_needs_pointer_pass();
                        }
                        needs_pointer_pass
                    }),
                Err(err) => Err(err),
            }
        }
        DispatchInvalidationKind::Focus => {
            match applier.with_node::<LayoutNode, _>(node_id, |node| {
                let needs_focus_sync = node.needs_focus_sync();
                if needs_focus_sync {
                    node.clear_needs_focus_sync();
                }
                needs_focus_sync
            }) {
                Ok(cleared) => Ok(cleared),
                Err(NodeError::TypeMismatch { .. }) => applier
                    .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                        let needs_focus_sync = node.needs_focus_sync();
                        if needs_focus_sync {
                            node.clear_needs_focus_sync();
                        }
                        needs_focus_sync
                    }),
                Err(err) => Err(err),
            }
        }
    }
}

pub(crate) fn build_draw_refresh_scope(
    applier: &mut MemoryApplier,
    dirty_nodes: &HashSet<NodeId>,
) -> HashSet<NodeId> {
    let mut refresh_scope =
        HashSet::with_capacity_and_hasher(dirty_nodes.len(), Default::default());
    for &dirty_node in dirty_nodes {
        let mut current = Some(dirty_node);
        while let Some(node_id) = current {
            if !refresh_scope.insert(node_id) {
                break;
            }
            current = applier.get_mut(node_id).ok().and_then(|node| node.parent());
        }
    }
    refresh_scope
}

/// Collects into `dirty_nodes` the nodes under `root` whose redraw flag was
/// set before they had an id, clearing the flags: a draw repass names every
/// other node whose content changed. A node under another window root, or
/// not attached yet, stays noted in `context` for a later frame. `pending`
/// keeps its storage for the next frame.
fn collect_retained_redraw_nodes(
    applier: &mut MemoryApplier,
    context: &cranpose_ui::AppContext,
    root: NodeId,
    dirty_nodes: &mut Vec<NodeId>,
    pending: &mut Vec<NodeId>,
) {
    pending.clear();
    context.take_retained_redraw_nodes_into(pending);
    for &id in pending.iter() {
        match redraw_place(applier, id, root) {
            RedrawPlace::Here => {
                if take_redraw_flag(applier, id) {
                    dirty_nodes.push(id);
                }
            }
            RedrawPlace::Elsewhere => context.keep_retained_redraw_node(id),
            RedrawPlace::Gone => {}
        }
    }
}

/// Where a node noted for a redraw lies for the surface of `root`.
enum RedrawPlace {
    /// Under `root` and under no other window root: the surface takes it.
    Here,
    /// Under another window root, or not attached to a root yet.
    Elsewhere,
    /// The node is gone.
    Gone,
}

fn redraw_place(applier: &mut MemoryApplier, id: NodeId, root: NodeId) -> RedrawPlace {
    let mut current = id;
    loop {
        if current == root {
            return RedrawPlace::Here;
        }
        let Ok(node) = applier.get_mut(current) else {
            return if current == id {
                RedrawPlace::Gone
            } else {
                RedrawPlace::Elsewhere
            };
        };
        let parent = node.parent();
        if node
            .as_any_mut()
            .downcast_mut::<LayoutNode>()
            .is_some_and(|layout| layout.is_window_root())
        {
            return RedrawPlace::Elsewhere;
        }
        let Some(parent) = parent else {
            return RedrawPlace::Elsewhere;
        };
        current = parent;
    }
}

/// Clears the redraw flag of the layout or subcompose node `id`, and returns
/// whether it was set.
fn take_redraw_flag(applier: &mut MemoryApplier, id: NodeId) -> bool {
    let Ok(node) = applier.get_mut(id) else {
        return false;
    };
    let any = node.as_any_mut();
    if let Some(layout) = any.downcast_mut::<LayoutNode>() {
        let redraw = layout.needs_redraw();
        layout.clear_needs_redraw();
        return redraw;
    }
    match any.downcast_mut::<SubcomposeLayoutNode>() {
        Some(subcompose) => {
            let redraw = subcompose.needs_redraw();
            subcompose.clear_needs_redraw();
            redraw
        }
        None => false,
    }
}

fn refresh_layout_box_data(
    applier: &mut MemoryApplier,
    layout: &mut LayoutBox,
    refresh_scope: &HashSet<NodeId>,
    dirty_nodes: &HashSet<NodeId>,
) {
    if !refresh_scope.contains(&layout.node_id) {
        return;
    }

    if dirty_nodes.contains(&layout.node_id) {
        let refreshed = applier
            .with_node::<LayoutNode, _>(layout.node_id, |node| {
                node.clear_needs_redraw();
                (
                    node.modifier.clone(),
                    node.resolved_modifiers(),
                    node.modifier_slices_snapshot(),
                    node.semantics_configuration(),
                )
            })
            .or_else(|_| {
                applier.with_node::<SubcomposeLayoutNode, _>(layout.node_id, |node| {
                    node.clear_needs_redraw();
                    (
                        node.modifier(),
                        node.resolved_modifiers(),
                        node.modifier_slices_snapshot(),
                        node.semantics_configuration(),
                    )
                })
            });
        if let Ok((modifier, resolved_modifiers, slices, semantics)) = refreshed {
            layout.node_data.modifier = modifier;
            layout.node_data.resolved_modifiers = resolved_modifiers;
            layout.node_data.modifier_slices = slices;
            layout.node_data.semantics = semantics.map(Rc::new);
        }
    }

    for child in &mut layout.children {
        refresh_layout_box_data(applier, child, refresh_scope, dirty_nodes);
    }
}

#[cfg(test)]
#[path = "tests/shell_frame_tests.rs"]
mod tests;
