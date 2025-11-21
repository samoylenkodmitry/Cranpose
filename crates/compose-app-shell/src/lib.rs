#![allow(clippy::type_complexity)]

use std::fmt::Debug;
use std::time::Instant;

use compose_core::{location_key, Composition, Key, MemoryApplier, NodeError};
use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_runtime_std::StdRuntime;
use compose_ui::{
    has_pending_focus_invalidations, has_pending_pointer_repasses, log_layout_tree,
    log_render_scene, log_screen_summary, peek_focus_invalidation, peek_pointer_invalidation,
    peek_render_invalidation, process_focus_invalidations, process_pointer_repasses,
    request_render_invalidation, take_focus_invalidation, take_pointer_invalidation,
    take_render_invalidation, HeadlessRenderer, LayoutEngine, LayoutNode, LayoutTree,
};
use compose_ui_graphics::Size;

pub struct AppShell<R>
where
    R: Renderer,
{
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    layout_tree: Option<LayoutTree>,
    layout_dirty: bool,
    scene_dirty: bool,
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub fn new(mut renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let build = content;
        if let Err(err) = composition.render(root_key, build) {
            log::error!("initial render failed: {err}");
        }
        renderer.scene_mut().clear();
        let mut shell = Self {
            runtime,
            composition,
            renderer,
            cursor: (0.0, 0.0),
            viewport: (800.0, 600.0),
            buffer_size: (800, 600),
            start_time: Instant::now(),
            layout_tree: None,
            layout_dirty: true,
            scene_dirty: true,
        };
        shell.process_frame();
        shell
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.layout_dirty = true;
        self.process_frame();
    }

    pub fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    pub fn scene(&self) -> &R::Scene {
        self.renderer.scene()
    }

    pub fn renderer(&mut self) -> &mut R {
        &mut self.renderer
    }

    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    pub fn clear_frame_waker(&mut self) {
        self.runtime.clear_frame_waker();
    }

    pub fn should_render(&self) -> bool {
        if self.layout_dirty
            || self.scene_dirty
            || peek_render_invalidation()
            || peek_pointer_invalidation()
            || peek_focus_invalidation()
        {
            return true;
        }
        self.runtime.take_frame_request() || self.composition.should_render()
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let frame_time = now
            .checked_duration_since(self.start_time)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.runtime.drain_frame_callbacks(frame_time);
        if self.composition.should_render() {
            match self.composition.process_invalid_scopes() {
                Ok(changed) => {
                    if changed {
                        self.layout_dirty = true;
                        // Request render invalidation so the scene gets rebuilt
                        request_render_invalidation();
                    }
                }
                Err(NodeError::Missing { id }) => {
                    // Node was removed (likely due to conditional render or tab switch)
                    // This is expected when scopes try to recompose after their nodes are gone
                    log::debug!("Recomposition skipped: node {} no longer exists", id);
                    self.layout_dirty = true;
                    request_render_invalidation();
                }
                Err(err) => {
                    log::error!("recomposition failed: {err}");
                    self.layout_dirty = true;
                    request_render_invalidation();
                }
            }
        }
        self.process_frame();
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.renderer.scene().hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    pub fn pointer_pressed(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    pub fn pointer_released(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    pub fn log_debug_info(&mut self) {
        println!("\n\n");
        println!("════════════════════════════════════════════════════════");
        println!("           DEBUG: CURRENT SCREEN STATE");
        println!("════════════════════════════════════════════════════════");

        if let Some(ref layout_tree) = self.layout_tree {
            log_layout_tree(layout_tree);
            let renderer = HeadlessRenderer::new();
            let render_scene = renderer.render(layout_tree);
            log_render_scene(&render_scene);
            log_screen_summary(layout_tree, &render_scene);
        } else {
            println!("No layout available");
        }

        println!("════════════════════════════════════════════════════════");
        println!("\n\n");
    }

    fn process_frame(&mut self) {
        self.run_layout_phase();
        self.run_dispatch_queues();
        self.run_render_phase();
    }

    fn run_layout_phase(&mut self) {
        // Early exit if layout is not needed (viewport didn't change, etc.)
        if !self.layout_dirty {
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

            // Selective measure optimization: skip layout if tree is clean (O(1) check)
            let needs_layout =
                compose_ui::tree_needs_layout(&mut *applier, root).unwrap_or_else(|err| {
                    log::warn!(
                        "Cannot check layout dirty status for root #{}: {}",
                        root,
                        err
                    );
                    true // Assume dirty on error
                });

            if !needs_layout {
                // Tree is clean - skip layout computation and keep cached layout
                log::trace!("Skipping layout: tree is clean");
                self.layout_dirty = false;
                applier.clear_runtime_handle();
                return;
            }

            // Tree needs layout - compute it
            self.layout_dirty = false;
            match applier.compute_layout(root, viewport_size) {
                Ok(layout_tree) => {
                    self.layout_tree = Some(layout_tree);
                    self.scene_dirty = true;
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                    self.layout_tree = None;
                    self.scene_dirty = true;
                }
            }
            applier.clear_runtime_handle();
        } else {
            self.layout_tree = None;
            self.scene_dirty = true;
            self.layout_dirty = false;
        }
    }

    fn run_dispatch_queues(&mut self) {
        // Process pointer input repasses
        // Similar to Jetpack Compose's pointer input invalidation processing,
        // we service nodes that need pointer input state updates without forcing layout/draw
        if has_pending_pointer_repasses() {
            let mut applier = self.composition.applier_mut();
            process_pointer_repasses(|node_id| {
                // Access the LayoutNode and clear its dirty flag
                let result = applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                    if layout_node.needs_pointer_pass() {
                        layout_node.clear_needs_pointer_pass();
                        log::trace!("Cleared pointer repass flag for node #{}", node_id);
                    }
                });
                if let Err(err) = result {
                    log::debug!(
                        "Could not process pointer repass for node #{}: {}",
                        node_id,
                        err
                    );
                }
            });
        }

        // Process focus invalidations
        // Mirrors Jetpack Compose's FocusInvalidationManager.invalidateNodes(),
        // processing nodes that need focus state synchronization
        if has_pending_focus_invalidations() {
            let mut applier = self.composition.applier_mut();
            process_focus_invalidations(|node_id| {
                // Access the LayoutNode and clear its dirty flag
                let result = applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                    if layout_node.needs_focus_sync() {
                        layout_node.clear_needs_focus_sync();
                        log::trace!("Cleared focus sync flag for node #{}", node_id);
                    }
                });
                if let Err(err) = result {
                    log::debug!(
                        "Could not process focus invalidation for node #{}: {}",
                        node_id,
                        err
                    );
                }
            });
        }
    }

    fn run_render_phase(&mut self) {
        let render_dirty = take_render_invalidation();
        let pointer_dirty = take_pointer_invalidation();
        let focus_dirty = take_focus_invalidation();
        if render_dirty || pointer_dirty || focus_dirty {
            self.scene_dirty = true;
        }
        if !self.scene_dirty {
            return;
        }
        self.scene_dirty = false;
        if let Some(layout_tree) = self.layout_tree.as_ref() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            if let Err(err) = self.renderer.rebuild_scene(layout_tree, viewport_size) {
                log::error!("renderer rebuild failed: {err:?}");
            }
        } else {
            self.renderer.scene_mut().clear();
        }
    }
}

impl<R> Drop for AppShell<R>
where
    R: Renderer,
{
    fn drop(&mut self) {
        self.runtime.clear_frame_waker();
    }
}

pub fn default_root_key() -> Key {
    location_key(file!(), line!(), column!())
}

#[cfg(test)]
#[path = "tests/app_shell_tests.rs"]
mod tests;
