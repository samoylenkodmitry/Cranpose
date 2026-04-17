use super::*;

#[derive(Copy, Clone)]
enum DispatchInvalidationKind {
    Pointer,
    Focus,
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
        if enabled {
            self.request_forced_layout_pass();
            self.mark_dirty();
        } else {
            self.semantics_tree = None;
        }
    }

    pub(crate) fn process_frame(&mut self) {
        // Record frame for FPS tracking
        fps_monitor::record_frame();

        #[cfg(debug_assertions)]
        let _frame_start = Instant::now();

        self.run_layout_phase();

        #[cfg(debug_assertions)]
        let _after_layout = Instant::now();

        self.run_dispatch_queues();

        #[cfg(debug_assertions)]
        let _after_dispatch = Instant::now();

        self.run_render_phase();
    }

    pub(crate) fn run_layout_phase(&mut self) {
        let has_scoped_repasses = cranpose_ui::has_pending_layout_repasses();

        // ═══════════════════════════════════════════════════════════════════════════════
        // GLOBAL LAYOUT INVALIDATION (rare fallback for true global events)
        // ═══════════════════════════════════════════════════════════════════════════════
        // This is the "nuclear option" - invalidates ALL layout caches across the entire app.
        //
        // WHEN THIS SHOULD FIRE:
        //   ✓ Window/viewport resize
        //   ✓ Global font scale or density changes
        //   ✓ Debug toggles that affect layout globally
        //
        // WHEN THIS SHOULD *NOT* FIRE:
        //   ✗ Scroll (use schedule_layout_repass instead)
        //   ✗ Single widget updates (use schedule_layout_repass instead)
        //   ✗ Any local layout change (use schedule_layout_repass instead)
        //
        // If you see this firing frequently during normal interactions,
        // someone is abusing request_layout_invalidation() - investigate!
        let invalidation_requested = take_layout_invalidation();

        if invalidation_requested && !has_scoped_repasses {
            // Invalidate all caches (O(app size) - expensive!)
            // This is internal-only API, only accessible via the internal path
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

            let tree_needs_semantics_check = self.semantics_enabled
                && cranpose_ui::tree_needs_semantics(&mut *applier, root).unwrap_or_else(|err| {
                    log::warn!(
                        "Cannot check semantics dirty status for root #{}: {}",
                        root,
                        err
                    );
                    true
                });
            let needs_layout = self.force_layout_pass
                || has_scoped_repasses
                || tree_needs_layout_check
                || tree_needs_semantics_check;

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
                    collect_semantics: self.semantics_enabled,
                    build_layout_tree: true,
                },
            ) {
                Ok(measurements) => {
                    let semantics_tree = measurements.semantics_tree().cloned();
                    self.layout_tree = Some(measurements.into_layout_tree());
                    self.semantics_tree = semantics_tree;
                    self.scene_dirty = true;
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                    self.layout_tree = None;
                    self.semantics_tree = None;
                    self.scene_dirty = true;
                }
            }
            applier.clear_runtime_handle();
        } else {
            self.layout_tree = None;
            self.semantics_tree = None;
            self.scene_dirty = true;
            self.layout_requested = false;
            self.force_layout_pass = false;
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
    }

    fn refresh_draw_repasses(&mut self) {
        let dirty_nodes = take_draw_repass_nodes();
        if dirty_nodes.is_empty() {
            return;
        }

        let Some(layout_tree) = self.layout_tree.as_mut() else {
            return;
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
    }

    pub(crate) fn run_render_phase(&mut self) {
        let render_dirty = take_render_invalidation();
        let pointer_dirty = take_pointer_invalidation();
        take_focus_invalidation();
        let draw_repass_pending = cranpose_ui::has_pending_draw_repasses();
        // Tick cursor blink timer - only marks dirty when visibility state changes
        let cursor_blink_dirty = cranpose_ui::tick_cursor_blink();

        let render_only_dirty = render_dirty || cursor_blink_dirty;
        // Pointer invalidations can replace hit-test handler closures inside modifier nodes.
        // The scene caches those closures, so it must be rebuilt to avoid dispatching stale input.
        let needs_scene_rebuild =
            self.scene_dirty || draw_repass_pending || render_only_dirty || pointer_dirty;

        if !needs_scene_rebuild {
            return;
        }
        self.scene_dirty = false;
        self.refresh_draw_repasses();
        let viewport_size = Size {
            width: self.viewport.0,
            height: self.viewport.1,
        };

        // Use new direct traversal rendering
        if let Some(root) = self.composition.root() {
            let mut applier = self.composition.applier_mut();
            if let Err(err) =
                self.renderer
                    .rebuild_scene_from_applier(&mut applier, root, viewport_size)
            {
                // Fallback to clearing scene on error
                log::error!("renderer rebuild failed: {err:?}");
                self.renderer.scene_mut().clear();
            }
        } else {
            self.renderer.scene_mut().clear();
        }

        // Draw FPS overlay if enabled (directly by renderer, no composition)
        if self.dev_options.fps_counter {
            let stats = fps_monitor::fps_stats();
            let text = format!(
                "{:.0} FPS | {:.1}ms | {} recomp/s",
                stats.fps, stats.avg_ms, stats.recomps_per_second
            );
            self.renderer.draw_dev_overlay(&text, viewport_size);
        }
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
