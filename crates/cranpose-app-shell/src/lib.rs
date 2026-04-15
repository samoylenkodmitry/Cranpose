#![allow(clippy::type_complexity)]

mod fps_monitor;
mod hit_path_tracker;

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

#[derive(Copy, Clone)]
enum DispatchInvalidationKind {
    Pointer,
    Focus,
}

pub struct AppShell<R>
where
    R: Renderer,
{
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    root_key: Key,
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
    fn resolve_gesture_targets(
        &self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        self.resolve_hit_path(pointer)
    }

    fn drain_root_render_requests(&mut self) {
        for _ in 0..100 {
            if !self.composition.take_root_render_request() {
                return;
            }

            match self.composition.render(self.root_key, &mut *self.content) {
                Ok(()) => {
                    fps_monitor::record_recomposition();
                    self.request_layout_pass();
                    request_render_invalidation();
                }
                Err(err) => {
                    log::error!("root render fallback failed: {err}");
                    return;
                }
            }
        }

        log::error!("root render fallback looped too many times");
    }

    pub fn new(mut renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        // Initialize FPS tracking
        fps_monitor::init_fps_tracker();

        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let mut build: Box<dyn FnMut()> = Box::new(content);
        if let Err(err) = composition.render(root_key, &mut *build) {
            log::error!("initial render failed: {err}");
        }
        renderer.scene_mut().clear();
        let mut shell = Self {
            runtime,
            composition,
            root_key,
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
        shell.drain_root_render_requests();
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
        self.runtime.has_frame_request() || self.composition.should_render()
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
        self.runtime.has_frame_request() || self.composition.should_render()
    }

    /// Returns the next scheduled event time for cursor blink.
    /// Use this for `ControlFlow::WaitUntil` scheduling.
    pub fn next_event_time(&self) -> Option<web_time::Instant> {
        cranpose_ui::next_cursor_blink_time()
    }

    /// Resolves cached NodeIds to fresh HitTargets from the current scene.
    ///
    /// This is the key to avoiding stale geometry during scroll/layout changes:
    /// - We cache NodeIds on PointerDown (stable identity)
    /// - On Move/Up/Cancel, we call find_target() to get fresh geometry
    /// - Handler closures are preserved (same Rc), so gesture state survives
    fn resolve_hit_path(
        &self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        let Some(node_ids) = self.hit_path_tracker.dispatch_order(pointer) else {
            return Vec::new();
        };

        let scene = self.renderer.scene();
        let targets: Vec<_> = node_ids
            .iter()
            .filter_map(|&id| scene.find_target(id))
            .collect();
        log::trace!(
            target: "cranpose::input",
            "resolve_hit_path pointer={pointer:?} cached={node_ids:?} resolved_count={}",
            targets.len()
        );
        targets
    }

    fn dispatch_targets<I>(&mut self, targets: I, event: PointerEvent, stop_on_consume: bool)
    where
        I: IntoIterator<Item = <<R as Renderer>::Scene as RenderScene>::HitTarget>,
    {
        for target in targets {
            let node_id = target.node_id();
            target.dispatch(event.clone());
            log::trace!(
                target: "cranpose::input",
                "dispatch {:?} node={} consumed={} stop_on_consume={}",
                event.kind,
                node_id,
                event.is_consumed(),
                stop_on_consume,
            );
            if stop_on_consume && event.is_consumed() {
                break;
            }
        }
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
                match self.composition.process_invalid_scopes() {
                    Ok(changed) => {
                        log::trace!(
                            target: "cranpose::input",
                            "process_invalid_scopes changed={changed}"
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
            self.drain_root_render_requests();
            self.process_frame();
            // Clear dirty flag after update (frame has been processed)
            self.is_dirty = false;
        });
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) -> bool {
        enter_event_handler();
        let result = run_in_mutable_snapshot(|| self.set_cursor_inner(x, y)).unwrap_or(false);
        exit_event_handler();
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "set_cursor ({x:.2},{y:.2}) -> {result}"
        );
        result
    }

    fn set_cursor_inner(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);

        // During a gesture (button pressed), ONLY dispatch to the tracked hit path.
        // Never fall back to hover hit-testing while buttons are down.
        // This maintains the invariant: the path that receives Down must receive Move and Up/Cancel.
        if self.buttons_pressed != PointerButtons::NONE {
            if self.hit_path_tracker.has_path(PointerId::PRIMARY) {
                let targets = self.resolve_gesture_targets(PointerId::PRIMARY);
                if !targets.is_empty() {
                    let event =
                        PointerEvent::new(PointerEventKind::Move, Point { x, y }, Point { x, y })
                            .with_buttons(self.buttons_pressed);
                    self.dispatch_targets(targets, event, false);
                    return true;
                }

                return false;
            }

            // Button is down but we have no recorded path inside this app
            // (e.g. drag started outside). Do not dispatch anything.
            return false;
        }

        // No gesture in progress: regular hover move using hit-test.
        // Diff against previous hover set to synthesize Enter/Exit events.
        let hits = self.renderer.scene().hit_test(x, y);
        let new_ids: Vec<NodeId> = hits.iter().map(|h| h.node_id()).collect();

        // Dispatch Exit to nodes that are no longer hovered
        let pos = Point { x, y };
        let previously_hovered = self.hovered_nodes.clone();
        for old_id in previously_hovered {
            if !new_ids.contains(&old_id) {
                if let Some(target) = self.renderer.scene().find_target(old_id) {
                    let exit_event = PointerEvent::new(PointerEventKind::Exit, pos, pos)
                        .with_buttons(self.buttons_pressed);
                    self.dispatch_targets(std::iter::once(target), exit_event, false);
                }
            }
        }

        // Dispatch Enter to newly hovered nodes
        for hit in &hits {
            if !self.hovered_nodes.contains(&hit.node_id()) {
                let enter_event = PointerEvent::new(PointerEventKind::Enter, pos, pos)
                    .with_buttons(self.buttons_pressed);
                self.dispatch_targets(std::iter::once(hit.clone()), enter_event, false);
            }
        }

        self.hovered_nodes = new_ids;

        if !hits.is_empty() {
            let event = PointerEvent::new(PointerEventKind::Move, pos, pos)
                .with_buttons(self.buttons_pressed);
            self.dispatch_targets(hits, event, true);
            true
        } else {
            false
        }
    }

    pub fn pointer_pressed(&mut self) -> bool {
        enter_event_handler();
        let result = run_in_mutable_snapshot(|| self.pointer_pressed_inner()).unwrap_or(false);
        exit_event_handler();
        if result {
            self.mark_dirty();
        }
        log::trace!(target: "cranpose::input", "pointer_pressed -> {result}");
        result
    }

    fn pointer_pressed_inner(&mut self) -> bool {
        // Track button state
        self.buttons_pressed.insert(PointerButton::Primary);

        // Hit-test against the current (last rendered) scene.
        // Even if the app is dirty, this scene is what the user actually saw and clicked.
        // Frame N is rendered → user sees frame N and taps → we hit-test frame N's geometry.
        // The pointer event may mark dirty → next frame runs update() → renders N+1.

        // Perform hit test and cache the NodeIds (not geometry!)
        // The key insight from Jetpack Compose: cache identity, resolve fresh geometry per dispatch
        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            self.hit_path_tracker.remove_path(PointerId::PRIMARY);
            false
        } else {
            let event = PointerEvent::new(
                PointerEventKind::Down,
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
            )
            .with_buttons(self.buttons_pressed);

            let mut delivered_capture_paths = Vec::new();
            for hit in hits {
                let node_id = hit.node_id();
                delivered_capture_paths.push(hit.capture_path());
                hit.dispatch(event.clone());
                log::trace!(
                    target: "cranpose::input",
                    "dispatch {:?} node={} consumed={} stop_on_consume=true",
                    event.kind,
                    node_id,
                    event.is_consumed(),
                );
                if event.is_consumed() {
                    break;
                }
            }

            self.hit_path_tracker
                .add_hit_path(PointerId::PRIMARY, delivered_capture_paths);
            log::trace!(
                target: "cranpose::input",
                "pointer_pressed_inner cached_hit_path={:?}",
                self.hit_path_tracker.get_path(PointerId::PRIMARY),
            );

            true
        }
    }

    pub fn pointer_released(&mut self) -> bool {
        enter_event_handler();
        let result = run_in_mutable_snapshot(|| self.pointer_released_inner()).unwrap_or(false);
        exit_event_handler();
        if result {
            self.mark_dirty();
        }
        log::trace!(target: "cranpose::input", "pointer_released -> {result}");
        result
    }

    fn pointer_released_inner(&mut self) -> bool {
        // UP events report buttons as "currently pressed" (after release),
        // matching typical platform semantics where primary is already gone.
        self.buttons_pressed.remove(PointerButton::Primary);
        let corrected_buttons = self.buttons_pressed;
        let targets = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Always remove the path, even if targets is empty (node may have been removed)
        self.hit_path_tracker.remove_path(PointerId::PRIMARY);

        if !targets.is_empty() {
            let event = PointerEvent::new(
                PointerEventKind::Up,
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
            )
            .with_buttons(corrected_buttons);

            self.dispatch_targets(targets, event, false);
            true
        } else {
            false
        }
    }

    /// Dispatches a mouse wheel / trackpad scroll event to hovered pointer handlers.
    ///
    /// Returns `true` if a handler consumed the event.
    pub fn pointer_scrolled(&mut self, delta_x: f32, delta_y: f32) -> bool {
        enter_event_handler();
        let result = run_in_mutable_snapshot(|| self.pointer_scrolled_inner(delta_x, delta_y))
            .unwrap_or(false);
        exit_event_handler();
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_scrolled ({delta_x:.2},{delta_y:.2}) -> {result}"
        );
        result
    }

    fn pointer_scrolled_inner(&mut self, delta_x: f32, delta_y: f32) -> bool {
        if delta_x.abs() <= f32::EPSILON && delta_y.abs() <= f32::EPSILON {
            return false;
        }

        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            return false;
        }

        let event = PointerEvent::new(
            PointerEventKind::Scroll,
            Point {
                x: self.cursor.0,
                y: self.cursor.1,
            },
            Point {
                x: self.cursor.0,
                y: self.cursor.1,
            },
        )
        .with_buttons(self.buttons_pressed)
        .with_scroll_delta(Point {
            x: delta_x,
            y: delta_y,
        });

        self.dispatch_targets(hits, event.clone(), true);

        event.is_consumed()
    }

    /// Cancels any active gesture, dispatching Cancel events to cached targets.
    /// Call this when:
    /// - Window loses focus
    /// - Mouse leaves window while button pressed
    /// - Any other gesture abort scenario
    pub fn cancel_gesture(&mut self) {
        enter_event_handler();
        let _ = run_in_mutable_snapshot(|| {
            self.cancel_gesture_inner();
        });
        exit_event_handler();
    }

    fn cancel_gesture_inner(&mut self) {
        let targets = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Clear tracker and button state
        self.hit_path_tracker.clear();
        self.buttons_pressed = PointerButtons::NONE;

        if !targets.is_empty() {
            let event = PointerEvent::new(
                PointerEventKind::Cancel,
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
            );

            self.dispatch_targets(targets, event, false);
        }

        // Dispatch Exit to all previously hovered nodes
        let pos = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };
        let hovered_nodes = self.hovered_nodes.clone();
        for node_id in hovered_nodes {
            if let Some(target) = self.renderer.scene().find_target(node_id) {
                let exit_event = PointerEvent::new(PointerEventKind::Exit, pos, pos);
                self.dispatch_targets(std::iter::once(target), exit_event, false);
            }
        }
        self.hovered_nodes.clear();
    }
    /// Routes a keyboard event to the focused text field, if any.
    ///
    /// Returns `true` if the event was consumed by a text field.
    ///
    /// On desktop, Ctrl+C/X/V are handled here with system clipboard (arboard).
    /// On web, these keys are NOT handled here - they bubble to browser for native copy/paste events.
    pub fn on_key_event(&mut self, event: &KeyEvent) -> bool {
        enter_event_handler();
        let result = self.on_key_event_inner(event);
        exit_event_handler();
        result
    }

    /// Internal keyboard event handler wrapped by on_key_event.
    fn on_key_event_inner(&mut self, event: &KeyEvent) -> bool {
        use KeyEventType::KeyDown;

        // Only process KeyDown events for clipboard shortcuts
        if event.event_type == KeyDown && event.modifiers.command_or_ctrl() {
            // Desktop-only clipboard handling via arboard
            // Use persistent self.clipboard to keep content alive on Linux X11
            #[cfg(all(
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            {
                match event.key_code {
                    // Ctrl+C - Copy
                    KeyCode::C => {
                        // Get text first, then access clipboard to avoid borrow conflict
                        let text = self.on_copy();
                        if let (Some(text), Some(clipboard)) = (text, self.clipboard.as_mut()) {
                            let _ = clipboard.set_text(&text);
                            return true;
                        }
                    }
                    // Ctrl+X - Cut
                    KeyCode::X => {
                        // Get text first (this also deletes it), then access clipboard
                        let text = self.on_cut();
                        if let (Some(text), Some(clipboard)) = (text, self.clipboard.as_mut()) {
                            let _ = clipboard.set_text(&text);
                            self.mark_dirty();
                            self.request_layout_pass();
                            return true;
                        }
                    }
                    // Ctrl+V - Paste
                    KeyCode::V => {
                        // Get text from clipboard first, then paste
                        let text = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok());
                        if let Some(text) = text {
                            if self.on_paste(&text) {
                                return true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Pure O(1) dispatch - no tree walking needed
        if !cranpose_ui::text_field_focus::has_focused_field() {
            return false;
        }

        // Wrap key event handling in a mutable snapshot so changes are atomically applied.
        // This ensures keyboard input modifications are visible to subsequent snapshot contexts
        // (like button click handlers that run in their own mutable snapshots).
        let handled = run_in_mutable_snapshot(|| {
            // O(1) dispatch via stored handler - handles ALL text input key events
            // No fallback needed since handler now handles arrows, Home/End, word nav
            cranpose_ui::text_field_focus::dispatch_key_event(event)
        })
        .unwrap_or(false);

        if handled {
            // Mark both dirty (for redraw) and request a layout pass to rebuild semantics.
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    /// Handles paste event from platform clipboard.
    /// Returns `true` if the paste was consumed by a focused text field.
    /// O(1) operation using stored handler.
    pub fn on_paste(&mut self, text: &str) -> bool {
        // Wrap paste in a mutable snapshot so changes are atomically applied.
        // This ensures paste modifications are visible to subsequent snapshot contexts
        // (like button click handlers that run in their own mutable snapshots).
        let handled =
            run_in_mutable_snapshot(|| cranpose_ui::text_field_focus::dispatch_paste(text))
                .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    /// Handles copy request from platform.
    /// Returns the selected text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_copy(&mut self) -> Option<String> {
        // Use O(1) dispatch instead of tree scan
        cranpose_ui::text_field_focus::dispatch_copy()
    }

    /// Handles cut request from platform.
    /// Returns the cut text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_cut(&mut self) -> Option<String> {
        // Use O(1) dispatch instead of tree scan
        let text = cranpose_ui::text_field_focus::dispatch_cut();

        if text.is_some() {
            self.mark_dirty();
            self.request_layout_pass();
        }

        text
    }

    /// Sets the Linux primary selection (for middle-click paste).
    /// This is called when text is selected in a text field.
    /// On non-Linux platforms, this is a no-op.
    #[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
    pub fn set_primary_selection(&mut self, text: &str) {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        if let Some(ref mut clipboard) = self.clipboard {
            let result = clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text.to_string());
            if let Err(e) = result {
                // Primary selection may not be available on all systems
                log::debug!("Primary selection set failed: {:?}", e);
            }
        }
    }

    /// Gets text from the Linux primary selection (for middle-click paste).
    /// On non-Linux platforms, returns None.
    #[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
    pub fn get_primary_selection(&mut self) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        if let Some(ref mut clipboard) = self.clipboard {
            clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text()
                .ok()
        } else {
            None
        }
    }

    #[cfg(all(
        not(target_os = "linux"),
        not(target_arch = "wasm32"),
        not(target_os = "ios")
    ))]
    pub fn get_primary_selection(&mut self) -> Option<String> {
        None
    }

    /// Syncs the current text field selection to PRIMARY (Linux X11).
    /// Call this when selection changes in a text field.
    pub fn sync_selection_to_primary(&mut self) {
        #[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
        {
            if let Some(text) = self.on_copy() {
                self.set_primary_selection(&text);
            }
        }
    }

    /// Handles IME preedit (composition) events.
    /// Called when the input method is composing text (e.g., typing CJK characters).
    ///
    /// - `text`: The current preedit text (empty to clear composition state)
    /// - `cursor`: Optional cursor position within the preedit text (start, end)
    ///
    /// Returns `true` if a text field consumed the event.
    pub fn on_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        // Wrap in mutable snapshot for atomic changes
        let handled = run_in_mutable_snapshot(|| {
            cranpose_ui::text_field_focus::dispatch_ime_preedit(text, cursor)
        })
        .unwrap_or(false);

        if handled {
            self.mark_dirty();
            // IME composition changes the visible text, needs layout update
            self.request_layout_pass();
        }

        handled
    }

    /// Handles IME delete-surrounding events.
    /// Returns `true` if a text field consumed the event.
    pub fn on_ime_delete_surrounding(&mut self, before_bytes: usize, after_bytes: usize) -> bool {
        let handled = run_in_mutable_snapshot(|| {
            cranpose_ui::text_field_focus::dispatch_delete_surrounding(before_bytes, after_bytes)
        })
        .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    pub fn debug_info_report(&mut self) -> String {
        let mut report = String::new();
        writeln!(report, "=== DEBUG: CURRENT SCREEN STATE ===").ok();
        if let Some(layout_tree) = self.layout_tree() {
            let renderer = HeadlessRenderer::new();
            let render_scene = renderer.render(layout_tree);
            writeln!(report, "{}", format_layout_tree(layout_tree)).ok();
            writeln!(report, "{}", format_render_scene(&render_scene)).ok();
            writeln!(
                report,
                "{}",
                format_screen_summary(layout_tree, &render_scene)
            )
            .ok();
        } else {
            writeln!(report, "No layout available").ok();
        }
        report
    }

    pub fn log_debug_info(&mut self) -> String {
        let report = self.debug_info_report();
        log::info!(target: "cranpose::debug::screen", "\n{report}");
        report
    }

    /// Get the current layout tree (for robot/testing)
    pub fn layout_tree(&mut self) -> Option<&LayoutTree> {
        self.layout_tree.as_ref()
    }

    /// Get the current semantics tree (for robot/testing)
    pub fn semantics_tree(&self) -> Option<&SemanticsTree> {
        self.semantics_tree.as_ref()
    }

    pub fn root_layout_size(&mut self) -> Option<(f32, f32)> {
        self.layout_tree().map(|tree| {
            let root = tree.root();
            (root.rect.width, root.rect.height)
        })
    }

    pub fn node_layout_bounds(&mut self, target: NodeId) -> Option<(f32, f32, f32, f32)> {
        self.layout_tree()
            .and_then(|tree| find_layout_box(tree.root(), target))
            .map(layout_box_bounds)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_runtime_leak_stats(&mut self) -> RuntimeLeakDebugStats {
        let runtime = self.composition.runtime_handle();
        let (applier_stats, live_node_heap_bytes, recycled_node_heap_bytes) = {
            let applier = self.composition.applier_mut();
            (
                applier.debug_stats(),
                applier.debug_live_node_heap_bytes(),
                applier.debug_recycled_node_heap_bytes(),
            )
        };
        RuntimeLeakDebugStats {
            applier_stats,
            live_node_heap_bytes,
            recycled_node_heap_bytes,
            slot_table_heap_bytes: self.composition.slot_table_heap_bytes(),
            pass_stats: self.composition.debug_last_pass_stats(),
            slot_stats: self.composition.debug_slot_table_stats(),
            observer_stats: self.composition.debug_observer_stats(),
            runtime_stats: runtime.debug_stats(),
            state_arena_stats: runtime.state_arena_debug_stats(),
            recompose_scope_stats: debug_recompose_scope_registry_stats(),
            snapshot_v2_stats: debug_snapshot_v2_stats(),
            snapshot_pinning_stats: debug_snapshot_pinning_stats(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_slot_table_groups(&self) -> Vec<(usize, Key, Option<usize>, usize)> {
        self.composition.debug_dump_slot_table_groups()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_all_slots(&self) -> Vec<(usize, String)> {
        self.composition.debug_dump_all_slots()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn runtime_handle(&self) -> cranpose_core::RuntimeHandle {
        self.composition.runtime_handle()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_live_subcompose_scope_ids(&mut self) -> Vec<(NodeId, Vec<(u64, Vec<usize>)>)> {
        fn collect_node_ids(layout: &LayoutBox, out: &mut Vec<NodeId>) {
            out.push(layout.node_id);
            for child in &layout.children {
                collect_node_ids(child, out);
            }
        }

        let mut node_ids = Vec::new();
        if let Some(tree) = self.layout_tree() {
            collect_node_ids(tree.root(), &mut node_ids);
        }

        let mut applier = self.composition.applier_mut();
        let mut result = Vec::new();
        for node_id in node_ids {
            if let Ok(scope_ids) = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_scope_ids_by_slot()
            }) {
                result.push((node_id, scope_ids));
            }
        }
        result
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_subcompose_slot_table(
        &mut self,
        node_id: NodeId,
        slot_id: u64,
    ) -> Option<Vec<(usize, String)>> {
        let mut applier = self.composition.applier_mut();
        applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_slot_table_for_slot(SlotId::new(slot_id))
            })
            .ok()
            .flatten()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_subcompose_slot_groups(
        &mut self,
        node_id: NodeId,
        slot_id: u64,
    ) -> Option<Vec<(usize, Key, Option<usize>, usize)>> {
        let mut applier = self.composition.applier_mut();
        applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_slot_table_groups_for_slot(SlotId::new(slot_id))
            })
            .ok()
            .flatten()
    }

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

    fn process_frame(&mut self) {
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

    fn run_layout_phase(&mut self) {
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

    fn run_render_phase(&mut self) {
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

fn find_layout_box(layout_box: &LayoutBox, target: NodeId) -> Option<&LayoutBox> {
    if layout_box.node_id == target {
        return Some(layout_box);
    }

    layout_box
        .children
        .iter()
        .find_map(|child| find_layout_box(child, target))
}

fn layout_box_bounds(layout_box: &LayoutBox) -> (f32, f32, f32, f32) {
    (
        layout_box.rect.x,
        layout_box.rect.y,
        layout_box.rect.width,
        layout_box.rect.height,
    )
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

fn build_draw_refresh_scope(
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
