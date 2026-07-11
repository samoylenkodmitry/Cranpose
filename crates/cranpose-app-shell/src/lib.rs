#![deny(unsafe_code)]
#![allow(clippy::type_complexity)]

mod fps_monitor;
mod hit_path_tracker;
mod shell_debug;
mod shell_frame;
mod shell_input;
#[cfg(test)]
use shell_frame::build_draw_refresh_scope;

pub use fps_monitor::FpsStats;

use std::fmt::{Debug, Write};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, MutexGuard,
};
// Use web_time for cross-platform time support (native + WASM) - compatible with winit
use web_time::Instant;

use cranpose_core::{
    enter_event_handler_scope, location_key, run_in_mutable_snapshot, Applier, Composition, Key,
    MemoryApplier, NodeError, NodeId,
};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_runtime_std::StdRuntime;
use cranpose_ui::{
    clear_transient_scroll_motion_contexts, format_layout_tree, format_render_scene,
    format_screen_summary, has_pending_focus_invalidations, has_pending_pointer_repasses,
    peek_focus_invalidation, peek_layout_invalidation, peek_pointer_invalidation,
    peek_render_invalidation, process_focus_invalidations, process_pointer_repasses,
    request_render_invalidation, take_draw_repass_nodes, take_focus_invalidation,
    take_layout_invalidation, take_pointer_invalidation, take_render_invalidation,
    HeadlessRenderer, LayoutBox, LayoutNode, LayoutTree, MeasureLayoutOptions, SemanticsTree,
    SubcomposeLayoutNode,
};
use cranpose_ui_graphics::{Point, Rect, Size};
use hit_path_tracker::{HitPathTracker, PointerId};
use std::collections::HashSet;

// Re-export key event types for use by cranpose
pub use cranpose_ui::{KeyCode, KeyEvent, KeyEventType, Modifiers};
// Re-export the pointer device source so platform backends can stamp it.
pub use cranpose_foundation::PointerSource;

/// Bridges the in-tree selection menu's clipboard actions to the desktop OS
/// clipboard (`arboard`). Holds a persistent clipboard handle (Linux X11 loses
/// clipboard contents when the last owning handle drops), shared behind an
/// `Rc<RefCell<..>>` so it stays alive for the app-context's lifetime.
#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
struct ShellClipboard {
    inner: std::rc::Rc<std::cell::RefCell<Option<arboard::Clipboard>>>,
}

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl cranpose_ui::clipboard_session::PlatformClipboard for ShellClipboard {
    fn write_text(&self, text: &str) {
        if let Some(clipboard) = self.inner.borrow_mut().as_mut() {
            let _ = clipboard.set_text(text);
        }
    }

    fn read_text(&self) -> Option<String> {
        self.inner
            .borrow_mut()
            .as_mut()
            .and_then(|clipboard| clipboard.get_text().ok())
    }
}
// Re-export the platform soft-keyboard hook so runtimes only depend on the shell
pub use cranpose_ui::PlatformTextInputHandler;
// Re-export the IME editable-state snapshot for platform text-input bridges
pub use cranpose_ui::ImeEditorState;

#[cfg(any(test, feature = "test-support"))]
use cranpose_core::{
    debug_recompose_scope_registry_stats, MemoryApplierDebugStats,
    RecomposeScopeRegistryDebugStats, SlotTableDebugStats,
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
    app_context: Rc<cranpose_ui::AppContext>,
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    content: Box<dyn FnMut()>,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    last_frame_time_nanos: u64,
    layout_tree: Option<LayoutTree>,
    semantics_tree: Option<SemanticsTree>,
    semantics_enabled: bool,
    layout_requested: bool,
    force_layout_pass: bool,
    scene_dirty: bool,
    scoped_layout_scene_nodes: Vec<NodeId>,
    is_dirty: bool,
    /// Tracks which mouse buttons are currently pressed
    buttons_pressed: PointerButtons,
    /// Device source (touch/mouse/stylus) of the most recent pointer sample,
    /// set by the platform before dispatching and stamped onto every
    /// `PointerEvent` the shell constructs. The pointer model is stateful
    /// (`set_cursor` then `pointer_pressed`), so a single per-shell cell mirrors
    /// the existing cursor/buttons state without churning every method signature.
    pointer_source: PointerSource,
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
        feature = "clipboard-native",
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    clipboard: Option<arboard::Clipboard>,
    /// Dev options for debugging and performance monitoring
    dev_options: DevOptions,
    dev_overlay_controls: Vec<DevOverlayControl>,
    dev_overlay_text: String,
    dev_overlay_last_refresh: Option<Instant>,
    dev_overlay_viewport: Option<Size>,
    fps_monitor: fps_monitor::FpsMonitor,
    frame_scheduler: FrameScheduler,
}

fn update_stage_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        std::env::var("CRANPOSE_UPDATE_STAGE_TELEMETRY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
    })
}

#[derive(Clone, Copy, Debug)]
struct UpdateStageTelemetry {
    started_at: Instant,
    after_frame_callbacks: Instant,
    after_ui_drain: Instant,
    after_reconcile: Instant,
    after_process_frame: Instant,
    should_render: bool,
    reconcile_attempted: bool,
    reconcile_changed: bool,
}

fn log_update_stage_telemetry(telemetry: UpdateStageTelemetry) {
    let Some(threshold_ms) = update_stage_telemetry_threshold_ms() else {
        return;
    };
    let total_ms = telemetry
        .after_process_frame
        .duration_since(telemetry.started_at)
        .as_secs_f64()
        * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    let frame_callbacks_ms = telemetry
        .after_frame_callbacks
        .duration_since(telemetry.started_at)
        .as_secs_f64()
        * 1000.0;
    let ui_drain_ms = telemetry
        .after_ui_drain
        .duration_since(telemetry.after_frame_callbacks)
        .as_secs_f64()
        * 1000.0;
    let reconcile_ms = telemetry
        .after_reconcile
        .duration_since(telemetry.after_ui_drain)
        .as_secs_f64()
        * 1000.0;
    let process_frame_ms = telemetry
        .after_process_frame
        .duration_since(telemetry.after_reconcile)
        .as_secs_f64()
        * 1000.0;
    eprintln!(
        "[update-stage-telemetry] total_ms={total_ms:.2} frame_callbacks_ms={frame_callbacks_ms:.2} ui_drain_ms={ui_drain_ms:.2} reconcile_ms={reconcile_ms:.2} process_frame_ms={process_frame_ms:.2} should_render={} reconcile_attempted={} reconcile_changed={}",
        telemetry.should_render,
        telemetry.reconcile_attempted,
        telemetry.reconcile_changed
    );
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FramePacingMode {
    /// Pace frames to the display refresh interval. The production default:
    /// animations advance once per vsync instead of re-rendering uncapped.
    #[default]
    Vsync,
    Hard60,
    Hard120,
    /// Render as fast as possible. For perf harnesses and robot drivers that
    /// measure work throughput; saturates the GPU if used in a real app.
    NoVsync,
}

impl FramePacingMode {
    pub const ALL: [Self; 4] = [Self::Vsync, Self::Hard60, Self::Hard120, Self::NoVsync];

    pub fn label(self) -> &'static str {
        match self {
            Self::Vsync => "VSync",
            Self::Hard60 => "60fps",
            Self::Hard120 => "120fps",
            Self::NoVsync => "NoVSync",
        }
    }

    pub fn target_fps(self) -> Option<u32> {
        match self {
            Self::Hard60 => Some(60),
            Self::Hard120 => Some(120),
            Self::Vsync | Self::NoVsync => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSchedule {
    pub needs_update: bool,
    pub needs_frame: bool,
    pub next_deadline: Option<web_time::Instant>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameUpdateResult {
    pub visual_changed: bool,
    pub structure_changed: bool,
}

pub trait PlatformFrameDriver {
    fn request_frame(&self);
    fn request_wake_at(&self, deadline: web_time::Instant);
    fn clear_wake(&self);
}

#[derive(Debug)]
pub struct FrameScheduler {
    update_pending: AtomicBool,
    frame_pending: AtomicBool,
    next_deadline: Mutex<Option<web_time::Instant>>,
}

impl Default for FrameScheduler {
    fn default() -> Self {
        Self {
            update_pending: AtomicBool::new(false),
            frame_pending: AtomicBool::new(false),
            next_deadline: Mutex::new(None),
        }
    }
}

impl FrameScheduler {
    fn lock_deadline(&self) -> MutexGuard<'_, Option<web_time::Instant>> {
        self.next_deadline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn record(&self, schedule: FrameSchedule) {
        self.update_pending
            .store(schedule.needs_update, Ordering::SeqCst);
        self.frame_pending
            .store(schedule.needs_frame, Ordering::SeqCst);
        let mut next_deadline = self.lock_deadline();
        *next_deadline = if schedule.needs_update {
            None
        } else {
            schedule.next_deadline
        };
    }

    pub fn schedule<D>(&self, schedule: FrameSchedule, driver: &D)
    where
        D: PlatformFrameDriver + ?Sized,
    {
        self.record(schedule);
        schedule.apply_to(driver);
    }

    pub fn snapshot(&self) -> FrameSchedule {
        FrameSchedule {
            needs_update: self.update_pending.load(Ordering::SeqCst),
            needs_frame: self.frame_pending.load(Ordering::SeqCst),
            next_deadline: *self.lock_deadline(),
        }
    }
}

impl FrameSchedule {
    pub fn apply_to<D>(self, driver: &D)
    where
        D: PlatformFrameDriver + ?Sized,
    {
        if self.needs_frame {
            driver.clear_wake();
            driver.request_frame();
        } else if self.needs_update {
            driver.request_wake_at(web_time::Instant::now());
        } else if let Some(deadline) = self.next_deadline {
            driver.request_wake_at(deadline);
        } else {
            driver.clear_wake();
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DevOverlayControl {
    bounds: Rect,
    mode: FramePacingMode,
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
    pub frame_pacing_controls: bool,
    pub frame_pacing_mode: FramePacingMode,
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
    pub fn new(renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        Self::new_with_size(renderer, root_key, content, (800, 600), (800.0, 600.0))
    }

    pub fn new_with_size(
        renderer: R,
        root_key: Key,
        content: impl FnMut() + 'static,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
    ) -> Self {
        Self::new_with_size_and_density(renderer, root_key, content, buffer_size, viewport, 1.0)
    }

    pub fn new_with_size_and_density(
        mut renderer: R,
        root_key: Key,
        content: impl FnMut() + 'static,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
        density: f32,
    ) -> Self {
        let app_context = cranpose_ui::AppContext::new_with_density(density);
        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        // Install the top-level overlay layer once, at the root, so every app
        // gets `Popup`/selection-handle/context-menu support for free and the
        // overlay is composed (and thus painted and hit-tested) last. The app's
        // content is called through a shared handle so this wrapper can be
        // re-created cheaply on every recomposition without moving `content`.
        let app_content = Rc::new(std::cell::RefCell::new(content));
        let mut build: Box<dyn FnMut()> = Box::new(move || {
            let app_content = Rc::clone(&app_content);
            cranpose_ui::widgets::PopupHost(move || {
                (app_content.borrow_mut())();
            });
        });
        renderer.attach_app_context_services(&app_context);
        app_context.enter(|| {
            // Route the selection menu's Copy/Cut/Paste through the OS clipboard
            // on desktop; other platforms fall back to the in-process clipboard.
            #[cfg(all(
                feature = "clipboard-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            {
                let clipboard =
                    std::rc::Rc::new(std::cell::RefCell::new(arboard::Clipboard::new().ok()));
                cranpose_ui::clipboard_session::set_platform_clipboard(std::rc::Rc::new(
                    ShellClipboard { inner: clipboard },
                ));
            }
            if let Err(err) = composition.render_stable(root_key, &mut *build) {
                log::error!("initial render failed: {err}");
            }
        });
        renderer.scene_mut().clear();
        let mut shell = Self {
            app_context,
            runtime,
            composition,
            content: build,
            renderer,
            cursor: (0.0, 0.0),
            viewport,
            buffer_size,
            start_time: Instant::now(),
            last_frame_time_nanos: 0,
            layout_tree: None,
            semantics_tree: None,
            semantics_enabled: false,
            layout_requested: true,
            force_layout_pass: true,
            scene_dirty: true,
            scoped_layout_scene_nodes: Vec::new(),
            is_dirty: true,
            buttons_pressed: PointerButtons::NONE,
            pointer_source: PointerSource::Unknown,
            hit_path_tracker: HitPathTracker::new(),
            hovered_nodes: Vec::new(),
            #[cfg(all(
                feature = "clipboard-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            clipboard: arboard::Clipboard::new().ok(),
            dev_options: DevOptions::default(),
            dev_overlay_controls: Vec::new(),
            dev_overlay_text: String::new(),
            dev_overlay_last_refresh: None,
            dev_overlay_viewport: None,
            fps_monitor: fps_monitor::FpsMonitor::new(),
            frame_scheduler: FrameScheduler::default(),
        };
        shell.process_frame();
        shell
    }

    /// The shell's [`AppContext`](cranpose_ui::AppContext). Platform backends
    /// use it to register per-context services (such as the OS clipboard) that
    /// need UIKit/JNI access the shell itself does not have: enter the context
    /// and call the relevant `set_platform_*` installer.
    pub fn app_context(&self) -> &Rc<cranpose_ui::AppContext> {
        &self.app_context
    }

    /// Set development options for debugging and performance monitoring.
    ///
    /// The FPS counter and other overlays are rendered directly by the renderer
    /// (not via composition) to avoid affecting performance measurements.
    pub fn set_dev_options(&mut self, options: DevOptions) {
        self.dev_options = options;
        self.invalidate_dev_overlay_text();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(request_render_invalidation);
        self.mark_dirty();
    }

    /// Get a reference to the current dev options.
    pub fn dev_options(&self) -> &DevOptions {
        &self.dev_options
    }

    pub fn frame_pacing_mode(&self) -> FramePacingMode {
        self.dev_options.frame_pacing_mode
    }

    pub fn current_fps(&self) -> f32 {
        self.fps_monitor.current_fps()
    }

    pub fn fps_stats(&self) -> FpsStats {
        self.fps_monitor.stats()
    }

    pub fn reset_fps_stats(&mut self) {
        self.fps_monitor.reset_stats();
        self.invalidate_dev_overlay_text();
    }

    pub fn record_presented_frame(
        &mut self,
        frame_started_at: Instant,
        frame_finished_at: Instant,
    ) {
        self.fps_monitor
            .record_frame_work(frame_started_at, frame_finished_at);
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn record_presented_frame_for_test(
        &mut self,
        frame_started_nanos: u64,
        frame_finished_nanos: u64,
    ) {
        let started = self.start_time + std::time::Duration::from_nanos(frame_started_nanos);
        let finished = self.start_time + std::time::Duration::from_nanos(frame_finished_nanos);
        self.record_presented_frame(started, finished);
    }

    pub fn set_frame_pacing_mode(&mut self, mode: FramePacingMode) {
        if self.dev_options.frame_pacing_mode == mode {
            return;
        }
        self.dev_options.frame_pacing_mode = mode;
        self.invalidate_dev_overlay_text();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(request_render_invalidation);
        self.mark_dirty();
    }

    pub fn handle_dev_overlay_click(&mut self, x: f32, y: f32) -> Option<FramePacingMode> {
        if !self.dev_options.frame_pacing_controls {
            return None;
        }
        let mode = self
            .dev_overlay_controls
            .iter()
            .find(|control| control.bounds.contains(x, y))
            .map(|control| control.mode)?;
        self.set_frame_pacing_mode(mode);
        Some(mode)
    }

    fn invalidate_dev_overlay_text(&mut self) {
        self.dev_overlay_text.clear();
        self.dev_overlay_last_refresh = None;
        self.dev_overlay_viewport = None;
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.request_forced_layout_pass();
        self.mark_dirty();
        self.process_frame();
    }

    pub fn viewport_size(&self) -> (f32, f32) {
        self.viewport
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
    pub fn set_frame_waker(&mut self, waker: impl Fn() + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    pub fn clear_frame_waker(&mut self) {
        self.runtime.clear_frame_waker();
    }

    pub fn should_render(&self) -> bool {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| {
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
        })
    }

    fn needs_ui_update_in_context(&self) -> bool {
        if self.is_dirty
            || self.layout_requested
            || self.scene_dirty
            || peek_render_invalidation()
            || peek_pointer_invalidation()
            || peek_focus_invalidation()
            || peek_layout_invalidation()
            || cranpose_ui::has_pending_layout_repasses()
            || cranpose_ui::has_pending_measure_repasses()
            || cranpose_ui::has_pending_draw_repasses()
            || has_pending_pointer_repasses()
            || has_pending_focus_invalidations()
        {
            return true;
        }

        self.composition.should_render()
    }

    pub fn needs_update(&self) -> bool {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.needs_ui_update_in_context())
    }

    /// Returns true if the shell needs to redraw (dirty flag, layout dirty, active animations).
    /// Note: Cursor blink is now timer-based and uses WaitUntil scheduling, not continuous redraw.
    pub fn needs_redraw(&self) -> bool {
        let app_context = Rc::clone(&self.app_context);
        app_context
            .enter(|| self.needs_ui_update_in_context() || self.renderer.needs_frame_warmup())
    }

    /// Marks the shell as dirty, indicating a redraw is needed.
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    pub fn request_root_render(&mut self) {
        self.composition.request_root_render();
        self.request_forced_layout_pass();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(request_render_invalidation);
        self.mark_dirty();
    }

    pub fn set_density(&mut self, density: f32) {
        let app_context = Rc::clone(&self.app_context);
        let changed = app_context.enter(|| {
            let previous = cranpose_ui::current_density().to_bits();
            cranpose_ui::set_density(density);
            previous != cranpose_ui::current_density().to_bits()
        });
        if changed {
            self.request_forced_layout_pass();
            self.mark_dirty();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_current_density(&self) -> f32 {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::current_density)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_enter_app_context<T>(&self, block: impl FnOnce() -> T) -> T {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(block)
    }

    fn request_layout_pass(&mut self) {
        self.layout_requested = true;
    }

    fn request_forced_layout_pass(&mut self) {
        self.layout_requested = true;
        self.force_layout_pass = true;
    }

    fn composition_tree_needs_layout(&mut self) -> bool {
        let Some(root) = self.composition.root() else {
            return true;
        };
        let mut applier = self.composition.applier_mut();
        cranpose_ui::tree_needs_layout(&mut *applier, root).unwrap_or_else(|err| {
            log::warn!(
                "Cannot check layout dirty status for root #{}: {}",
                root,
                err
            );
            true
        })
    }

    /// Returns true if there are active animations or pending recompositions.
    pub fn has_active_animations(&self) -> bool {
        self.composition.should_render()
    }

    pub fn has_active_pointer_gesture(&self) -> bool {
        self.buttons_pressed != PointerButtons::NONE
            && self.hit_path_tracker.has_path(PointerId::PRIMARY)
    }

    /// Returns the next scheduled event time for cursor blink.
    /// Use this for `ControlFlow::WaitUntil` scheduling.
    pub fn next_event_time(&self) -> Option<web_time::Instant> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::next_cursor_blink_time)
    }

    fn compute_frame_schedule(&self) -> FrameSchedule {
        let needs_update = self.needs_update();
        let needs_frame = needs_update
            || self.has_active_animations()
            || self.has_active_pointer_gesture()
            || self.renderer.needs_frame_warmup();
        FrameSchedule {
            needs_update,
            needs_frame,
            next_deadline: self.next_event_time(),
        }
    }

    pub fn frame_schedule(&self) -> FrameSchedule {
        let schedule = self.compute_frame_schedule();
        self.frame_scheduler.record(schedule);
        schedule
    }

    pub fn schedule_platform_frame<D>(&self, driver: &D) -> FrameSchedule
    where
        D: PlatformFrameDriver + ?Sized,
    {
        let schedule = self.compute_frame_schedule();
        self.frame_scheduler.schedule(schedule, driver);
        schedule
    }

    pub fn frame_scheduler_snapshot(&self) -> FrameSchedule {
        self.frame_scheduler.snapshot()
    }

    fn frame_time_nanos_at(&self, now: Instant) -> u64 {
        now.checked_duration_since(self.start_time)
            .unwrap_or_default()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64
    }

    pub fn update_after_frame_interval(
        &mut self,
        frame_interval: std::time::Duration,
    ) -> FrameUpdateResult {
        let wall_frame_time = self.frame_time_nanos_at(Instant::now());
        let base_frame_time = self.last_frame_time_nanos.max(wall_frame_time);
        let frame_time = base_frame_time
            .saturating_add(frame_interval.as_nanos().min(u128::from(u64::MAX)) as u64);
        self.update_at_frame_time_nanos(frame_time)
    }

    /// Advance the frame clock by EXACTLY `frame_interval` past the last
    /// frame — no wall anchoring — and run one update there. Robot keyframe
    /// captures ride this to sample animations deterministically: while the
    /// advanced clock is ahead of wall time, interleaved wall-clocked
    /// updates clamp to it (dt 0) instead of fast-forwarding animations.
    pub fn update_after_exact_interval(
        &mut self,
        frame_interval: std::time::Duration,
    ) -> FrameUpdateResult {
        let frame_time = self
            .last_frame_time_nanos
            .saturating_add(frame_interval.as_nanos().min(u128::from(u64::MAX)) as u64);
        self.update_at_frame_time_nanos(frame_time)
    }

    pub fn update_at_frame_time_nanos(&mut self, frame_time: u64) -> FrameUpdateResult {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| {
            let update_started_at = Instant::now();
            let frame_time = frame_time.max(self.last_frame_time_nanos);
            self.last_frame_time_nanos = frame_time;
            let runtime_handle = self.runtime.runtime_handle();
            runtime_handle.with_deferred_state_releases(|| {
                self.runtime.drain_frame_callbacks(frame_time);
                let after_frame_callbacks = Instant::now();
                runtime_handle.drain_ui();
                let after_ui_drain = Instant::now();
                let should_render = self.composition.should_render();
                let mut reconcile_attempted = false;
                let mut reconcile_changed = false;
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
                        let result = self.process_frame_in_context(reconcile_changed);
                        let after_process_frame = Instant::now();
                        log_update_stage_telemetry(UpdateStageTelemetry {
                            started_at: update_started_at,
                            after_frame_callbacks,
                            after_ui_drain,
                            after_reconcile: after_ui_drain,
                            after_process_frame,
                            should_render,
                            reconcile_attempted,
                            reconcile_changed,
                        });
                        self.is_dirty = false;
                        return result;
                    };
                    reconcile_attempted = true;
                    match self.composition.reconcile(root_key, &mut *self.content) {
                        Ok(changed) => {
                            reconcile_changed = changed;
                            log::trace!(
                                target: "cranpose::input",
                                "reconcile changed={changed}"
                            );
                            if changed {
                                self.fps_monitor.record_recomposition();
                                if self.composition_tree_needs_layout() {
                                    self.request_layout_pass();
                                }
                                request_render_invalidation();
                            }
                        }
                        Err(NodeError::Missing { id }) => {
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
                let after_reconcile = Instant::now();
                let result = self.process_frame_in_context(reconcile_changed);
                let after_process_frame = Instant::now();
                log_update_stage_telemetry(UpdateStageTelemetry {
                    started_at: update_started_at,
                    after_frame_callbacks,
                    after_ui_drain,
                    after_reconcile,
                    after_process_frame,
                    should_render,
                    reconcile_attempted,
                    reconcile_changed,
                });
                self.is_dirty = false;
                result
            })
        })
    }

    pub fn update(&mut self) -> FrameUpdateResult {
        let frame_time = self.frame_time_nanos_at(Instant::now());
        self.update_at_frame_time_nanos(frame_time)
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
mod frame_pacing_tests {
    use super::{FramePacingMode, FrameSchedule, FrameScheduler, PlatformFrameDriver};
    use std::cell::RefCell;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::Duration;
    use web_time::Instant;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum DriverCall {
        RequestFrame,
        RequestWakeAt(Instant),
        ClearWake,
    }

    #[derive(Default)]
    struct RecordingFrameDriver {
        calls: RefCell<Vec<DriverCall>>,
    }

    impl RecordingFrameDriver {
        fn calls(&self) -> Vec<DriverCall> {
            self.calls.borrow().clone()
        }
    }

    impl PlatformFrameDriver for RecordingFrameDriver {
        fn request_frame(&self) {
            self.calls.borrow_mut().push(DriverCall::RequestFrame);
        }

        fn request_wake_at(&self, deadline: Instant) {
            self.calls
                .borrow_mut()
                .push(DriverCall::RequestWakeAt(deadline));
        }

        fn clear_wake(&self) {
            self.calls.borrow_mut().push(DriverCall::ClearWake);
        }
    }

    #[test]
    fn frame_pacing_labels_match_overlay_modes() {
        assert_eq!(FramePacingMode::Vsync.label(), "VSync");
        assert_eq!(FramePacingMode::Hard60.label(), "60fps");
        assert_eq!(FramePacingMode::Hard120.label(), "120fps");
        assert_eq!(FramePacingMode::NoVsync.label(), "NoVSync");
    }

    #[test]
    fn only_hard_modes_have_fixed_targets() {
        assert_eq!(FramePacingMode::Vsync.target_fps(), None);
        assert_eq!(FramePacingMode::Hard60.target_fps(), Some(60));
        assert_eq!(FramePacingMode::Hard120.target_fps(), Some(120));
        assert_eq!(FramePacingMode::NoVsync.target_fps(), None);
    }

    #[test]
    fn frame_schedule_requests_immediate_frame_and_clears_deadline() {
        let driver = RecordingFrameDriver::default();
        let deadline = Instant::now() + Duration::from_millis(25);

        FrameSchedule {
            needs_update: true,
            needs_frame: true,
            next_deadline: Some(deadline),
        }
        .apply_to(&driver);

        assert_eq!(
            driver.calls(),
            vec![DriverCall::ClearWake, DriverCall::RequestFrame]
        );
    }

    #[test]
    fn frame_schedule_requests_deadline_when_idle_until_timer() {
        let driver = RecordingFrameDriver::default();
        let deadline = Instant::now() + Duration::from_millis(25);

        FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: Some(deadline),
        }
        .apply_to(&driver);

        assert_eq!(driver.calls(), vec![DriverCall::RequestWakeAt(deadline)]);
    }

    #[test]
    fn frame_schedule_wakes_without_requesting_frame_for_update_only_work() {
        let driver = RecordingFrameDriver::default();
        let before = Instant::now();

        FrameSchedule {
            needs_update: true,
            needs_frame: false,
            next_deadline: None,
        }
        .apply_to(&driver);

        let calls = driver.calls();
        assert_eq!(calls.len(), 1);
        match calls[0] {
            DriverCall::RequestWakeAt(deadline) => {
                assert!(deadline >= before);
            }
            other => panic!("update-only work must wake without requesting a frame: {other:?}"),
        }
    }

    #[test]
    fn frame_schedule_clears_wake_when_fully_idle() {
        let driver = RecordingFrameDriver::default();

        FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: None,
        }
        .apply_to(&driver);

        assert_eq!(driver.calls(), vec![DriverCall::ClearWake]);
    }

    #[test]
    fn frame_scheduler_records_latest_schedule_and_applies_driver() {
        let scheduler = FrameScheduler::default();
        let driver = RecordingFrameDriver::default();
        let deadline = Instant::now() + Duration::from_millis(25);

        scheduler.schedule(
            FrameSchedule {
                needs_update: false,
                needs_frame: false,
                next_deadline: Some(deadline),
            },
            &driver,
        );

        assert_eq!(
            scheduler.snapshot(),
            FrameSchedule {
                needs_update: false,
                needs_frame: false,
                next_deadline: Some(deadline),
            }
        );
        assert_eq!(driver.calls(), vec![DriverCall::RequestWakeAt(deadline)]);
    }

    #[test]
    fn frame_scheduler_clears_deadline_for_immediate_frame() {
        let scheduler = FrameScheduler::default();
        let driver = RecordingFrameDriver::default();
        let deadline = Instant::now() + Duration::from_millis(25);

        scheduler.schedule(
            FrameSchedule {
                needs_update: true,
                needs_frame: true,
                next_deadline: Some(deadline),
            },
            &driver,
        );

        assert_eq!(
            scheduler.snapshot(),
            FrameSchedule {
                needs_update: true,
                needs_frame: true,
                next_deadline: None,
            }
        );
        assert_eq!(
            driver.calls(),
            vec![DriverCall::ClearWake, DriverCall::RequestFrame]
        );
    }

    #[test]
    fn frame_scheduler_recovers_poisoned_deadline_lock() {
        let scheduler = FrameScheduler::default();
        let deadline = Instant::now() + Duration::from_millis(25);

        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = scheduler.lock_deadline();
            panic!("poison frame scheduler deadline lock");
        }));

        scheduler.record(FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: Some(deadline),
        });

        assert_eq!(
            scheduler.snapshot(),
            FrameSchedule {
                needs_update: false,
                needs_frame: false,
                next_deadline: Some(deadline),
            }
        );
    }
}

#[cfg(test)]
#[path = "tests/app_shell_tests.rs"]
mod tests;
