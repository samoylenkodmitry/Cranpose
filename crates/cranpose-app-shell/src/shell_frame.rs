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

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub fn set_semantics_enabled(&mut self, enabled: bool) {
        if self.semantics_enabled == enabled {
            return;
        }
        self.semantics_enabled = enabled;
        self.semantics_snapshot_revision = self.semantics_snapshot_revision.wrapping_add(1);
        if enabled {
            self.request_forced_layout_pass();
            self.mark_dirty();
        } else {
            self.semantics_tree = None;
        }
    }

    /// Sets how the platform should vote the display's frame rate for this
    /// app. The default, [`FrameRatePreference::Auto`], mirrors Compose: the
    /// panel's fastest rate while frames are being produced, no preference
    /// when the scene is still.
    pub fn set_frame_rate_preference(&mut self, preference: crate::FrameRatePreference) {
        self.frame_rate_preference = preference;
    }

    /// The app's current display frame-rate preference. Read by platform
    /// backends each frame; the vote itself is applied by the backend that
    /// owns the native window.
    pub fn frame_rate_preference(&self) -> crate::FrameRatePreference {
        self.frame_rate_preference
    }

    pub(crate) fn process_frame(&mut self) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.process_frame_in_context(false))
    }

    pub(crate) fn process_frame_in_context(
        &mut self,
        recomposed_before_frame: bool,
    ) -> FrameUpdateResult {
        let frame_start = Instant::now();

        self.run_layout_phase();
        let after_initial_layout = Instant::now();

        let recomposed_this_frame = recomposed_before_frame || self.run_post_layout_recomposition();

        let after_layout = Instant::now();

        self.run_dispatch_queues();

        let after_dispatch = Instant::now();

        clear_transient_scroll_motion_contexts();

        let result = self.run_render_phase_with_recomposition_state(recomposed_this_frame);
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

    pub(crate) fn run_layout_phase(&mut self) {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.run_layout_phase_in_context());
    }

    fn run_layout_phase_in_context(&mut self) {
        // Both scoped layout repasses (re-placement) and scoped measure repasses
        // (re-sizing, e.g. a collapsing swipe-dismissed row) drive a layout pass.
        let has_scoped_repasses = cranpose_ui::has_pending_layout_repasses()
            || cranpose_ui::has_pending_measure_repasses();
        // Both kinds carry the node they were scheduled for, and the scene
        // phase scopes its graph update to exactly those nodes. Collecting only
        // the layout ids silently dropped the measure ones, which is the path a
        // scrolling `LazyColumn` takes: the scene phase then saw an empty dirty
        // set with the scene marked dirty, read that as "everything changed",
        // and rebuilt the graph from the composition root on every scrolled
        // frame.
        let mut scoped_layout_nodes = if cranpose_ui::has_pending_layout_repasses() {
            cranpose_ui::pending_layout_repass_nodes_snapshot()
        } else {
            Vec::new()
        };
        if cranpose_ui::has_pending_measure_repasses() {
            for node in cranpose_ui::pending_measure_repass_nodes_snapshot() {
                if !scoped_layout_nodes.contains(&node) {
                    scoped_layout_nodes.push(node);
                }
            }
        }

        // Global layout invalidation is reserved for app-wide inputs such as
        // viewport, density, font-scale, or debug layout changes. Normal node
        // updates should arrive through scoped repasses.
        let invalidation_requested = take_layout_invalidation();
        let global_layout_invalidation = invalidation_requested && !has_scoped_repasses;
        let force_layout_pass = self.force_layout_pass;

        if invalidation_requested && !has_scoped_repasses {
            cranpose_ui::layout::invalidate_all_layout_caches();

            // Mark root as needing layout AND measure so tree_needs_layout() returns true
            // and intrinsic sizes are recalculated (e.g., text field resizing on content change)
            if let Some(root) = self.composition.root() {
                let mut applier = self.composition.applier_mut();
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
            self.request_forced_layout_pass();
        } else if invalidation_requested || has_scoped_repasses {
            self.request_layout_pass();
        }

        if !self.layout_requested {
            return;
        }

        let viewport_size = Size {
            width: self.viewport.0,
            height: self.viewport.1,
        };
        if let Some(root) = self.composition.root() {
            let handle = self.composition.runtime_handle();
            let mut applier = self.composition.applier_mut();
            applier.set_runtime_handle(handle);

            let tree_needs_layout_check = cranpose_ui::tree_needs_layout(&mut *applier, root)
                .unwrap_or_else(|err| {
                    log::warn!(
                        "Cannot check layout dirty status for root #{}: {}",
                        root,
                        err
                    );
                    true // Assume dirty on error
                });

            let needs_layout =
                self.force_layout_pass || has_scoped_repasses || tree_needs_layout_check;

            if !needs_layout {
                log::trace!("Skipping layout: tree is clean");
                self.layout_requested = false;
                self.force_layout_pass = false;
                applier.clear_runtime_handle();
                return;
            }

            self.layout_requested = false;
            self.force_layout_pass = false;

            // Ensure slots exist and borrow mutably (handled inside measure_layout via MemoryApplier)
            match cranpose_ui::measure_layout_with_options(
                &mut applier,
                root,
                viewport_size,
                MeasureLayoutOptions {
                    collect_semantics: false,
                    build_layout_tree: false,
                },
            ) {
                Ok(_measurements) => {
                    self.layout_tree = None;
                    self.semantics_snapshot_revision =
                        self.semantics_snapshot_revision.wrapping_add(1);
                    if self.semantics_enabled {
                        self.semantics_tree = None;
                    }
                    // A frame runs this phase more than once — the initial pass
                    // and again after post-layout recomposition — and the scene
                    // phase consumes the ids once, at the end. Assigning here
                    // let a later pass with nothing pending wipe the ids an
                    // earlier one recorded, which is how a scrolling
                    // `LazyColumn` reached the scene phase with an empty dirty
                    // set and got a full rebuild. Accumulate instead; only an
                    // app-wide relayout, where scoping means nothing, clears.
                    if global_layout_invalidation || force_layout_pass {
                        self.scoped_layout_scene_nodes.clear();
                    } else if has_scoped_repasses {
                        for node in scoped_layout_nodes {
                            if !self.scoped_layout_scene_nodes.contains(&node) {
                                self.scoped_layout_scene_nodes.push(node);
                            }
                        }
                    }
                    self.scene_dirty = true;
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                    self.layout_tree = None;
                    self.semantics_tree = None;
                    self.semantics_snapshot_revision =
                        self.semantics_snapshot_revision.wrapping_add(1);
                    self.scoped_layout_scene_nodes.clear();
                    self.scene_dirty = true;
                }
            }
            applier.clear_runtime_handle();
        } else {
            self.layout_tree = None;
            self.semantics_tree = None;
            self.semantics_snapshot_revision = self.semantics_snapshot_revision.wrapping_add(1);
            self.scoped_layout_scene_nodes.clear();
            self.scene_dirty = true;
            self.layout_requested = false;
            self.force_layout_pass = false;
        }
    }

    fn run_post_layout_recomposition(&mut self) -> bool {
        if !self.composition.should_recompose() {
            return false;
        }

        let Some(root_key) = self.composition.root_key() else {
            return false;
        };

        match self.composition.reconcile(root_key, &mut *self.content) {
            Ok(changed) => {
                if !changed {
                    return false;
                }
                self.fps_monitor.record_recomposition();
                if self.composition_tree_needs_layout() {
                    self.request_layout_pass();
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
                self.request_layout_pass();
                request_render_invalidation();
                true
            }
            Err(err) => {
                log::error!("post-layout recomposition failed: {err}");
                self.request_layout_pass();
                request_render_invalidation();
                true
            }
        }
    }

    fn run_dispatch_queues(&mut self) {
        // Process pointer input repasses
        // Similar to Jetpack Compose's pointer input invalidation processing,
        // we service nodes that need pointer input state updates without forcing layout/draw
        if has_pending_pointer_repasses() {
            let mut applier = self.composition.applier_mut();
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

        // Process focus invalidations
        // Mirrors Jetpack Compose's FocusInvalidationManager.invalidateNodes(),
        // processing nodes that need focus state synchronization
        if has_pending_focus_invalidations() {
            let mut applier = self.composition.applier_mut();
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

        // Process semantics invalidations raised from outside composition.
        //
        // Mirrors Compose's `SemanticsModifierNode.invalidateSemantics()`: an
        // app whose recorder reads state the composition does not observe says
        // so here, and the flag is bubbled to the root because
        // `tree_needs_semantics` reads the root alone. Without this a screen
        // that never relays out — a game drawn on one `Canvas` — publishes its
        // semantics once and then keeps publishing that same stale tree.
        if has_pending_semantics_invalidations() {
            let mut applier = self.composition.applier_mut();
            process_semantics_invalidations(|node_id| {
                cranpose_core::bubble_semantics_dirty(&mut *applier, node_id);
            });
        }
    }

    fn refresh_draw_repasses(&mut self) -> Vec<NodeId> {
        let dirty_nodes = take_draw_repass_nodes();
        if dirty_nodes.is_empty() {
            return dirty_nodes;
        }
        self.refresh_draw_nodes(dirty_nodes)
    }

    fn refresh_retained_redraw_nodes(&mut self) -> Vec<NodeId> {
        let Some(root) = self.composition.root() else {
            return Vec::new();
        };
        let mut dirty_nodes = Vec::new();
        {
            let mut applier = self.composition.applier_mut();
            collect_retained_redraw_nodes(&mut applier, root, &mut dirty_nodes);
        }
        if dirty_nodes.is_empty() {
            return dirty_nodes;
        }
        self.refresh_draw_nodes(dirty_nodes)
    }

    fn refresh_draw_nodes(&mut self, dirty_nodes: Vec<NodeId>) -> Vec<NodeId> {
        let Some(layout_tree) = self.layout_tree.as_mut() else {
            return dirty_nodes;
        };

        let dirty_set: HashSet<NodeId> = dirty_nodes.into_iter().collect();
        let mut applier = self.composition.applier_mut();
        let refresh_scope = build_draw_refresh_scope(&mut applier, &dirty_set);
        refresh_layout_box_data(
            &mut applier,
            layout_tree.root_mut(),
            &refresh_scope,
            &dirty_set,
        );
        dirty_set.into_iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn run_render_phase(&mut self) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.run_render_phase_in_context(false))
    }

    fn run_render_phase_with_recomposition_state(
        &mut self,
        recomposed_this_frame: bool,
    ) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.run_render_phase_in_context(recomposed_this_frame))
    }

    fn run_render_phase_in_context(&mut self, recomposed_this_frame: bool) -> FrameUpdateResult {
        let render_dirty = take_render_invalidation();
        let pointer_dirty = take_pointer_invalidation();
        take_focus_invalidation();
        let draw_repass_pending = cranpose_ui::has_pending_draw_repasses();
        let mut draw_dirty_nodes = self.refresh_draw_repasses();
        if render_dirty && !draw_repass_pending && draw_dirty_nodes.is_empty() {
            draw_dirty_nodes = self.refresh_retained_redraw_nodes();
        }
        let layout_dirty_nodes = std::mem::take(&mut self.scoped_layout_scene_nodes);
        // Parents whose child lists changed structurally this frame. A scoped
        // scene update patches only the dirty subtrees, so every structural
        // parent must be in scope or a removed subtree's layers stay in the
        // persistent render graph and keep compositing (the cross-tab ghost).
        let structural_parents = if cranpose_core::env_flag!("CRANPOSE_DISABLE_STRUCTURAL_DIRT") {
            Vec::new()
        } else if let Some(root) = self.composition.root() {
            self.composition
                .applier_mut()
                .take_structural_change_parents_attached_to(root)
        } else {
            Vec::new()
        };
        let structural_dirty = !structural_parents.is_empty();
        let mut partial_dirty_nodes = draw_dirty_nodes.clone();
        partial_dirty_nodes.extend(layout_dirty_nodes.iter().copied());
        partial_dirty_nodes.extend(structural_parents.iter().copied());
        partial_dirty_nodes.sort_unstable();
        partial_dirty_nodes.dedup();
        let draw_dirty_node_count = draw_dirty_nodes.len();
        let layout_dirty_node_count = layout_dirty_nodes.len();
        let structural_dirty_node_count = structural_parents.len();
        let partial_dirty_node_count = partial_dirty_nodes.len();
        // Detect a focused text field that left the composition this frame:
        // the check clears the stale focus entry and asks the platform to
        // hide its soft keyboard (no-op while focus is live or when no
        // keyboard was requested).
        let _ = cranpose_ui::has_focused_field();
        // Tick cursor blink timer - only marks dirty when visibility state changes
        let cursor_blink_dirty = cranpose_ui::tick_cursor_blink();

        let render_only_dirty = (render_dirty
            && partial_dirty_nodes.is_empty()
            && !draw_repass_pending
            && !structural_dirty)
            || cursor_blink_dirty;
        let scene_dirty = self.scene_dirty;
        let draw_only_partial_dirty = !draw_dirty_nodes.is_empty()
            && layout_dirty_nodes.is_empty()
            && !pointer_dirty
            && !recomposed_this_frame
            && !structural_dirty;
        let scoped_scene_dirty = scene_dirty && !layout_dirty_nodes.is_empty();
        let full_scene_dirty = scene_dirty && !scoped_scene_dirty && !draw_only_partial_dirty;
        let partial_scene_dirty = !partial_dirty_nodes.is_empty();
        let needs_scene_rebuild = full_scene_dirty
            || scoped_scene_dirty
            || partial_scene_dirty
            || draw_repass_pending
            || render_only_dirty
            || structural_dirty;

        if !needs_scene_rebuild {
            log_render_phase_dirty_diagnostics(RenderPhaseDirtyDiagnostics {
                render_dirty,
                pointer_dirty,
                scene_dirty,
                draw_repass_pending,
                draw_dirty_nodes: draw_dirty_node_count,
                layout_dirty_nodes: layout_dirty_node_count,
                structural_dirty_nodes: structural_dirty_node_count,
                partial_dirty_nodes: partial_dirty_node_count,
                dirty_node_ids: render_phase_dirty_diagnostics_enabled().then(|| {
                    format!(
                        "draw={:?} layout={:?} structural={:?} partial={:?}",
                        draw_dirty_nodes,
                        layout_dirty_nodes,
                        structural_parents,
                        partial_dirty_nodes
                    )
                }),
                render_only_dirty,
                recomposed_this_frame,
                path: "skip",
            });
            self.scoped_layout_scene_nodes = layout_dirty_nodes;
            return FrameUpdateResult::default();
        }
        self.scene_dirty = false;
        let viewport_size = Size {
            width: self.viewport.0,
            height: self.viewport.1,
        };
        let visual_update_only =
            draw_only_partial_dirty && !partial_dirty_nodes.is_empty() && !full_scene_dirty;
        let structure_changed = !render_only_dirty && !visual_update_only;

        // Use new direct traversal rendering
        if let Some(root) = self.composition.root() {
            let mut applier = self.composition.applier_mut();
            let use_partial_update =
                !partial_dirty_nodes.is_empty() && !render_only_dirty && !full_scene_dirty;
            let use_visual_update = use_partial_update && draw_only_partial_dirty;
            log_render_phase_dirty_diagnostics(RenderPhaseDirtyDiagnostics {
                render_dirty,
                pointer_dirty,
                scene_dirty,
                draw_repass_pending,
                draw_dirty_nodes: draw_dirty_node_count,
                layout_dirty_nodes: layout_dirty_node_count,
                structural_dirty_nodes: structural_dirty_node_count,
                partial_dirty_nodes: partial_dirty_node_count,
                dirty_node_ids: render_phase_dirty_diagnostics_enabled().then(|| {
                    format!(
                        "draw={:?} layout={:?} structural={:?} partial={:?}",
                        draw_dirty_nodes,
                        layout_dirty_nodes,
                        structural_parents,
                        partial_dirty_nodes
                    )
                }),
                render_only_dirty,
                recomposed_this_frame,
                path: if use_visual_update {
                    "visual-update"
                } else if use_partial_update {
                    "update"
                } else {
                    "rebuild"
                },
            });
            let rebuild_result = if use_visual_update {
                self.renderer.update_visual_scene_from_applier(
                    &mut applier,
                    root,
                    viewport_size,
                    &partial_dirty_nodes,
                )
            } else if use_partial_update {
                self.renderer.update_scene_from_applier(
                    &mut applier,
                    root,
                    viewport_size,
                    &partial_dirty_nodes,
                )
            } else {
                self.renderer
                    .rebuild_scene_from_applier(&mut applier, root, viewport_size)
            };
            if let Err(err) = rebuild_result {
                // Fallback to clearing scene on error
                log::error!("renderer rebuild failed: {err:?}");
                self.renderer.scene_mut().clear();
            }
        } else {
            self.renderer.scene_mut().clear();
        }
        if let Some(retained_nodes) = self.renderer.scene().retained_visual_observation_nodes() {
            cranpose_ui::prune_draw_observations_to_nodes(&retained_nodes);
        }

        // Draw FPS overlay if enabled (directly by renderer, no composition)
        if self.dev_options.fps_counter {
            self.refresh_dev_overlay_text_for_frame_at(viewport_size, Instant::now());
            let renderer = &mut self.renderer;
            let text = self.dev_overlay_text.as_str();
            renderer.draw_dev_overlay(text, viewport_size);
        }

        FrameUpdateResult {
            visual_changed: true,
            structure_changed,
        }
    }

    fn refresh_dev_overlay_text_for_frame_at(&mut self, viewport_size: Size, now: Instant) {
        if !self.dev_overlay_text_needs_refresh(viewport_size, now) {
            return;
        }
        self.dev_overlay_text = self.build_dev_overlay_text(viewport_size);
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

    fn build_dev_overlay_text(&mut self, viewport_size: Size) -> String {
        self.dev_overlay_controls.clear();

        let stats = self.fps_monitor.stats();
        let mut text = format!(
            "{:.0} FPS | avg {:.1}ms | p95 {:.1}ms | max {:.1}ms | work {:.1}ms | {} recomp/s",
            stats.fps,
            stats.avg_ms,
            stats.p95_ms,
            stats.max_ms,
            stats.work_avg_ms,
            stats.recomps_per_second
        );

        if !self.dev_options.frame_pacing_controls {
            return text;
        }

        text.push_str(" | ");
        let mut controls = Vec::with_capacity(FramePacingMode::ALL.len());
        for (index, mode) in FramePacingMode::ALL.into_iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            let start = text.len();
            if mode == self.dev_options.frame_pacing_mode {
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

fn collect_retained_redraw_nodes(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    dirty_nodes: &mut Vec<NodeId>,
) {
    let children = match applier.get_mut(node_id) {
        Ok(node) => node.children(),
        Err(_) => return,
    };

    let redraw = match applier.with_node::<LayoutNode, _>(node_id, |node| {
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
    };
    if redraw {
        dirty_nodes.push(node_id);
    }

    for child in children {
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
