#![allow(clippy::type_complexity)]

mod fps_monitor;
mod hit_path_tracker;

// Re-export FPS monitoring API
pub use fps_monitor::{
    current_fps, fps_display, fps_display_detailed, fps_stats, record_recomposition, FpsStats,
};

use std::fmt::Debug;
use std::sync::OnceLock;
// Use web_time for cross-platform time support (native + WASM) - compatible with winit
use web_time::Instant;

use cranpose_core::{
    debug_recompose_scope_registry_stats, debug_snapshot_pinning_stats,
    debug_snapshot_state_thread_local_stats, debug_snapshot_v2_stats, enter_event_handler,
    exit_event_handler, location_key, run_in_mutable_snapshot, Applier, Composition, Key,
    MemoryApplier, MemoryApplierDebugStats, Node, NodeError, NodeId, SlotId,
};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_runtime_std::StdRuntime;
use cranpose_ui::{
    has_pending_focus_invalidations, has_pending_pointer_repasses, log_layout_tree,
    log_render_scene, log_screen_summary, peek_focus_invalidation, peek_layout_invalidation,
    peek_pointer_invalidation, peek_render_invalidation, process_focus_invalidations,
    process_pointer_repasses, request_render_invalidation, take_draw_repass_nodes,
    take_focus_invalidation, take_layout_invalidation, take_pointer_invalidation,
    take_render_invalidation, HeadlessRenderer, LayoutBox, LayoutNode, LayoutNodeData,
    LayoutNodeKind, LayoutTree, MeasureLayoutOptions, SemanticsTree, SubcomposeLayoutNode,
};
use cranpose_ui_graphics::{Point, Size};
use hit_path_tracker::{HitPathTracker, PointerId};
use std::collections::{HashMap, HashSet};

// Re-export key event types for use by cranpose
pub use cranpose_ui::{KeyCode, KeyEvent, KeyEventType, Modifiers};

#[derive(Copy, Clone)]
enum DispatchInvalidationKind {
    Pointer,
    Focus,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum GestureTargetSource {
    HitPath,
    CaptureContext,
    Captured,
    None,
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
    /// Original hit targets captured on PointerDown.
    ///
    /// These preserve handler closure state even if node IDs disappear from the
    /// current scene during a gesture.
    captured_targets: HashMap<PointerId, Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget>>,
    /// Tracks which nodes the pointer is currently hovering over.
    /// Used to synthesize Enter/Exit events when the hover set changes.
    hovered_nodes: Vec<NodeId>,
    /// Stable translated-content identities for active pointer gestures.
    capture_contexts: HashMap<PointerId, usize>,
    /// Node IDs that most recently received gesture events for each pointer.
    gesture_target_ids: HashMap<PointerId, Vec<NodeId>>,
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

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct RuntimeLeakDebugStats {
    pub applier_stats: MemoryApplierDebugStats,
    pub live_node_heap_bytes: usize,
    pub recycled_node_heap_bytes: usize,
    pub slot_table_heap_bytes: usize,
    pub pass_stats: cranpose_core::CompositionPassDebugStats,
    pub slot_stats: cranpose_core::SlotTableDebugStats,
    pub observer_stats: cranpose_core::SnapshotStateObserverDebugStats,
    pub runtime_stats: cranpose_core::RuntimeDebugStats,
    pub state_arena_stats: cranpose_core::StateArenaDebugStats,
    pub recompose_scope_stats: cranpose_core::RecomposeScopeRegistryDebugStats,
    pub snapshot_v2_stats: cranpose_core::SnapshotV2DebugStats,
    pub snapshot_pinning_stats: cranpose_core::SnapshotPinningDebugStats,
    pub snapshot_state_stats: cranpose_core::SnapshotStateThreadLocalDebugStats,
}

fn input_pipeline_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_INPUT_DEBUG").is_some())
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    fn node_parent(&mut self, node_id: NodeId) -> Option<NodeId> {
        if let Ok(parent) = self
            .composition
            .applier_mut()
            .with_node::<LayoutNode, _>(node_id, |node| node.folded_parent())
        {
            return parent;
        }

        self.composition
            .applier_mut()
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| node.parent())
            .ok()
            .flatten()
    }

    fn node_translated_content_context_identity(&mut self, node_id: NodeId) -> Option<usize> {
        if let Ok(identity) =
            self.composition
                .applier_mut()
                .with_node::<LayoutNode, _>(node_id, |node| {
                    node.modifier_slices_snapshot()
                        .translated_content_context_identity()
                })
        {
            return identity;
        }

        self.composition
            .applier_mut()
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.modifier_slices_snapshot()
                    .translated_content_context_identity()
            })
            .ok()
            .flatten()
    }

    fn node_has_pointer_handlers(&mut self, node_id: NodeId) -> bool {
        if let Ok(has_handlers) = self
            .composition
            .applier_mut()
            .with_node::<LayoutNode, _>(node_id, |node| {
                !node.modifier_slices_snapshot().pointer_inputs().is_empty()
            })
        {
            return has_handlers;
        }

        self.composition
            .applier_mut()
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                !node.modifier_slices_snapshot().pointer_inputs().is_empty()
            })
            .unwrap_or(false)
    }

    fn translated_content_context_identity_for_hits(
        &mut self,
        hit_node_ids: &[NodeId],
    ) -> Option<usize> {
        for &node_id in hit_node_ids {
            let mut current = Some(node_id);
            while let Some(candidate) = current {
                if let Some(identity) = self.node_translated_content_context_identity(candidate) {
                    return Some(identity);
                }
                current = self.node_parent(candidate);
            }
        }

        None
    }

    fn collect_live_node_ids(
        applier: &mut MemoryApplier,
        node_id: NodeId,
        depth: usize,
        out: &mut Vec<(usize, NodeId)>,
    ) {
        out.push((depth, node_id));

        if let Ok(children) =
            applier.with_node::<LayoutNode, _>(node_id, |node| node.children.clone())
        {
            for child_id in children {
                Self::collect_live_node_ids(applier, child_id, depth + 1, out);
            }
            return;
        }

        if let Ok(children) =
            applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| node.active_children())
        {
            for child_id in children {
                Self::collect_live_node_ids(applier, child_id, depth + 1, out);
            }
        }
    }

    fn resolve_capture_context_targets(
        &mut self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        let Some(identity) = self.capture_contexts.get(&pointer).copied() else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        let Some(root_id) = self.composition.root() else {
            return Vec::new();
        };
        {
            let mut applier = self.composition.applier_mut();
            Self::collect_live_node_ids(&mut applier, root_id, 0, &mut candidates);
        }
        candidates.sort_by_key(|candidate| candidate.0);

        let mut resolved = Vec::new();
        let mut seen = HashSet::new();
        for (_, node_id) in candidates {
            if !seen.insert(node_id) {
                continue;
            }
            if self.node_translated_content_context_identity(node_id) != Some(identity) {
                continue;
            }
            if !self.node_has_pointer_handlers(node_id) {
                continue;
            }
            if let Some(target) = self.renderer.scene().find_target(node_id) {
                resolved.push(target);
            }
        }

        if input_pipeline_debug_enabled() {
            let resolved_ids: Vec<_> = resolved.iter().map(|target| target.node_id()).collect();
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] resolve_capture_context_targets pointer={:?} identity={} resolved={:?}",
                pointer, identity, resolved_ids
            );
        }

        resolved
    }

    fn captured_targets_for_pointer(
        &self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        self.captured_targets
            .get(&pointer)
            .cloned()
            .unwrap_or_default()
    }

    fn resolve_gesture_targets(
        &mut self,
        pointer: PointerId,
    ) -> (
        Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget>,
        GestureTargetSource,
    ) {
        let targets = self.resolve_hit_path(pointer);
        if !targets.is_empty() {
            return (targets, GestureTargetSource::HitPath);
        }

        if self.capture_contexts.contains_key(&pointer) {
            let capture_context_targets = self.resolve_capture_context_targets(pointer);
            if !capture_context_targets.is_empty() {
                return (capture_context_targets, GestureTargetSource::CaptureContext);
            }

            let captured_targets = self.captured_targets_for_pointer(pointer);
            if !captured_targets.is_empty() {
                return (captured_targets, GestureTargetSource::Captured);
            }
        }

        (Vec::new(), GestureTargetSource::None)
    }

    fn record_gesture_target_ids(
        &mut self,
        pointer: PointerId,
        targets: &[<<R as Renderer>::Scene as RenderScene>::HitTarget],
    ) {
        let node_ids = targets.iter().map(|target| target.node_id()).collect();
        self.gesture_target_ids.insert(pointer, node_ids);
    }

    fn prime_retargeted_capture_context_targets(
        &mut self,
        pointer: PointerId,
        previous_position: Point,
        targets: &[<<R as Renderer>::Scene as RenderScene>::HitTarget],
    ) {
        let current_ids: Vec<_> = targets.iter().map(|target| target.node_id()).collect();
        let previous_ids = self
            .gesture_target_ids
            .get(&pointer)
            .cloned()
            .unwrap_or_default();
        if current_ids == previous_ids {
            return;
        }

        let event = PointerEvent::new(PointerEventKind::Down, previous_position, previous_position)
            .with_buttons(self.buttons_pressed);
        self.dispatch_targets(targets.iter().cloned(), event, true);
        self.gesture_target_ids.insert(pointer, current_ids);
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
            layout_dirty: true,
            scene_dirty: true,
            is_dirty: true,
            buttons_pressed: PointerButtons::NONE,
            hit_path_tracker: HitPathTracker::new(),
            captured_targets: HashMap::new(),
            hovered_nodes: Vec::new(),
            capture_contexts: HashMap::new(),
            gesture_target_ids: HashMap::new(),
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
        if self.is_dirty
            || self.layout_dirty
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

    /// Returns true if there are active animations or pending recompositions.
    pub fn has_active_animations(&self) -> bool {
        self.runtime.take_frame_request() || self.composition.should_render()
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
        let Some(node_ids) = self.hit_path_tracker.get_path(pointer) else {
            return Vec::new();
        };

        let scene = self.renderer.scene();
        let targets: Vec<_> = node_ids
            .iter()
            .filter_map(|&id| scene.find_target(id))
            .collect();
        if input_pipeline_debug_enabled() {
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] resolve_hit_path pointer={:?} cached={:?} resolved_count={}",
                pointer,
                node_ids,
                targets.len()
            );
        }
        targets
    }

    fn dispatch_targets<I>(&mut self, targets: I, event: PointerEvent, stop_on_consume: bool)
    where
        I: IntoIterator<Item = <<R as Renderer>::Scene as RenderScene>::HitTarget>,
    {
        let mut applier = self.composition.applier_mut();
        for target in targets {
            target.dispatch_with_applier(&mut applier, event.clone());
            if stop_on_consume && event.is_consumed() {
                break;
            }
        }
    }

    fn sync_pending_input_recompositions(&mut self) {
        let runtime_handle = self.runtime.runtime_handle();
        runtime_handle.with_deferred_state_releases(|| {
            if self.composition.should_render() {
                match self.composition.process_invalid_scopes() {
                    Ok(changed) => {
                        if changed {
                            fps_monitor::record_recomposition();
                            self.layout_dirty = true;
                            request_render_invalidation();
                        }
                    }
                    Err(NodeError::Missing { id }) => {
                        log::debug!(
                            "Recomposition skipped during input sync: node {id} no longer exists"
                        );
                        self.layout_dirty = true;
                        request_render_invalidation();
                    }
                    Err(err) => {
                        log::error!("input sync recomposition failed: {err}");
                        self.layout_dirty = true;
                        request_render_invalidation();
                    }
                }
            }

            if self.composition.take_root_render_request() {
                if let Err(err) = self.composition.render(self.root_key, &mut *self.content) {
                    log::error!("input sync root render failed: {err}");
                } else {
                    fps_monitor::record_recomposition();
                    self.layout_dirty = true;
                    request_render_invalidation();
                }
            }
        });
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
            if input_pipeline_debug_enabled() && should_render {
                eprintln!(
                    "[CRANPOSE_INPUT_DEBUG] update begin: should_render=true layout_dirty={} scene_dirty={} is_dirty={}",
                    self.layout_dirty, self.scene_dirty, self.is_dirty
                );
            }
            if should_render {
                match self.composition.process_invalid_scopes() {
                    Ok(changed) => {
                        if input_pipeline_debug_enabled() {
                            eprintln!(
                                "[CRANPOSE_INPUT_DEBUG] process_invalid_scopes changed={}",
                                changed
                            );
                        }
                        if changed {
                            fps_monitor::record_recomposition();
                            self.layout_dirty = true;
                            // Force root needs_measure since bubbling may fail for
                            // subcomposition nodes with broken parent chains (node 226 issue)
                            if let Some(root_id) = self.composition.root() {
                                let mut applier = self.composition.applier_mut();
                                match applier.with_node::<LayoutNode, _>(root_id, |node| {
                                    node.mark_needs_measure();
                                }) {
                                    Ok(()) | Err(NodeError::Missing { .. }) => {}
                                    Err(NodeError::TypeMismatch { .. }) => {
                                        let _ = applier.with_node::<SubcomposeLayoutNode, _>(
                                            root_id,
                                            |node| {
                                                node.mark_needs_measure();
                                            },
                                        );
                                    }
                                    Err(_) => {}
                                }
                            }
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
            if self.composition.take_root_render_request() {
                if let Err(err) = self.composition.render(self.root_key, &mut *self.content) {
                    log::error!("root render fallback failed: {err}");
                } else {
                    fps_monitor::record_recomposition();
                    self.layout_dirty = true;
                    request_render_invalidation();
                }
            }
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
        if input_pipeline_debug_enabled() {
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] set_cursor ({:.2},{:.2}) -> {}",
                x, y, result
            );
        }
        result
    }

    fn set_cursor_inner(&mut self, x: f32, y: f32) -> bool {
        self.sync_pending_input_recompositions();
        let previous_position = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };
        self.cursor = (x, y);

        // During a gesture (button pressed), ONLY dispatch to the tracked hit path.
        // Never fall back to hover hit-testing while buttons are down.
        // This maintains the invariant: the path that receives Down must receive Move and Up/Cancel.
        if self.buttons_pressed != PointerButtons::NONE {
            if self.hit_path_tracker.has_path(PointerId::PRIMARY) {
                let (targets, source) = self.resolve_gesture_targets(PointerId::PRIMARY);
                if !targets.is_empty() {
                    if source == GestureTargetSource::CaptureContext {
                        self.prime_retargeted_capture_context_targets(
                            PointerId::PRIMARY,
                            previous_position,
                            &targets,
                        );
                    } else {
                        self.record_gesture_target_ids(PointerId::PRIMARY, &targets);
                    }
                    let event =
                        PointerEvent::new(PointerEventKind::Move, Point { x, y }, Point { x, y })
                            .with_buttons(self.buttons_pressed);
                    self.dispatch_targets(targets, event, true);
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
        if input_pipeline_debug_enabled() {
            eprintln!("[CRANPOSE_INPUT_DEBUG] pointer_pressed -> {}", result);
        }
        result
    }

    fn pointer_pressed_inner(&mut self) -> bool {
        self.sync_pending_input_recompositions();
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
        let mut node_ids = Vec::new();
        let mut seen = HashSet::new();
        for hit in &hits {
            for node_id in hit.capture_path() {
                if seen.insert(node_id) {
                    node_ids.push(node_id);
                }
            }
        }
        let raw_node_ids: Vec<_> = hits.iter().map(|hit| hit.node_id()).collect();
        self.hit_path_tracker
            .add_hit_path(PointerId::PRIMARY, node_ids);
        let capture_identity = self.translated_content_context_identity_for_hits(&raw_node_ids);
        if let Some(identity) = capture_identity {
            self.capture_contexts.insert(PointerId::PRIMARY, identity);
        } else {
            self.capture_contexts.remove(&PointerId::PRIMARY);
        }
        if hits.is_empty() {
            self.captured_targets.remove(&PointerId::PRIMARY);
            self.gesture_target_ids.remove(&PointerId::PRIMARY);
        } else {
            self.captured_targets.insert(
                PointerId::PRIMARY,
                self.resolve_hit_path(PointerId::PRIMARY),
            );
            self.record_gesture_target_ids(PointerId::PRIMARY, &hits);
        }
        if input_pipeline_debug_enabled() {
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] pointer_pressed_inner cached_hit_path={:?} capture_identity={:?}",
                self.hit_path_tracker.get_path(PointerId::PRIMARY),
                capture_identity
            );
        }

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
            self.dispatch_targets(hits, event, true);
            true
        } else {
            false
        }
    }

    pub fn pointer_released(&mut self) -> bool {
        enter_event_handler();
        let result = run_in_mutable_snapshot(|| self.pointer_released_inner()).unwrap_or(false);
        exit_event_handler();
        if result {
            self.mark_dirty();
        }
        if input_pipeline_debug_enabled() {
            eprintln!("[CRANPOSE_INPUT_DEBUG] pointer_released -> {}", result);
        }
        result
    }

    fn pointer_released_inner(&mut self) -> bool {
        self.sync_pending_input_recompositions();
        // UP events report buttons as "currently pressed" (after release),
        // matching typical platform semantics where primary is already gone.
        self.buttons_pressed.remove(PointerButton::Primary);
        let corrected_buttons = self.buttons_pressed;
        let (targets, _) = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Always remove the path, even if targets is empty (node may have been removed)
        self.hit_path_tracker.remove_path(PointerId::PRIMARY);
        self.captured_targets.remove(&PointerId::PRIMARY);
        self.capture_contexts.remove(&PointerId::PRIMARY);
        self.gesture_target_ids.remove(&PointerId::PRIMARY);

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

            self.dispatch_targets(targets, event, true);
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
        if input_pipeline_debug_enabled() {
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] pointer_scrolled ({:.2},{:.2}) -> {}",
                delta_x, delta_y, result
            );
        }
        result
    }

    fn pointer_scrolled_inner(&mut self, delta_x: f32, delta_y: f32) -> bool {
        self.sync_pending_input_recompositions();
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
        self.sync_pending_input_recompositions();
        let (targets, _) = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Clear tracker and button state
        self.hit_path_tracker.clear();
        self.captured_targets.clear();
        self.capture_contexts.clear();
        self.gesture_target_ids.clear();
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
            run_in_mutable_snapshot(|| cranpose_ui::text_field_focus::dispatch_paste(text))
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
            self.layout_dirty = true;
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
            self.layout_dirty = true;
        }

        handled
    }

    pub fn log_debug_info(&mut self) {
        println!("\n\n");
        println!("════════════════════════════════════════════════════════");
        println!("           DEBUG: CURRENT SCREEN STATE");
        println!("════════════════════════════════════════════════════════");

        if let Some(layout_tree) = self.layout_tree() {
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
    pub fn layout_tree(&mut self) -> Option<&LayoutTree> {
        self.layout_tree = self.snapshot_layout_tree_from_live_nodes();
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
            snapshot_state_stats: debug_snapshot_state_thread_local_stats(),
        }
    }

    #[doc(hidden)]
    pub fn debug_slot_table_groups(&self) -> Vec<(usize, Key, Option<usize>, usize)> {
        self.composition.debug_dump_slot_table_groups()
    }

    #[doc(hidden)]
    pub fn debug_all_slots(&self) -> Vec<(usize, String)> {
        self.composition.debug_dump_all_slots()
    }

    #[doc(hidden)]
    pub fn runtime_handle(&self) -> cranpose_core::RuntimeHandle {
        self.composition.runtime_handle()
    }

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

    fn snapshot_layout_tree_from_live_nodes(&mut self) -> Option<LayoutTree> {
        let root = self.composition.root()?;
        let mut applier = self.composition.applier_mut();
        snapshot_layout_tree_from_root(&mut applier, root)
    }

    pub fn set_semantics_enabled(&mut self, enabled: bool) {
        if self.semantics_enabled == enabled {
            return;
        }
        self.semantics_enabled = enabled;
        if enabled {
            self.layout_dirty = true;
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
        // ═══════════════════════════════════════════════════════════════════════════════
        // SCOPED LAYOUT REPASSES (preferred path for local changes)
        // ═══════════════════════════════════════════════════════════════════════════════
        // Process node-specific layout invalidations (e.g., from scroll).
        // This bubbles dirty flags up from specific nodes WITHOUT invalidating all caches.
        // Result: O(subtree) remeasurement, not O(app).
        let repass_nodes = cranpose_ui::take_layout_repass_nodes();
        let had_repass_nodes = !repass_nodes.is_empty();
        if had_repass_nodes {
            let root = self.composition.root();
            let mut applier = self.composition.applier_mut();
            for node_id in repass_nodes {
                // Bubble measure dirty flags up to root so cache epoch increments.
                // This uses the centralized function in cranpose-core.
                cranpose_core::bubble_measure_dirty(
                    &mut *applier as &mut dyn cranpose_core::Applier,
                    node_id,
                );
                cranpose_core::bubble_layout_dirty(
                    &mut *applier as &mut dyn cranpose_core::Applier,
                    node_id,
                );
            }

            // IMPORTANT: Also mark the actual root as needing measure.
            // The bubble may not reach root if intermediate nodes (e.g., subcomposed slot roots
            // from SubcomposeLayout) have broken parent chains. This ensures the epoch
            // is incremented so SubcomposeLayout re-measures its items.
            if let Some(root) = root {
                if let Ok(node) = applier.get_mut(root) {
                    node.mark_needs_measure();
                }
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

        // Only do global cache invalidation if:
        // 1. Invalidation was requested (flag was set)
        // 2. AND there were no scoped repass nodes (which handle layout more efficiently)
        //
        // If scoped repasses were handled above, they've already marked the tree dirty
        // and bubbled up the hierarchy. We don't need to also invalidate all caches.
        if invalidation_requested && !had_repass_nodes {
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
            self.layout_dirty = true;
        } else if invalidation_requested {
            // Invalidation was requested but scoped repasses already handled it.
            // Just make sure layout_dirty is set.
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
            let tree_needs_layout_check = cranpose_ui::tree_needs_layout(&mut *applier, root)
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
            match cranpose_ui::measure_layout_with_options(
                &mut applier,
                root,
                viewport_size,
                MeasureLayoutOptions {
                    collect_semantics: self.semantics_enabled,
                    build_layout_tree: false,
                },
            ) {
                Ok(measurements) => {
                    self.layout_tree = None;
                    self.semantics_tree = measurements.semantics_tree().cloned();
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

fn snapshot_layout_tree_from_root(applier: &mut MemoryApplier, root: NodeId) -> Option<LayoutTree> {
    snapshot_layout_box_recursive(applier, root, Point::default(), Point::default())
        .ok()
        .flatten()
        .map(LayoutTree::new)
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

fn snapshot_layout_box_recursive(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    parent_origin: Point,
    parent_content_offset: Point,
) -> Result<Option<LayoutBox>, NodeError> {
    let Some(snapshot) = with_live_layout_snapshot(
        applier,
        node_id,
        |node| {
            let state = node.layout_state();
            (
                state,
                node.children.clone(),
                LayoutNodeData::new(
                    node.modifier.clone(),
                    node.resolved_modifiers(),
                    node.modifier_slices_snapshot(),
                    LayoutNodeKind::Layout,
                ),
            )
        },
        |node| {
            let state = node.layout_state();
            (
                state,
                node.active_children(),
                LayoutNodeData::new(
                    node.modifier(),
                    node.resolved_modifiers(),
                    node.modifier_slices_snapshot(),
                    LayoutNodeKind::Subcompose,
                ),
            )
        },
    )?
    else {
        return Ok(None);
    };

    snapshot_layout_box_from_parts(
        applier,
        node_id,
        parent_origin,
        parent_content_offset,
        snapshot,
    )
}

fn with_live_layout_snapshot<T, FLayout, FSubcompose>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    on_layout: FLayout,
    on_subcompose: FSubcompose,
) -> Result<Option<T>, NodeError>
where
    FLayout: FnOnce(&mut LayoutNode) -> T,
    FSubcompose: FnOnce(&mut SubcomposeLayoutNode) -> T,
{
    match applier.with_node::<LayoutNode, _>(node_id, on_layout) {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(NodeError::TypeMismatch { .. }) => {
            match applier.with_node::<SubcomposeLayoutNode, _>(node_id, on_subcompose) {
                Ok(snapshot) => Ok(Some(snapshot)),
                Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => Ok(None),
                Err(err) => Err(err),
            }
        }
        Err(NodeError::Missing { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

fn snapshot_layout_box_from_parts(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    parent_origin: Point,
    parent_content_offset: Point,
    snapshot: (
        cranpose_ui::widgets::LayoutState,
        Vec<NodeId>,
        LayoutNodeData,
    ),
) -> Result<Option<LayoutBox>, NodeError> {
    let (state, children, data) = snapshot;
    if !state.is_placed {
        return Ok(None);
    }

    let origin = Point {
        x: parent_origin.x + parent_content_offset.x + state.position.x,
        y: parent_origin.y + parent_content_offset.y + state.position.y,
    };
    let mut layout_children = Vec::with_capacity(children.len());
    for child_id in children {
        if let Some(child) =
            snapshot_layout_box_recursive(applier, child_id, origin, state.content_offset)?
        {
            layout_children.push(child);
        }
    }

    Ok(Some(LayoutBox::new(
        node_id,
        cranpose_ui::Rect {
            x: origin.x,
            y: origin.y,
            width: state.size.width,
            height: state.size.height,
        },
        state.content_offset,
        data,
        layout_children,
    )))
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
