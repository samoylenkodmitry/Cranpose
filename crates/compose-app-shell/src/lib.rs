#![allow(clippy::type_complexity)]

mod hit_path_tracker;

use std::fmt::Debug;
// Use web_time for cross-platform time support (native + WASM) - compatible with winit
use web_time::Instant;

use compose_core::{
    enter_event_handler, exit_event_handler, location_key, run_in_mutable_snapshot, Applier,
    Composition, Key, MemoryApplier, NodeError,
};
use compose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_runtime_std::StdRuntime;
use compose_ui::{
    has_pending_focus_invalidations, has_pending_pointer_repasses, log_layout_tree,
    log_render_scene, log_screen_summary, peek_focus_invalidation, peek_layout_invalidation,
    peek_pointer_invalidation, peek_render_invalidation, process_focus_invalidations,
    process_pointer_repasses, request_render_invalidation, take_focus_invalidation,
    take_layout_invalidation, take_pointer_invalidation, take_render_invalidation,
    HeadlessRenderer, LayoutNode, LayoutTree, SemanticsTree,
};
use compose_ui_graphics::{Point, Size};
use hit_path_tracker::{HitPathTracker, PointerId};

// Re-export key event types for use by compose-app
pub use compose_ui::{KeyCode, KeyEvent, KeyEventType, Modifiers};

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
    semantics_tree: Option<SemanticsTree>,
    layout_dirty: bool,
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
    /// Persistent clipboard for desktop (Linux X11 requires clipboard to stay alive)
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    clipboard: Option<arboard::Clipboard>,
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
            semantics_tree: None,
            layout_dirty: true,
            scene_dirty: true,
            is_dirty: true,
            buttons_pressed: PointerButtons::NONE,
            hit_path_tracker: HitPathTracker::new(),
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
            clipboard: arboard::Clipboard::new().ok(),
        };
        shell.process_frame();
        shell
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.layout_dirty = true;
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
        if self.layout_dirty
            || self.scene_dirty
            || peek_render_invalidation()
            || peek_pointer_invalidation()
            || peek_focus_invalidation()
            || peek_layout_invalidation()
        {
            return true;
        }
        self.runtime.take_frame_request() || self.composition.should_render()
    }

    /// Returns true if the shell needs to redraw (dirty flag, layout dirty, active animations).
    /// Note: Cursor blink is now timer-based and uses WaitUntil scheduling, not continuous redraw.
    pub fn needs_redraw(&self) -> bool {
        self.is_dirty || self.layout_dirty || self.has_active_animations()
    }

    /// Marks the shell as dirty, indicating a redraw is needed.
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    /// Returns true if there are active animations or pending recompositions.
    pub fn has_active_animations(&self) -> bool {
        self.runtime.take_frame_request() || self.composition.should_render()
    }

    /// Returns the next scheduled event time for cursor blink.
    /// Use this for `ControlFlow::WaitUntil` scheduling.
    pub fn next_event_time(&self) -> Option<web_time::Instant> {
        compose_ui::next_cursor_blink_time()
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
        let Some(node_ids) = self.hit_path_tracker.get_path(pointer) else {
            return Vec::new();
        };

        let scene = self.renderer.scene();
        node_ids
            .iter()
            .filter_map(|&id| scene.find_target(id))
            .collect()
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let frame_time = now
            .checked_duration_since(self.start_time)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.runtime.drain_frame_callbacks(frame_time);
        self.runtime.runtime_handle().drain_ui();
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
        // Clear dirty flag after update (frame has been processed)
        self.is_dirty = false;
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);

        // During a gesture (button pressed), ONLY dispatch to the tracked hit path.
        // Never fall back to hover hit-testing while buttons are down.
        // This maintains the invariant: the path that receives Down must receive Move and Up/Cancel.
        if self.buttons_pressed != PointerButtons::NONE {
            if self.hit_path_tracker.has_path(PointerId::PRIMARY) {
                // Resolve fresh targets from current scene (not cached geometry!)
                let targets = self.resolve_hit_path(PointerId::PRIMARY);

                if !targets.is_empty() {
                    let event =
                        PointerEvent::new(PointerEventKind::Move, Point { x, y }, Point { x, y })
                            .with_buttons(self.buttons_pressed);

                    for hit in targets {
                        hit.dispatch(event.clone());
                        if event.is_consumed() {
                            break;
                        }
                    }
                    self.mark_dirty();
                    return true;
                }

                // Gesture exists but we can't resolve any nodes (removed / no hit region).
                // Do NOT switch to hover mode while buttons are pressed.
                return false;
            }

            // Button is down but we have no recorded path inside this app
            // (e.g. drag started outside). Do not dispatch anything.
            return false;
        }

        // No gesture in progress: regular hover move using hit-test.
        let hits = self.renderer.scene().hit_test(x, y);
        if !hits.is_empty() {
            let event = PointerEvent::new(PointerEventKind::Move, Point { x, y }, Point { x, y })
                .with_buttons(self.buttons_pressed); // usually NONE here
            for hit in hits {
                hit.dispatch(event.clone());
                if event.is_consumed() {
                    break;
                }
            }
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    pub fn pointer_pressed(&mut self) -> bool {
        enter_event_handler();
        let result = self.pointer_pressed_inner();
        exit_event_handler();
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

        // Cache NodeIds for this pointer
        let node_ids: Vec<_> = hits.iter().map(|h| h.node_id()).collect();
        self.hit_path_tracker
            .add_hit_path(PointerId::PRIMARY, node_ids);

        if !hits.is_empty() {
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

            // Dispatch to fresh hits (geometry is already current for Down event)
            for hit in hits {
                hit.dispatch(event.clone());
                if event.is_consumed() {
                    break;
                }
            }
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    pub fn pointer_released(&mut self) -> bool {
        enter_event_handler();
        let result = self.pointer_released_inner();
        exit_event_handler();
        result
    }

    fn pointer_released_inner(&mut self) -> bool {
        // UP events report buttons as "currently pressed" (after release),
        // matching typical platform semantics where primary is already gone.
        self.buttons_pressed.remove(PointerButton::Primary);
        let corrected_buttons = self.buttons_pressed;

        // Resolve FRESH targets from cached NodeIds
        let targets = self.resolve_hit_path(PointerId::PRIMARY);

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

            for hit in targets {
                hit.dispatch(event.clone());
                if event.is_consumed() {
                    break;
                }
            }
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// Cancels any active gesture, dispatching Cancel events to cached targets.
    /// Call this when:
    /// - Window loses focus
    /// - Mouse leaves window while button pressed
    /// - Any other gesture abort scenario
    pub fn cancel_gesture(&mut self) {
        // Resolve FRESH targets from cached NodeIds
        let targets = self.resolve_hit_path(PointerId::PRIMARY);

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

            for hit in targets {
                hit.dispatch(event.clone());
            }
            self.mark_dirty();
        }
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
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
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
                            self.layout_dirty = true;
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
        if !compose_ui::text_field_focus::has_focused_field() {
            return false;
        }

        // Wrap key event handling in a mutable snapshot so changes are atomically applied.
        // This ensures keyboard input modifications are visible to subsequent snapshot contexts
        // (like button click handlers that run in their own mutable snapshots).
        let handled = run_in_mutable_snapshot(|| {
            // O(1) dispatch via stored handler - handles ALL text input key events
            // No fallback needed since handler now handles arrows, Home/End, word nav
            compose_ui::text_field_focus::dispatch_key_event(event)
        })
        .unwrap_or(false);

        if handled {
            // Mark both dirty (for redraw) and layout_dirty (to rebuild semantics tree)
            self.mark_dirty();
            self.layout_dirty = true;
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
            run_in_mutable_snapshot(|| compose_ui::text_field_focus::dispatch_paste(text))
                .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.layout_dirty = true;
        }

        handled
    }

    /// Handles copy request from platform.
    /// Returns the selected text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_copy(&mut self) -> Option<String> {
        // Use O(1) dispatch instead of tree scan
        compose_ui::text_field_focus::dispatch_copy()
    }

    /// Handles cut request from platform.
    /// Returns the cut text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_cut(&mut self) -> Option<String> {
        // Use O(1) dispatch instead of tree scan
        let text = compose_ui::text_field_focus::dispatch_cut();

        if text.is_some() {
            self.mark_dirty();
            self.layout_dirty = true;
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
            compose_ui::text_field_focus::dispatch_ime_preedit(text, cursor)
        })
        .unwrap_or(false);

        if handled {
            self.mark_dirty();
            // IME composition changes the visible text, needs layout update
            self.layout_dirty = true;
        }

        handled
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

    /// Get the current layout tree (for robot/testing)
    pub fn layout_tree(&self) -> Option<&LayoutTree> {
        self.layout_tree.as_ref()
    }

    /// Get the current semantics tree (for robot/testing)
    pub fn semantics_tree(&self) -> Option<&SemanticsTree> {
        self.semantics_tree.as_ref()
    }

    fn process_frame(&mut self) {
        self.run_layout_phase();
        self.run_dispatch_queues();
        self.run_render_phase();
    }

    fn run_layout_phase(&mut self) {
        // ═══════════════════════════════════════════════════════════════════════════════
        // SCOPED LAYOUT REPASSES (preferred path for local changes)
        // ═══════════════════════════════════════════════════════════════════════════════
        // Process node-specific layout invalidations (e.g., from scroll).
        // This bubbles dirty flags up from specific nodes WITHOUT invalidating all caches.
        // Result: O(subtree) remeasurement, not O(app).
        let repass_nodes = compose_ui::take_layout_repass_nodes();
        if !repass_nodes.is_empty() {
            let mut applier = self.composition.applier_mut();
            for node_id in repass_nodes {
                compose_core::bubble_layout_dirty(
                    &mut *applier as &mut dyn compose_core::Applier,
                    node_id,
                );
            }
            drop(applier);
            self.layout_dirty = true;
        }

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

        // If invalidation was requested (e.g., text field content changed),
        // we must invalidate caches AND mark for remeasure so intrinsic sizes are recalculated.
        // This happens regardless of whether layout_dirty was already set from keyboard handling.
        if invalidation_requested {
            // Invalidate all caches (O(app size) - expensive!)
            // This is internal-only API, only accessible via the internal path
            compose_ui::layout::invalidate_all_layout_caches();

            // Mark root as needing layout AND measure so tree_needs_layout() returns true
            // and intrinsic sizes are recalculated (e.g., text field resizing on content change)
            if let Some(root) = self.composition.root() {
                let mut applier = self.composition.applier_mut();
                if let Ok(node) = applier.get_mut(root) {
                    if let Some(layout_node) =
                        node.as_any_mut().downcast_mut::<compose_ui::LayoutNode>()
                    {
                        layout_node.mark_needs_measure();
                        layout_node.mark_needs_layout();
                    }
                }
            }
            self.layout_dirty = true;
        }

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
            // UNLESS layout_dirty was explicitly set (e.g., from keyboard input)
            let tree_needs_layout_check = compose_ui::tree_needs_layout(&mut *applier, root)
                .unwrap_or_else(|err| {
                    log::warn!(
                        "Cannot check layout dirty status for root #{}: {}",
                        root,
                        err
                    );
                    true // Assume dirty on error
                });

            // Force layout if either:
            // 1. Tree nodes are marked dirty (tree_needs_layout_check = true)
            // 2. layout_dirty was explicitly set (e.g., from keyboard/external events)
            let needs_layout = tree_needs_layout_check || self.layout_dirty;

            if !needs_layout {
                // Tree is clean and no external dirtying - skip layout computation
                log::trace!("Skipping layout: tree is clean");
                self.layout_dirty = false;
                applier.clear_runtime_handle();
                return;
            }

            // Tree needs layout - compute it
            self.layout_dirty = false;

            // Ensure slots exist and borrow mutably (handled inside measure_layout via MemoryApplier)
            match compose_ui::measure_layout(&mut applier, root, viewport_size) {
                Ok(measurements) => {
                    self.semantics_tree = Some(measurements.semantics_tree().clone());
                    self.layout_tree = Some(measurements.into_layout_tree());
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
        // Tick cursor blink timer - only marks dirty when visibility state changes
        let cursor_blink_dirty = compose_ui::tick_cursor_blink();
        if render_dirty || pointer_dirty || focus_dirty || cursor_blink_dirty {
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
