#![allow(clippy::type_complexity)]

mod fps_monitor;
mod hit_path_tracker;
mod shell_debug;
mod shell_frame;
mod shell_input;
#[cfg(test)]
use shell_frame::build_draw_refresh_scope;

// Re-export FPS monitoring API
pub use fps_monitor::{
    current_fps, fps_display, fps_display_detailed, fps_stats, record_recomposition, FpsStats,
};

use std::fmt::{Debug, Write};
// Use web_time for cross-platform time support (native + WASM) - compatible with winit
use web_time::Instant;

use cranpose_core::{
    enter_event_handler, exit_event_handler, location_key, run_in_mutable_snapshot, Applier,
    Composition, Key, MemoryApplier, NodeError, NodeId,
};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_runtime_std::StdRuntime;
use cranpose_ui::{
    format_layout_tree, format_render_scene, format_screen_summary,
    has_pending_focus_invalidations, has_pending_pointer_repasses, peek_focus_invalidation,
    peek_layout_invalidation, peek_pointer_invalidation, peek_render_invalidation,
    process_focus_invalidations, process_pointer_repasses, request_render_invalidation,
    take_draw_repass_nodes, take_focus_invalidation, take_layout_invalidation,
    take_pointer_invalidation, take_render_invalidation, HeadlessRenderer, LayoutBox, LayoutNode,
    LayoutTree, MeasureLayoutOptions, SemanticsTree, SubcomposeLayoutNode,
};
use cranpose_ui_graphics::{Point, Size};
use hit_path_tracker::{HitPathTracker, PointerId};
use std::collections::HashSet;

// Re-export key event types for use by cranpose
pub use cranpose_ui::{KeyCode, KeyEvent, KeyEventType, Modifiers};

#[cfg(any(test, feature = "test-support"))]
use cranpose_core::{
    debug_recompose_scope_registry_stats, slot_table::SlotTableDebugStats, MemoryApplierDebugStats,
    RecomposeScopeRegistryDebugStats,
};
#[cfg(any(test, feature = "test-support"))]
use cranpose_core::{
    runtime::{RuntimeDebugStats, StateArenaDebugStats},
    snapshot_pinning::{debug_snapshot_pinning_stats, SnapshotPinningDebugStats},
    snapshot_state_observer::SnapshotStateObserverDebugStats,
    snapshot_v2::{debug_snapshot_v2_stats, SnapshotV2DebugStats},
    CompositionPassDebugStats, SlotId,
};

pub struct AppShell<R>
where
    R: Renderer,
{
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    content: Box<dyn FnMut()>,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    layout_tree: Option<LayoutTree>,
    semantics_tree: Option<SemanticsTree>,
    semantics_enabled: bool,
    layout_requested: bool,
    force_layout_pass: bool,
    scene_dirty: bool,
    is_dirty: bool,
    /// Tracks which mouse buttons are currently pressed
    buttons_pressed: PointerButtons,
    /// Tracks which nodes were hit on PointerDown (by stable NodeId).
    ///
    /// This follows Jetpack Compose's HitPathTracker pattern:
    /// - On Down: cache NodeIds, not geometry
    /// - On Move/Up/Cancel: resolve fresh HitTargets from current scene
    /// - Handler closures are preserved (same Rc), so internal state survives
    hit_path_tracker: HitPathTracker,
    /// Tracks which nodes the pointer is currently hovering over.
    /// Used to synthesize Enter/Exit events when the hover set changes.
    hovered_nodes: Vec<NodeId>,
    /// Persistent clipboard for desktop (Linux X11 requires clipboard to stay alive)
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    clipboard: Option<arboard::Clipboard>,
    /// Dev options for debugging and performance monitoring
    dev_options: DevOptions,
}

/// Development options for debugging and performance monitoring.
///
/// These are rendered directly by the renderer (not via composition)
/// to avoid affecting performance measurements.
#[derive(Clone, Debug, Default)]
pub struct DevOptions {
    /// Show FPS counter overlay
    pub fps_counter: bool,
    /// Show recomposition count
    pub recomposition_counter: bool,
    /// Show layout timing breakdown
    pub layout_timing: bool,
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct RuntimeLeakDebugStats {
    pub applier_stats: MemoryApplierDebugStats,
    pub live_node_heap_bytes: usize,
    pub recycled_node_heap_bytes: usize,
    pub slot_table_heap_bytes: usize,
    pub pass_stats: CompositionPassDebugStats,
    pub slot_stats: SlotTableDebugStats,
    pub observer_stats: SnapshotStateObserverDebugStats,
    pub runtime_stats: RuntimeDebugStats,
    pub state_arena_stats: StateArenaDebugStats,
    pub recompose_scope_stats: RecomposeScopeRegistryDebugStats,
    pub snapshot_v2_stats: SnapshotV2DebugStats,
    pub snapshot_pinning_stats: SnapshotPinningDebugStats,
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub fn new(mut renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        // Initialize FPS tracking
        fps_monitor::init_fps_tracker();

        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let mut build: Box<dyn FnMut()> = Box::new(content);
        if let Err(err) = composition.render_stable(root_key, &mut *build) {
            log::error!("initial render failed: {err}");
        }
        renderer.scene_mut().clear();
        let mut shell = Self {
            runtime,
            composition,
            content: build,
            renderer,
            cursor: (0.0, 0.0),
            viewport: (800.0, 600.0),
            buffer_size: (800, 600),
            start_time: Instant::now(),
            layout_tree: None,
            semantics_tree: None,
            semantics_enabled: false,
            layout_requested: true,
            force_layout_pass: true,
            scene_dirty: true,
            is_dirty: true,
            buttons_pressed: PointerButtons::NONE,
            hit_path_tracker: HitPathTracker::new(),
            hovered_nodes: Vec::new(),
            #[cfg(all(
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            clipboard: arboard::Clipboard::new().ok(),
            dev_options: DevOptions::default(),
        };
        shell.process_frame();
        shell
    }

    /// Set development options for debugging and performance monitoring.
    ///
    /// The FPS counter and other overlays are rendered directly by the renderer
    /// (not via composition) to avoid affecting performance measurements.
    pub fn set_dev_options(&mut self, options: DevOptions) {
        self.dev_options = options;
    }

    /// Get a reference to the current dev options.
    pub fn dev_options(&self) -> &DevOptions {
        &self.dev_options
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.request_forced_layout_pass();
        self.mark_dirty();
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    pub fn clear_frame_waker(&mut self) {
        self.runtime.clear_frame_waker();
    }

    pub fn should_render(&self) -> bool {
        if self.layout_requested
            || self.scene_dirty
            || peek_render_invalidation()
            || peek_pointer_invalidation()
            || peek_focus_invalidation()
            || peek_layout_invalidation()
        {
            return true;
        }
        self.composition.should_render()
    }

    /// Returns true if the shell needs to redraw (dirty flag, layout dirty, active animations).
    /// Note: Cursor blink is now timer-based and uses WaitUntil scheduling, not continuous redraw.
    pub fn needs_redraw(&self) -> bool {
        if self.is_dirty
            || self.layout_requested
            || self.scene_dirty
            || peek_render_invalidation()
            || peek_pointer_invalidation()
            || peek_focus_invalidation()
            || peek_layout_invalidation()
            || cranpose_ui::has_pending_layout_repasses()
            || cranpose_ui::has_pending_draw_repasses()
            || has_pending_pointer_repasses()
            || has_pending_focus_invalidations()
        {
            return true;
        }

        self.composition.should_render()
    }

    /// Marks the shell as dirty, indicating a redraw is needed.
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    fn request_layout_pass(&mut self) {
        self.layout_requested = true;
    }

    fn request_forced_layout_pass(&mut self) {
        self.layout_requested = true;
        self.force_layout_pass = true;
    }

    /// Returns true if there are active animations or pending recompositions.
    pub fn has_active_animations(&self) -> bool {
        self.composition.should_render()
    }

    /// Returns the next scheduled event time for cursor blink.
    /// Use this for `ControlFlow::WaitUntil` scheduling.
    pub fn next_event_time(&self) -> Option<web_time::Instant> {
        cranpose_ui::next_cursor_blink_time()
    }

    pub fn update(&mut self) {
        let runtime_handle = self.runtime.runtime_handle();
        runtime_handle.with_deferred_state_releases(|| {
            let now = Instant::now();
            let frame_time = now
                .checked_duration_since(self.start_time)
                .unwrap_or_default()
                .as_nanos() as u64;
            self.runtime.drain_frame_callbacks(frame_time);
            runtime_handle.drain_ui();
            let should_render = self.composition.should_render();
            if should_render {
                log::trace!(
                    target: "cranpose::input",
                    "update begin: should_render=true layout_requested={} scene_dirty={} is_dirty={}",
                    self.layout_requested,
                    self.scene_dirty,
                    self.is_dirty
                );
            }
            if should_render {
                let Some(root_key) = self.composition.root_key() else {
                    self.process_frame();
                    self.is_dirty = false;
                    return;
                };
                match self.composition.reconcile(root_key, &mut *self.content) {
                    Ok(changed) => {
                        log::trace!(
                            target: "cranpose::input",
                            "reconcile changed={changed}"
                        );
                        if changed {
                            fps_monitor::record_recomposition();
                            self.request_layout_pass();
                            request_render_invalidation();
                        }
                    }
                    Err(NodeError::Missing { id }) => {
                        // Node was removed (likely due to conditional render or tab switch)
                        // This is expected when scopes try to recompose after their nodes are gone
                        log::debug!("Recomposition skipped: node {} no longer exists", id);
                        self.request_layout_pass();
                        request_render_invalidation();
                    }
                    Err(err) => {
                        log::error!("recomposition failed: {err}");
                        self.request_layout_pass();
                        request_render_invalidation();
                    }
                }
            }
            self.process_frame();
            // Clear dirty flag after update (frame has been processed)
            self.is_dirty = false;
        });
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
