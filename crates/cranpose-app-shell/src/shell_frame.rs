use super::*;

const DEV_OVERLAY_PADDING: f32 = 8.0;
const DEV_OVERLAY_FONT_SIZE: f32 = 14.0;
const DEV_OVERLAY_CHAR_WIDTH: f32 = 7.0;
const DEV_OVERLAY_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
const DEFAULT_FRAME_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

#[derive(Copy, Clone)]
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

fn log_frame_stage_telemetry(
    frame_start: Instant,
    after_initial_layout: Instant,
    after_layout: Instant,
    after_dispatch: Instant,
    after_render: Instant,
) {
    let Some(threshold_ms) = frame_stage_telemetry_threshold_ms() else {
        return;
    };

    let total_ms = after_render.duration_since(frame_start).as_secs_f64() * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    let layout_ms = after_layout.duration_since(frame_start).as_secs_f64() * 1000.0;
    let initial_layout_ms = after_initial_layout
        .duration_since(frame_start)
        .as_secs_f64()
        * 1000.0;
    let post_layout_ms = after_layout
        .duration_since(after_initial_layout)
        .as_secs_f64()
        * 1000.0;
    let dispatch_ms = after_dispatch.duration_since(after_layout).as_secs_f64() * 1000.0;
    let scene_ms = after_render.duration_since(after_dispatch).as_secs_f64() * 1000.0;
    log::warn!(
        "[frame-stage-telemetry] total_ms={total_ms:.2} layout_ms={layout_ms:.2} initial_layout_ms={initial_layout_ms:.2} post_layout_ms={post_layout_ms:.2} dispatch_ms={dispatch_ms:.2} scene_ms={scene_ms:.2}",
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
    layout_dirty_nodes: usize,
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
        layout_dirty_nodes,
        structural_dirty_nodes,
        partial_dirty_nodes,
        dirty_node_ids,
        render_only_dirty,
        recomposed_this_frame,
        path,
    } = diagnostics;
    if let Some(dirty_node_ids) = dirty_node_ids {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layout_dirty_nodes={layout_dirty_nodes} structural_dirty_nodes={structural_dirty_nodes} partial_dirty_nodes={partial_dirty_nodes} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame} ids={dirty_node_ids}",
        );
    } else {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layout_dirty_nodes={layout_dirty_nodes} structural_dirty_nodes={structural_dirty_nodes} partial_dirty_nodes={partial_dirty_nodes} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame}",
        );
    }
}

#[derive(Clone, Copy)]
struct FrameDirt {
    render_dirty: bool,
    pointer_dirty: bool,
    attributed: bool,
    recomposed_this_frame: bool,
}

struct SurfaceDirt {
    draw_dirty_nodes: Vec<NodeId>,
    layout_dirty_nodes: Vec<NodeId>,
    structural_parents: Vec<NodeId>,
    partial_dirty_nodes: Vec<NodeId>,
    draw_repass_pending: bool,
    render_only_dirty: bool,
    draw_only_partial_dirty: bool,
    full_scene_dirty: bool,
    needs_scene_rebuild: bool,
}

impl SurfaceDirt {
    fn classify(
        frame: &FrameDirt,
        scene_dirty: bool,
        draw_repass_pending: bool,
        draw_dirty_nodes: Vec<NodeId>,
        layout_dirty_nodes: Vec<NodeId>,
        structural_parents: Vec<NodeId>,
    ) -> Self {
        let structural_dirty = !structural_parents.is_empty();
        let mut partial_dirty_nodes = draw_dirty_nodes.clone();
        partial_dirty_nodes.extend(layout_dirty_nodes.iter().copied());
        partial_dirty_nodes.extend(structural_parents.iter().copied());
        partial_dirty_nodes.sort_unstable();
        partial_dirty_nodes.dedup();

        let render_only_dirty = frame.render_dirty
            && !frame.attributed
            && partial_dirty_nodes.is_empty()
            && !draw_repass_pending
            && !structural_dirty;
        let draw_only_partial_dirty = !draw_dirty_nodes.is_empty()
            && layout_dirty_nodes.is_empty()
            && !frame.pointer_dirty
            && !frame.recomposed_this_frame
            && !structural_dirty;
        let scoped_scene_dirty = scene_dirty && !layout_dirty_nodes.is_empty();
        let full_scene_dirty = scene_dirty && !scoped_scene_dirty && !draw_only_partial_dirty;
        let partial_scene_dirty = !partial_dirty_nodes.is_empty();
        let needs_scene_rebuild = full_scene_dirty
            || scoped_scene_dirty
            || partial_scene_dirty
            || draw_repass_pending
            || structural_dirty;
        Self {
            draw_dirty_nodes,
            layout_dirty_nodes,
            structural_parents,
            partial_dirty_nodes,
            draw_repass_pending,
            render_only_dirty,
            draw_only_partial_dirty,
            full_scene_dirty,
            needs_scene_rebuild,
        }
    }

    fn use_partial_update(&self) -> bool {
        !self.partial_dirty_nodes.is_empty() && !self.render_only_dirty && !self.full_scene_dirty
    }

    fn use_visual_update(&self) -> bool {
        self.use_partial_update() && self.draw_only_partial_dirty
    }

    fn visual_update_only(&self) -> bool {
        self.draw_only_partial_dirty
            && !self.partial_dirty_nodes.is_empty()
            && !self.full_scene_dirty
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
            layout_dirty_nodes: self.layout_dirty_nodes.len(),
            structural_dirty_nodes: self.structural_parents.len(),
            partial_dirty_nodes: self.partial_dirty_nodes.len(),
            dirty_node_ids: render_phase_dirty_diagnostics_enabled().then(|| {
                format!(
                    "draw={:?} layout={:?} structural={:?} partial={:?}",
                    self.draw_dirty_nodes,
                    self.layout_dirty_nodes,
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

fn pending_repass_nodes() -> Vec<NodeId> {
    let mut nodes = if cranpose_ui::has_pending_layout_repasses() {
        cranpose_ui::pending_layout_repass_nodes_snapshot()
    } else {
        Vec::new()
    };
    if cranpose_ui::has_pending_measure_repasses() {
        for node in cranpose_ui::pending_measure_repass_nodes_snapshot() {
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

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
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
    pub fn set_frame_rate_preference(&mut self, preference: crate::FrameRatePreference) {
        self.surfaces[0].frame_rate_preference = preference;
    }

    /// The app's current display frame-rate preference. Read by platform
    /// backends each frame; the vote itself is applied by the backend that
    /// owns the native window.
    pub fn frame_rate_preference(&self) -> crate::FrameRatePreference {
        self.surfaces[0].frame_rate_preference
    }

    pub(crate) fn process_frame(&mut self) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app.app_context);
        app_context.enter(|| self.process_frame_in_context(false))
    }

    pub(crate) fn process_frame_in_context(
        &mut self,
        recomposed_before_frame: bool,
    ) -> FrameUpdateResult {
        let frame_start = Instant::now();

        self.sync_window_roots_in_context();
        self.run_layout_phase_in_context();
        let after_initial_layout = Instant::now();

        let recomposed_this_frame = recomposed_before_frame || self.run_post_layout_recomposition();

        let after_layout = Instant::now();

        self.run_dispatch_queues();
        self.update_modal_focus();
        self.reveal_new_focus();

        let after_dispatch = Instant::now();

        clear_transient_scroll_motion_contexts();

        let result = self.run_render_phase_in_context(recomposed_this_frame);
        let after_render = Instant::now();
        log_frame_stage_telemetry(
            frame_start,
            after_initial_layout,
            after_layout,
            after_dispatch,
            after_render,
        );
        result
    }

    #[cfg(test)]
    pub(crate) fn run_layout_phase(&mut self) {
        let app_context = Rc::clone(&self.app.app_context);
        app_context.enter(|| self.run_layout_phase_in_context());
    }

    pub(crate) fn run_layout_phase_in_context(&mut self) {
        let has_scoped_repasses = cranpose_ui::has_pending_layout_repasses()
            || cranpose_ui::has_pending_measure_repasses();
        let scoped_layout_nodes = pending_repass_nodes();

        let global_layout_invalidation = take_layout_invalidation();
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
                log::warn!(
                    "Cannot check layout dirty status for root #{}: {}",
                    root,
                    err
                );
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
            surface.scoped_layout_scene_nodes.clear();
            surface.scene_dirty = true;
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
        if global {
            for surface in &mut self.surfaces {
                surface.scoped_layout_scene_nodes.clear();
                surface.scene_dirty = true;
            }
            let _ = cranpose_ui::take_geometry_scene_nodes();
            return;
        }
        let mut nodes = if has_scoped_repasses {
            scoped_layout_nodes
        } else {
            Vec::new()
        };
        if let Some(root) = self.app.composition.root() {
            let mut applier = self.app.composition.applier_mut();
            for node in cranpose_ui::take_geometry_scene_nodes() {
                if let Some(node) = applier.scene_node_attached_to(node, root) {
                    nodes.push(node);
                }
            }
        }
        let buckets = partition_nodes_by_surface(&mut self.app, &self.surfaces, nodes);
        let mut any_named = false;
        for (surface, bucket) in self.surfaces.iter_mut().zip(buckets) {
            if bucket.is_empty() {
                continue;
            }
            any_named = true;
            surface.scene_dirty = true;
            let mut seen: HashSet<NodeId> =
                surface.scoped_layout_scene_nodes.iter().copied().collect();
            for node in bucket {
                if seen.insert(node) {
                    surface.scoped_layout_scene_nodes.push(node);
                }
            }
        }
        if !any_named {
            for surface in &mut self.surfaces {
                surface.scene_dirty = true;
            }
        }
    }

    fn run_post_layout_recomposition(&mut self) -> bool {
        if !self.app.composition.should_recompose() {
            return false;
        }

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
                log::debug!(
                    "Post-layout recomposition skipped: node {} no longer exists",
                    id
                );
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
        if has_pending_pointer_repasses() {
            let mut applier = self.app.composition.applier_mut();
            process_pointer_repasses(|node_id| {
                match clear_dispatch_invalidation(
                    &mut applier,
                    node_id,
                    DispatchInvalidationKind::Pointer,
                ) {
                    Ok(true) => {
                        log::trace!("Cleared pointer repass flag for node #{}", node_id);
                    }
                    Ok(false) => {}
                    Err(err) => {
                        log::debug!(
                            "Could not process pointer repass for node #{}: {}",
                            node_id,
                            err
                        );
                    }
                }
            });
        }

        if has_pending_focus_invalidations() {
            let mut applier = self.app.composition.applier_mut();
            process_focus_invalidations(|node_id| {
                match clear_dispatch_invalidation(
                    &mut applier,
                    node_id,
                    DispatchInvalidationKind::Focus,
                ) {
                    Ok(true) => {
                        log::trace!("Cleared focus sync flag for node #{}", node_id);
                    }
                    Ok(false) => {}
                    Err(err) => {
                        log::debug!(
                            "Could not process focus invalidation for node #{}: {}",
                            node_id,
                            err
                        );
                    }
                }
            });
        }

        if has_pending_semantics_invalidations() {
            let mut applier = self.app.composition.applier_mut();
            process_semantics_invalidations(|node_id| {
                cranpose_core::bubble_semantics_dirty(&mut *applier, node_id);
            });
        }
    }

    fn take_structural_change_parents(&mut self) -> Vec<NodeId> {
        if cranpose_core::env_flag!("CRANPOSE_DISABLE_STRUCTURAL_DIRT") {
            return Vec::new();
        }
        let Some(root) = self.app.composition.root() else {
            return Vec::new();
        };
        self.app
            .composition
            .applier_mut()
            .take_structural_change_parents_attached_to(root)
    }

    #[cfg(test)]
    pub(crate) fn run_render_phase(&mut self) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app.app_context);
        app_context.enter(|| self.run_render_phase_in_context(false))
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
        cranpose_ui::tick_cursor_blink();
        let render_dirty = take_render_invalidation();
        let pointer_dirty = take_pointer_invalidation();
        take_focus_invalidation();
        let draw_dirty =
            partition_nodes_by_surface(&mut self.app, &self.surfaces, take_draw_repass_nodes());
        let structural = self.take_structural_change_parents();
        let structural = partition_nodes_by_surface(&mut self.app, &self.surfaces, structural);
        let _ = cranpose_ui::has_focused_field();
        let attributed = draw_dirty
            .iter()
            .chain(structural.iter())
            .any(|nodes| !nodes.is_empty())
            || self
                .surfaces
                .iter()
                .any(|surface| !surface.scoped_layout_scene_nodes.is_empty());
        let frame = FrameDirt {
            render_dirty,
            pointer_dirty,
            attributed,
            recomposed_this_frame,
        };

        let mut result = FrameUpdateResult::default();
        let mut retained_visual_nodes = HashSet::new();
        let mut prune_observations = false;
        for ((surface, draw_dirty), structural) in
            self.surfaces.iter_mut().zip(draw_dirty).zip(structural)
        {
            let mut frame = render_surface(&mut self.app, surface, &frame, draw_dirty, structural);
            frame.result.visual_changed |=
                crate::inspector::refresh(&mut self.app, surface, inspector_revision);
            surface.last_update = frame.result;
            surface.frame_owed |= frame.result.visual_changed;
            result.visual_changed |= frame.result.visual_changed;
            result.structure_changed |= frame.result.structure_changed;
            if frame.rebuilt
                && surface
                    .renderer
                    .scene()
                    .collect_retained_visual_observation_nodes(&mut surface.retained_visual_nodes)
            {
                prune_observations = true;
            }
            retained_visual_nodes.extend(surface.retained_visual_nodes.iter().copied());
        }
        if prune_observations {
            cranpose_ui::prune_draw_observations_to_nodes(&retained_visual_nodes);
        }
        result
    }
}

fn render_surface<R>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    frame: &FrameDirt,
    draw_dirty_nodes: Vec<NodeId>,
    structural_parents: Vec<NodeId>,
) -> SurfaceFrame
where
    R: Renderer,
    R::Error: Debug,
{
    let draw_repass_pending = !draw_dirty_nodes.is_empty();
    let mut draw_dirty_nodes = refresh_draw_nodes(app, surface, draw_dirty_nodes);
    if frame.render_dirty && !draw_repass_pending {
        draw_dirty_nodes = refresh_retained_redraw_nodes(app, surface);
    }
    let layout_dirty_nodes = std::mem::take(&mut surface.scoped_layout_scene_nodes);
    let scene_dirty = surface.scene_dirty;
    let dirt = SurfaceDirt::classify(
        frame,
        scene_dirty,
        draw_repass_pending,
        draw_dirty_nodes,
        layout_dirty_nodes,
        structural_parents,
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
        if dirt.render_only_dirty && app.dev_options.fps_counter {
            draw_dev_overlay(app, surface);
        }
        return SurfaceFrame {
            result: FrameUpdateResult {
                visual_changed: dirt.render_only_dirty,
                structure_changed: false,
            },
            rebuilt: false,
        };
    }

    surface.scene_dirty = false;
    let structure_changed = !dirt.render_only_dirty && !dirt.visual_update_only();
    rebuild_surface_scene(app, surface, frame, scene_dirty, &dirt);
    if dev_overlay_belongs_on(surface.id, app.dev_options.fps_counter) {
        draw_dev_overlay(app, surface);
    }
    SurfaceFrame {
        result: FrameUpdateResult {
            visual_changed: true,
            structure_changed,
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
        surface.renderer.update_visual_scene_from_applier(
            &mut applier,
            root,
            viewport_size,
            &dirt.partial_dirty_nodes,
        )
    } else if dirt.use_partial_update() {
        surface.renderer.update_scene_from_applier(
            &mut applier,
            root,
            viewport_size,
            &dirt.partial_dirty_nodes,
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

fn draw_dev_overlay<R: Renderer>(app: &ShellApp, surface: &mut RootSurface<R>) {
    let viewport_size = surface.viewport_size();
    surface.refresh_dev_overlay_text_for_frame_at(app, viewport_size, Instant::now());
    surface
        .renderer
        .draw_dev_overlay(surface.dev_overlay_text.as_str(), viewport_size);
}

fn refresh_draw_nodes<R: Renderer>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    dirty_nodes: Vec<NodeId>,
) -> Vec<NodeId> {
    if dirty_nodes.is_empty() {
        return dirty_nodes;
    }
    let Some(layout_tree) = surface.layout_tree.as_mut() else {
        return dirty_nodes;
    };

    let dirty_set: HashSet<NodeId> = dirty_nodes.into_iter().collect();
    let mut applier = app.composition.applier_mut();
    let refresh_scope = build_draw_refresh_scope(&mut applier, &dirty_set);
    refresh_layout_box_data(
        &mut applier,
        layout_tree.root_mut(),
        &refresh_scope,
        &dirty_set,
    );
    dirty_set.into_iter().collect()
}

fn refresh_retained_redraw_nodes<R: Renderer>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
) -> Vec<NodeId> {
    let Some(root) = surface.root_node(app) else {
        return Vec::new();
    };
    let mut dirty_nodes = Vec::new();
    {
        let mut applier = app.composition.applier_mut();
        collect_retained_redraw_nodes(&mut applier, root, &mut dirty_nodes);
    }
    refresh_draw_nodes(app, surface, dirty_nodes)
}

impl<R: Renderer> RootSurface<R> {
    fn refresh_dev_overlay_text_for_frame_at(
        &mut self,
        app: &ShellApp,
        viewport_size: Size,
        now: Instant,
    ) {
        if !self.dev_overlay_text_needs_refresh(viewport_size, now) {
            return;
        }
        self.dev_overlay_text = self.build_dev_overlay_text(app, viewport_size);
        self.dev_overlay_last_refresh = Some(now);
        self.dev_overlay_viewport = Some(viewport_size);
    }

    fn dev_overlay_text_needs_refresh(&self, viewport_size: Size, now: Instant) -> bool {
        if self.dev_overlay_text.is_empty() || self.dev_overlay_viewport != Some(viewport_size) {
            return true;
        }

        self.dev_overlay_last_refresh
            .map(|last| {
                now.checked_duration_since(last).unwrap_or_default() >= DEV_OVERLAY_REFRESH_INTERVAL
            })
            .unwrap_or(true)
    }

    fn build_dev_overlay_text(&mut self, app: &ShellApp, viewport_size: Size) -> String {
        self.dev_overlay_controls.clear();

        let stats = app.fps_monitor.stats();
        let mut text = format!(
            "{:.0} FPS | avg {:.1}ms | p95 {:.1}ms | max {:.1}ms | work {:.1}ms | {} recomp/s",
            stats.fps,
            stats.avg_ms,
            stats.p95_ms,
            stats.max_ms,
            stats.work_avg_ms,
            stats.recomps_per_second
        );

        if !app.dev_options.frame_pacing_controls {
            return text;
        }

        text.push_str(" | ");
        let mut controls = Vec::with_capacity(FramePacingMode::ALL.len());
        for (index, mode) in FramePacingMode::ALL.into_iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            let start = text.len();
            if mode == app.dev_options.frame_pacing_mode {
                text.push('[');
                text.push_str(mode.label());
                text.push(']');
            } else {
                text.push_str(mode.label());
            }
            controls.push((start, text.len(), mode));
        }

        let overlay_width = text.len() as f32 * DEV_OVERLAY_CHAR_WIDTH;
        let overlay_x = (viewport_size.width - overlay_width - DEV_OVERLAY_PADDING * 2.0)
            .max(DEV_OVERLAY_PADDING);
        let overlay_y = DEV_OVERLAY_PADDING;
        let text_x = overlay_x + DEV_OVERLAY_PADDING / 2.0;
        let text_y = overlay_y + DEV_OVERLAY_PADDING / 4.0;
        let text_height = DEV_OVERLAY_FONT_SIZE * 1.4;

        self.dev_overlay_controls = controls
            .into_iter()
            .map(|(start, end, mode)| DevOverlayControl {
                bounds: Rect {
                    x: text_x + start as f32 * DEV_OVERLAY_CHAR_WIDTH - 3.0,
                    y: text_y - 3.0,
                    width: (end - start) as f32 * DEV_OVERLAY_CHAR_WIDTH + 6.0,
                    height: text_height + 6.0,
                },
                mode,
            })
            .collect();

        text
    }
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
    let mut refresh_scope = HashSet::with_capacity(dirty_nodes.len());
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

fn take_needs_redraw(applier: &mut MemoryApplier, node_id: NodeId) -> bool {
    match applier.with_node::<LayoutNode, _>(node_id, |node| {
        let needs_redraw = node.needs_redraw();
        if needs_redraw {
            node.clear_needs_redraw();
        }
        needs_redraw
    }) {
        Ok(needs_redraw) => needs_redraw,
        Err(NodeError::TypeMismatch { .. }) => applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                let needs_redraw = node.needs_redraw();
                if needs_redraw {
                    node.clear_needs_redraw();
                }
                needs_redraw
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn collect_retained_redraw_nodes(
    applier: &mut MemoryApplier,
    root: NodeId,
    dirty_nodes: &mut Vec<NodeId>,
) {
    let mut children = Default::default();
    match applier.get_mut(root) {
        Ok(node) => node.collect_children_into(&mut children),
        Err(_) => return,
    }

    if take_needs_redraw(applier, root) {
        dirty_nodes.push(root);
    }

    for child in children {
        if cranpose_ui::is_window_root(applier, child) {
            continue;
        }
        collect_retained_redraw_nodes(applier, child, dirty_nodes);
    }
}

fn refresh_layout_box_data(
    applier: &mut MemoryApplier,
    layout: &mut cranpose_ui::layout::LayoutBox,
    refresh_scope: &HashSet<NodeId>,
    dirty_nodes: &HashSet<NodeId>,
) {
    if !refresh_scope.contains(&layout.node_id) {
        return;
    }

    if dirty_nodes.contains(&layout.node_id) {
        if let Ok((modifier, resolved_modifiers, slices)) =
            applier.with_node::<LayoutNode, _>(layout.node_id, |node| {
                node.clear_needs_redraw();
                (
                    node.modifier.clone(),
                    node.resolved_modifiers(),
                    node.modifier_slices_snapshot(),
                )
            })
        {
            layout.node_data.modifier = modifier;
            layout.node_data.resolved_modifiers = resolved_modifiers;
            layout.node_data.modifier_slices = slices;
        } else if let Ok((modifier, resolved_modifiers)) = applier
            .with_node::<SubcomposeLayoutNode, _>(layout.node_id, |node| {
                node.clear_needs_redraw();
                (node.modifier(), node.resolved_modifiers())
            })
        {
            layout.node_data.modifier = modifier.clone();
            layout.node_data.resolved_modifiers = resolved_modifiers;
            layout.node_data.modifier_slices =
                std::rc::Rc::new(cranpose_ui::collect_slices_from_modifier(&modifier));
        }
    }

    for child in &mut layout.children {
        refresh_layout_box_data(applier, child, refresh_scope, dirty_nodes);
    }
}

#[cfg(test)]
#[path = "tests/shell_frame_tests.rs"]
mod tests;
