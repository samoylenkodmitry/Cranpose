#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod focus_reveal;
mod fps_monitor;
mod frame_rate_boost;
mod hit_path_tracker;
pub mod inspector;
mod modal_focus;
#[cfg(debug_assertions)]
mod semantics_check;
mod shell_debug;
mod shell_frame;
mod shell_input;
mod surface;
mod touch_router;
mod wheel;
use std::{
    cell::{Cell, RefCell},
    fmt::{Debug, Write},
    rc::Rc,
};

use cranpose_core::{
    Applier, Composition, Key, MemoryApplier, NodeError, NodeId, NodeMarks,
    SceneNodeAttachmentScratch, collections::map::HashSet, enter_event_handler_scope, location_key,
    run_in_mutable_snapshot,
};
pub use cranpose_foundation::{
    DEFAULT_ROTARY_SCROLL_FACTOR_DP, Modifiers, PointerSource, RotaryScrollEvent,
    rotary_scroll_pixels_from_detents,
};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer, SceneUpdates};
use cranpose_runtime_std::StdRuntime;
use cranpose_ui::{
    HeadlessRenderer, LayoutBox, LayoutNode, LayoutTree, MeasureLayoutOptions, SemanticsTree,
    SubcomposeLayoutNode, WindowRootEntry, format_layout_tree, format_render_scene,
    format_screen_summary, process_focus_invalidations, process_pointer_repasses,
    process_semantics_invalidations, request_render_invalidation,
};
pub use cranpose_ui::{KeyCode, KeyEvent, KeyEventType};
use cranpose_ui_graphics::{Point, PointerIcon, Rect, Size};
pub use fps_monitor::FpsStats;
pub use frame_rate_boost::FrameRateBoost;
use hit_path_tracker::PointerId;
#[cfg(test)]
use shell_frame::build_draw_refresh_scope;
pub use surface::{RootId, RootSurface, SurfaceMut};
use surface::{
    SurfaceDirtyLane, TextInputRouter, TextInputRoutes, route_geometry_scene_nodes,
    route_nodes_by_surface,
};
pub use touch_router::TouchRouter;
use web_time::Instant;
pub use wheel::WheelScroll;

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
#[derive(Default)]
struct ShellClipboard {
    /// The system clipboard, opened at the first copy or paste: opened at
    /// launch, it held the main thread ~0.8 ms on an Apple M5, for apps that
    /// never touch it too.
    inner: std::cell::OnceCell<RefCell<Option<arboard::Clipboard>>>,
}

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl ShellClipboard {
    /// `use_clipboard`'s answer for the system clipboard, opened on first
    /// use; `None` when it cannot be opened.
    fn with<T>(
        &self,
        use_clipboard: impl FnOnce(&mut arboard::Clipboard) -> Option<T>,
    ) -> Option<T> {
        self.inner
            .get_or_init(|| RefCell::new(arboard::Clipboard::new().ok()))
            .borrow_mut()
            .as_mut()
            .and_then(use_clipboard)
    }
}

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl cranpose_ui::clipboard_session::PlatformClipboard for ShellClipboard {
    fn write_text(&self, text: &str) {
        self.with(|clipboard| clipboard.set_text(text).ok());
    }

    fn read_text(&self) -> Option<String> {
        self.with(|clipboard| clipboard.get_text().ok())
    }
}
#[cfg(any(test, feature = "test-support"))]
use cranpose_core::{
    CompositionPassDebugStats, SlotId,
    runtime::{RuntimeDebugStats, StateArenaDebugStats},
    snapshot_pinning::{SnapshotPinningDebugStats, debug_snapshot_pinning_stats},
    snapshot_v2::{SnapshotV2DebugStats, debug_snapshot_v2_stats},
};
#[cfg(any(test, feature = "test-support"))]
use cranpose_core::{
    MemoryApplierDebugStats, RecomposeScopeRegistryDebugStats, SlotTableDebugStats,
    debug_recompose_scope_registry_stats,
};
pub use cranpose_ui::{ImeEditorState, PlatformTextInputHandler};
#[cfg(any(test, feature = "test-support"))]
pub mod accessibility_audit;
#[cfg(any(test, feature = "test-support"))]
pub mod placed_semantics;

/// How the platform should vote the display's frame rate on behalf of the app.
///
/// Compose apps get 120 Hz gameplay on a 120 Hz panel not by presenting faster
/// but because HWUI votes a rate on the window while gestures run, content
/// moves or the window redraws continuously, and clears it when they stop. A window that never votes is pinned by
/// SurfaceFlinger's cadence inference instead — which also throttles the app's
/// choreographer, so the inference reinforces itself. `Auto` reproduces the
/// HWUI behaviour; the platform backends read it every frame and vote through
/// the native window when the desired rate changes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FrameRatePreference {
    /// Ask for the panel's fastest rate while [`FrameRateBoost`] holds, a
    /// quiet rate while frames arrive only intermittently, no preference
    /// when the scene is still. This is the default, matching what Compose/HWUI do for every
    /// app without the app's involvement.
    #[default]
    Auto,
    /// Never vote; the OS infers a rate from presentation cadence.
    NoPreference,
    /// Always vote exactly this rate in Hz. Values `<= 0` behave like
    /// [`FrameRatePreference::NoPreference`].
    Exact(f32),
}

impl FrameRatePreference {
    /// The baseline `Auto` votes while frames arrive without a boost — the
    /// same rate HWUI's NORMAL frame-rate category resolves to on phone
    /// panels. The quiet vote cannot simply be "no vote": SurfaceFlinger
    /// infers a non-voting window's rate from whatever cadence it last
    /// observed and pins it, so an app that ever ran the panel's fast rate
    /// would stay there forever (measured on a Pixel 9 Pro, both directions).
    pub const AUTO_QUIET_RATE_HZ: f32 = 60.0;

    /// The rate the platform should vote right now, in Hz, where `0.0` means
    /// "clear the vote". `producing_frames` is whether the frame loop has a
    /// frame scheduled, `boosted` what [`FrameRateBoost::boosted`] says, and
    /// `panel_max_hz` the display's fastest supported rate when the platform
    /// knows it.
    ///
    /// While boosted, `Auto` holds the boost even through moments with no
    /// frame scheduled: a gesture sequence crosses still screens (a tap lands,
    /// the old scene stops animating, the new one hasn't started), and letting
    /// each of those instantly clear the vote flapped the display between the
    /// boost rate and no-vote several times per second on device. This mirrors
    /// SurfaceFlinger's own touch boost, which also outlives the touch by
    /// seconds regardless of what the app presents in between.
    pub fn desired_rate_hz(
        self,
        producing_frames: bool,
        boosted: bool,
        panel_max_hz: Option<f32>,
    ) -> f32 {
        match self {
            Self::Auto => {
                if boosted {
                    panel_max_hz
                        .filter(|rate| *rate > 0.0)
                        .unwrap_or(Self::AUTO_QUIET_RATE_HZ)
                } else if producing_frames {
                    Self::AUTO_QUIET_RATE_HZ
                } else {
                    0.0
                }
            }
            Self::NoPreference => 0.0,
            Self::Exact(rate) if rate > 0.0 => rate,
            Self::Exact(_) => 0.0,
        }
    }
}

pub(crate) struct ShellApp {
    pub(crate) app_context: Rc<cranpose_ui::AppContext>,
    pub(crate) runtime: StdRuntime,
    pub(crate) composition: Composition<MemoryApplier>,
    pub(crate) content: Box<dyn FnMut()>,
    pub(crate) start_time: Instant,
    pub(crate) last_frame_time_nanos: u64,
    pub(crate) semantics_enabled: bool,
    pub(crate) semantics_snapshot_revision: u64,
    pub(crate) revealed_focus: Option<NodeId>,
    pub(crate) layout_requested: bool,
    pub(crate) content_moved: bool,
    pub(crate) force_layout_pass: bool,
    pub(crate) modifiers: Option<Modifiers>,
    pub(crate) rotary_scroll_factor: f32,
    #[cfg(all(feature = "clipboard-native", target_os = "linux"))]
    pub(crate) clipboard: Option<arboard::Clipboard>,
    pub(crate) dev_options: DevOptions,
    pub(crate) inspector_projector: Option<inspector::InspectorProjector>,
    pub(crate) fps_monitor: fps_monitor::FpsMonitor,
    pub(crate) text_input_routes: Rc<RefCell<TextInputRoutes>>,
    pub(crate) text_input_router_installed: bool,
    pub(crate) window_roots_seen: Option<u64>,
    /// Frames with a rebuilt scene since draw observations were last pruned.
    pub(crate) rebuilt_frames_since_observation_prune: u32,
    /// The installed-font generation the renderers last took in.
    #[cfg(feature = "installed-fonts")]
    pub(crate) installed_fonts: usize,
}

/// The application: one runtime, one composition, and a surface per window
/// it shows.
///
/// Methods that name no root act on the primary surface, so a platform with
/// one window uses the shell as it always has. A platform showing more
/// windows reaches the others through [`AppShell::surface`], after giving
/// each window root a renderer with [`AppShell::add_window_surface`].
pub struct AppShell<R>
where
    R: Renderer,
{
    pub(crate) app: ShellApp,
    pub(crate) surfaces: Vec<RootSurface<R>>,
    pub(crate) routing_scratch: SurfaceRoutingScratch,
    pub(crate) pending_dirty_nodes: Vec<NodeId>,
    pub(crate) pending_layer_property_nodes: Vec<NodeId>,
    pub(crate) geometry_scene_nodes: cranpose_ui::GeometrySceneNodes,
}

#[derive(Default)]
pub(crate) struct SurfaceRoutingScratch {
    pub(crate) attached: Vec<Option<NodeId>>,
    pub(crate) owners: Vec<Option<NodeId>>,
    pub(crate) seen: HashSet<(usize, NodeId)>,
    pub(crate) attachment: SceneNodeAttachmentScratch,
    pub(crate) window_roots: cranpose_ui::WindowRootRoutingScratch,
    /// The nodes a layout lane routing gave the surfaces, while the moved
    /// lane after it is routed.
    pub(crate) laid_out: NodeMarks,
    /// Each surface's count of layout lane nodes before that routing.
    pub(crate) laid_out_starts: Vec<usize>,
    /// The moved nodes the moved lane routes.
    pub(crate) moved: Vec<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Platform and animation-clock timestamps for one pointer sample.
pub struct PointerEventTime {
    /// Timestamp supplied by the platform, in its millisecond clock domain.
    pub platform_time_ms: Option<i64>,
    /// Timestamp in the animation frame-clock domain.
    pub animation_time_nanos: u64,
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

/// The stages of an update that its telemetry times: start, frame
/// callbacks, UI drain, reconcile and frame processing.
type UpdateStages = cranpose_core::StageTimes<5>;

struct UpdateOutcome {
    should_render: bool,
    reconcile_attempted: bool,
    reconcile_changed: bool,
}

fn log_update_stage_telemetry(stages: &UpdateStages, outcome: UpdateOutcome) {
    if let Some(threshold_ms) = update_stage_telemetry_threshold_ms() {
        report_update_stages(threshold_ms, stages, outcome);
    }
}

/// Prints the stage times of an update that took `threshold_ms` or longer.
/// Out of line, as the telemetry is off unless asked for.
#[inline(never)]
fn report_update_stages(threshold_ms: f64, stages: &UpdateStages, outcome: UpdateOutcome) {
    let Some(
        [
            started_at,
            after_frame_callbacks,
            after_ui_drain,
            after_reconcile,
            after_process_frame,
        ],
    ) = stages.times()
    else {
        return;
    };
    let ms = cranpose_core::stage_ms;
    let total_ms = ms(started_at, after_process_frame);
    if total_ms < threshold_ms {
        return;
    }
    eprintln!(
        "[update-stage-telemetry] total_ms={total_ms:.2} frame_callbacks_ms={:.2} ui_drain_ms={:.2} reconcile_ms={:.2} process_frame_ms={:.2} should_render={} reconcile_attempted={} reconcile_changed={}",
        ms(started_at, after_frame_callbacks),
        ms(after_frame_callbacks, after_ui_drain),
        ms(after_ui_drain, after_reconcile),
        ms(after_reconcile, after_process_frame),
        outcome.should_render,
        outcome.reconcile_attempted,
        outcome.reconcile_changed
    );
}

/// How the shell paces its frames against the display.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FramePacingMode {
    /// Pace frames to the display refresh interval. The production default:
    /// animations advance once per vsync instead of re-rendering uncapped.
    #[default]
    Vsync,
    /// Cap frames at 60 per second with a timer, whatever the display refresh rate is.
    Hard60,
    /// Cap frames at 120 per second with a timer, whatever the display refresh rate is.
    Hard120,
    /// Render as fast as possible. For perf harnesses and robot drivers that
    /// measure work throughput; saturates the GPU if used in a real app.
    NoVsync,
}

impl FramePacingMode {
    /// Every pacing mode, in the order the dev overlay lists them.
    pub const ALL: [Self; 4] = [Self::Vsync, Self::Hard60, Self::Hard120, Self::NoVsync];

    /// Returns the short name the dev overlay shows for this mode.
    pub fn label(self) -> &'static str {
        match self {
            Self::Vsync => "VSync",
            Self::Hard60 => "60fps",
            Self::Hard120 => "120fps",
            Self::NoVsync => "NoVSync",
        }
    }

    /// Returns the frames per second this mode caps at with a timer, or `None` when the mode sets
    /// no cap.
    pub fn target_fps(self) -> Option<u32> {
        match self {
            Self::Hard60 => Some(60),
            Self::Hard120 => Some(120),
            Self::Vsync | Self::NoVsync => None,
        }
    }
}

/// What the shell needs from the platform loop next: an update now, a display frame, or a wake at a
/// later time. The default needs nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameSchedule {
    /// Whether queued UI tasks, state changes, layout or semantics work can run now, without
    /// waiting for a display frame.
    pub needs_update: bool,
    /// Whether the shell wants a display frame because something on screen changed or is animating.
    pub needs_frame: bool,
    /// When the next timed event, such as a cursor blink, is due. `None` when no timed event is
    /// waiting.
    pub next_deadline: Option<Instant>,
}

/// What one shell update changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameUpdateResult {
    /// Whether the update changed what the surface draws, so a new frame needs to be presented.
    pub visual_changed: bool,
    /// Whether the update rebuilt the scene because layout or tree structure changed, not only
    /// drawing.
    pub structure_changed: bool,
    /// Whether this frame's layout moved or resized a node: content in
    /// motion, such as a scroll or a size animation. Compose votes its high
    /// frame-rate category on exactly these changes, so the platforms boost
    /// the display rate on them the way they do for input.
    pub content_moved: bool,
    /// Whether this frame drew new content rather than presenting the scene
    /// it had: something drew, moved or changed, so the scene was rebuilt.
    pub content_redrawn: bool,
}

/// The platform's way of waking its loop, used by [`FrameScheduler`] to carry out a
/// [`FrameSchedule`].
pub trait PlatformFrameDriver {
    /// Asks the platform to deliver a display frame.
    fn request_frame(&self);
    /// Asks the platform to wake its loop at `deadline`. A deadline of the current time means wake
    /// as soon as possible.
    fn request_wake_at(&self, deadline: Instant);
    /// Cancels the wake requested by `request_wake_at`.
    fn clear_wake(&self);
}

/// Stores the latest [`FrameSchedule`] of a surface for the platform code that drives it. A
/// surface lives on its shell's thread, so the schedule sits in a `Cell`: on ESP-IDF a `Mutex`
/// is a FreeRTOS mutex, which every frame took.
#[derive(Debug, Default)]
pub struct FrameScheduler {
    latest: Cell<FrameSchedule>,
}

impl FrameScheduler {
    /// Stores `schedule` as the latest one. A schedule that needs an update drops its deadline.
    pub fn record(&self, schedule: FrameSchedule) {
        self.latest.set(FrameSchedule {
            next_deadline: if schedule.needs_update {
                None
            } else {
                schedule.next_deadline
            },
            ..schedule
        });
    }

    /// Stores `schedule` and asks `driver` for the frame or wake it calls for.
    pub fn schedule<D>(&self, schedule: FrameSchedule, driver: &D)
    where
        D: PlatformFrameDriver + ?Sized,
    {
        self.record(schedule);
        schedule.apply_to(driver);
    }

    /// Returns the latest stored schedule.
    pub fn snapshot(&self) -> FrameSchedule {
        self.latest.get()
    }
}

impl FrameSchedule {
    /// Whether the shell asks for nothing more: no frame, no update and no timed event.
    pub fn is_idle(self) -> bool {
        !self.needs_frame && !self.needs_update && self.next_deadline.is_none()
    }

    /// Asks `driver` for what this schedule calls for: a frame, an immediate wake, a wake at the
    /// deadline, or no wake.
    pub fn apply_to<D>(self, driver: &D)
    where
        D: PlatformFrameDriver + ?Sized,
    {
        if self.needs_frame {
            driver.clear_wake();
            driver.request_frame();
        } else if self.needs_update {
            driver.request_wake_at(Instant::now());
        } else if let Some(deadline) = self.next_deadline {
            driver.request_wake_at(deadline);
        } else {
            driver.clear_wake();
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DevOverlayControl {
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
    /// Show buttons in the dev overlay that switch the frame pacing mode.
    pub frame_pacing_controls: bool,
    /// How the shell paces its frames.
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
    pub runtime_stats: RuntimeDebugStats,
    pub state_arena_stats: StateArenaDebugStats,
    pub recompose_scope_stats: RecomposeScopeRegistryDebugStats,
    pub snapshot_v2_stats: SnapshotV2DebugStats,
    pub snapshot_pinning_stats: SnapshotPinningDebugStats,
}

impl ShellApp {
    pub(crate) fn flush_semantics_invalidations(&mut self) {
        if self.app_context.has_pending_semantics_invalidations() {
            self.bubble_semantics_invalidations();
        }
    }

    /// Marks the semantics of the queued nodes and their ancestors dirty.
    /// Out of line, as a frame seldom has any.
    #[inline(never)]
    fn bubble_semantics_invalidations(&mut self) {
        let mut applier = self.composition.applier_mut();
        process_semantics_invalidations(|node_id| {
            cranpose_core::bubble_semantics_dirty(&mut *applier, node_id);
        });
    }

    pub(crate) fn request_layout_pass(&mut self) {
        self.layout_requested = true;
    }

    pub(crate) fn request_forced_layout_pass(&mut self) {
        self.layout_requested = true;
        self.force_layout_pass = true;
    }

    pub(crate) fn composition_tree_needs_layout(&mut self) -> bool {
        let Some(root) = self.composition.root() else {
            return true;
        };
        let mut applier = self.composition.applier_mut();
        cranpose_ui::tree_needs_layout(&mut *applier, root).unwrap_or_else(|err| {
            log::warn!("Cannot check layout dirty status for root #{root}: {err}");
            true
        })
    }

    pub(crate) fn has_stale_work_in_context(&self) -> bool {
        self.layout_requested || self.app_context.has_stale_work()
    }

    pub(crate) fn wants_frame_in_context(&self) -> bool {
        self.layout_requested
            || self.app_context.has_pending_frame_work()
            || self.composition.should_render()
    }

    pub(crate) fn needs_ui_update_in_context(&self, surfaces_dirty: bool) -> bool {
        surfaces_dirty
            || self.has_stale_work_in_context()
            || self.composition.runtime().has_pending_ui()
            || self.app_context.has_pending_semantics_invalidations()
            || self.composition.should_render()
    }

    pub(crate) fn next_event_time(&self) -> Option<Instant> {
        let blink = self.app_context.enter(cranpose_ui::next_cursor_blink_time);
        match (blink, cranpose_core::concurrency::next_alarm_deadline()) {
            (Some(blink), Some(alarm)) => Some(blink.min(alarm)),
            (blink, alarm) => blink.or(alarm),
        }
    }

    pub(crate) fn frame_time_nanos_at(&self, now: Instant) -> u64 {
        now.checked_duration_since(self.start_time)
            .unwrap_or_default()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64
    }

    pub(crate) fn realtime_pointer_event_time(
        &self,
        platform_time_ms: Option<i64>,
    ) -> PointerEventTime {
        PointerEventTime {
            platform_time_ms,
            animation_time_nanos: self
                .frame_time_nanos_at(Instant::now())
                .max(self.last_frame_time_nanos),
        }
    }

    pub(crate) fn install_text_input_router(&mut self) {
        if self.text_input_router_installed {
            return;
        }
        let router = Rc::new(TextInputRouter {
            routes: Rc::clone(&self.text_input_routes),
        });
        self.app_context
            .enter(|| cranpose_ui::text_input_session::set_platform_text_input_handler(router));
        self.text_input_router_installed = true;
    }
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Creates a shell that composes `content` under `root_key`, with an 800 by 600 logical pixel
    /// viewport and a buffer of the same size in pixels at density 1.0.
    pub fn new(renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        Self::new_with_size(renderer, root_key, content, (800, 600), (800.0, 600.0))
    }

    /// Creates a shell that composes `content` under `root_key`, with a `buffer_size` in pixels and
    /// a `viewport` in logical pixels at density 1.0.
    pub fn new_with_size(
        renderer: R,
        root_key: Key,
        content: impl FnMut() + 'static,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
    ) -> Self {
        Self::new_with_size_and_density(renderer, root_key, content, buffer_size, viewport, 1.0)
    }

    /// A shell that lays out `viewport` at `density` and gives its primary
    /// renderer the same density as its root scale, for `buffer_size` pixels.
    pub fn new_with_size_and_density(
        mut renderer: R,
        root_key: Key,
        content: impl FnMut() + 'static,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
        density: f32,
    ) -> Self {
        renderer.set_root_scale(density);
        let app_context = cranpose_ui::AppContext::new_with_density(density);
        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let app_content = Rc::new(RefCell::new(content));
        let mut build: Box<dyn FnMut()> = Box::new(move || {
            let app_content = Rc::clone(&app_content);
            cranpose_ui::density::ProvideDensity(cranpose_ui::Density::from_host(), move || {
                cranpose_ui::widgets::PopupHost(move || {
                    (app_content.borrow_mut())();
                });
            });
        });
        renderer.attach_app_context_services(&app_context);
        app_context.enter(|| {
            #[cfg(all(
                feature = "clipboard-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            {
                cranpose_ui::clipboard_session::set_platform_clipboard(Rc::new(
                    ShellClipboard::default(),
                ));
            }
            if let Err(err) = composition.render_stable(root_key, &mut *build) {
                log::error!("initial render failed: {err}");
            }
        });
        renderer.scene_mut().clear();
        let app = ShellApp {
            app_context,
            runtime,
            composition,
            content: build,
            start_time: Instant::now(),
            last_frame_time_nanos: 0,
            semantics_enabled: false,
            semantics_snapshot_revision: 0,
            revealed_focus: None,
            layout_requested: true,
            content_moved: false,
            force_layout_pass: true,
            modifiers: None,
            rotary_scroll_factor: DEFAULT_ROTARY_SCROLL_FACTOR_DP,
            #[cfg(all(feature = "clipboard-native", target_os = "linux"))]
            clipboard: arboard::Clipboard::new().ok(),
            dev_options: DevOptions::default(),
            inspector_projector: None,
            fps_monitor: fps_monitor::FpsMonitor::new(),
            text_input_routes: Rc::new(RefCell::new(TextInputRoutes::default())),
            text_input_router_installed: false,
            window_roots_seen: None,
            rebuilt_frames_since_observation_prune: 0,
            #[cfg(feature = "installed-fonts")]
            installed_fonts: 0,
        };
        let mut shell = Self {
            app,
            surfaces: vec![RootSurface::new(
                RootId::Primary,
                renderer,
                buffer_size,
                viewport,
            )],
            routing_scratch: SurfaceRoutingScratch::default(),
            pending_dirty_nodes: Vec::new(),
            pending_layer_property_nodes: Vec::new(),
            geometry_scene_nodes: cranpose_ui::GeometrySceneNodes::default(),
        };
        shell.process_frame();
        shell
    }

    /// The shell's [`AppContext`](cranpose_ui::AppContext). Platform backends
    /// use it to register per-context services (such as the OS clipboard) that
    /// need UIKit/JNI access the shell itself does not have: enter the context
    /// and call the relevant `set_platform_*` installer.
    pub fn app_context(&self) -> &Rc<cranpose_ui::AppContext> {
        &self.app.app_context
    }

    /// The surface drawing `root`, or `None` when no surface was added for
    /// it. The primary surface always exists.
    pub fn surface(&mut self, root: RootId) -> Option<SurfaceMut<'_, R>> {
        let index = self.surface_index(root)?;
        Some(SurfaceMut::new(self, index))
    }

    /// The surface drawing the composition root.
    pub fn primary(&mut self) -> SurfaceMut<'_, R> {
        SurfaceMut::new(self, 0)
    }

    /// The root whose surface now draws the node that took the primary
    /// press, or `None` while no press is being held.
    ///
    /// A press is taken by a node, not by a rectangle. When the application
    /// moves that node into a window of its own — a tab pulled out of a
    /// strip, a pane pulled off a stack — the press belongs to that window
    /// from then on, and a platform can hand it over on the strength of this
    /// rather than on where the pointer happens to be.
    pub fn root_holding_the_press(&mut self) -> Option<RootId> {
        let pressed = self
            .surfaces
            .iter()
            .find_map(|surface| surface.hit_path_tracker.dispatch_order(PointerId::PRIMARY))?;
        let primary_root = self.app.composition.root()?;
        let mut applier = self.app.composition.applier_mut();
        let holder = pressed.into_iter().find_map(|node| {
            let node = applier.scene_node_attached_to(node, primary_root)?;
            Some(cranpose_ui::nearest_window_root(&mut applier, node))
        })?;
        drop(applier);
        self.surfaces
            .iter()
            .find(|surface| surface.owns_nodes_under(holder))
            .map(|surface| surface.id)
    }

    fn surface_index(&self, root: RootId) -> Option<usize> {
        self.surfaces.iter().position(|surface| surface.id == root)
    }

    /// Gives the window root registered under `window` a surface of its
    /// own, drawn by `renderer` into a framebuffer of `buffer_size` physical
    /// pixels and `viewport` logical pixels. The surface follows the window
    /// root node while it stays in the tree, and draws nothing while it is
    /// out. Returns the renderer of the surface this one replaces, if the
    /// window already had one.
    ///
    /// The renderer joins an app whose services the primary renderer
    /// already installed; nothing is attached to the app context again.
    pub fn add_window_surface(
        &mut self,
        window: u64,
        renderer: R,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
    ) -> Option<R> {
        let previous = self.remove_window_surface(window);
        self.surfaces.push(RootSurface::new(
            RootId::Window(window),
            renderer,
            buffer_size,
            viewport,
        ));
        self.app.window_roots_seen = None;
        self.sync_window_roots();
        previous
    }

    /// Takes the surface of `window` away, handing back its renderer.
    pub fn remove_window_surface(&mut self, window: u64) -> Option<R> {
        let root = RootId::Window(window);
        let index = self.surface_index(root)?;
        self.app.text_input_routes.borrow_mut().remove(root);
        Some(self.surfaces.remove(index).renderer)
    }

    /// Every surface's root, the primary first.
    pub fn surface_ids(&self) -> Vec<RootId> {
        self.surfaces.iter().map(|surface| surface.id).collect()
    }

    /// The window roots attached in the composition right now. A platform
    /// reads this after an update to open a window for each new entry and
    /// close the windows whose entries left.
    pub fn window_roots(&self) -> Vec<WindowRootEntry> {
        self.app.app_context.enter(cranpose_ui::window_roots)
    }

    /// Changes whenever a window root attaches, detaches or is updated; a
    /// platform that stored the last value it acted on skips
    /// [`Self::window_roots`] while it reads the same.
    pub fn window_roots_revision(&self) -> u64 {
        self.app
            .app_context
            .enter(cranpose_ui::window_roots_revision)
    }

    /// Whether the primary root lays out anything of its own: a node with
    /// area outside every window root. A platform hides the primary window
    /// while this is false, because every visible node is then in a window
    /// of its own.
    pub fn primary_has_content(&mut self) -> bool {
        self.read_primary_boxes(cranpose_ui::has_placed_content)
            .unwrap_or(false)
    }

    /// The extent of what the primary root lays out: the far right and
    /// bottom edges of its nodes with area, measured from the root's origin
    /// and leaving out every window root's subtree. `None` while it lays
    /// out nothing. A platform whose primary window wraps its content sizes
    /// the window to this after every update.
    pub fn primary_content_size(&mut self) -> Option<Size> {
        self.read_primary_boxes(cranpose_ui::placed_content_extent)
            .flatten()
    }

    /// Reads the boxes placed under the primary root with `read`, straight
    /// off the applier: these run after every update, so they build no
    /// layout snapshot.
    fn read_primary_boxes<T>(
        &mut self,
        read: impl FnOnce(&mut MemoryApplier, NodeId) -> Result<T, NodeError>,
    ) -> Option<T> {
        let _app_context = self.app.app_context.enter_scope();
        let root = self.surfaces[0].root_node(&self.app)?;
        let mut applier = self.app.composition.applier_mut();
        read(&mut applier, root)
            .inspect_err(|err| log::debug!("failed to read the primary root's boxes: {err}"))
            .ok()
    }

    /// The `drag_and_drop_target` under `point` and where the point falls in
    /// that target's surface. A point on the screen is looked up in every
    /// surface whose window position the platform told, later windows
    /// first and the primary last; a point without one only in `source`,
    /// the surface holding the press.
    pub(crate) fn drag_and_drop_target_at(
        &self,
        point: cranpose_ui::DragAndDropPoint,
        source: usize,
    ) -> Option<(NodeId, Point)> {
        let drag_and_drop = self.app.app_context.drag_and_drop();
        let target_in = |index: usize, local: Point| {
            self.surfaces[index]
                .renderer
                .scene()
                .hit_test_nodes(local.x, local.y)
                .into_iter()
                .find(|node| drag_and_drop.is_target(*node))
                .map(|node| (node, local))
        };
        match point.screen {
            Some(screen) => (0..self.surfaces.len()).rev().find_map(|index| {
                let local = self.surfaces[index].screen_point_inside(screen)?;
                target_in(index, local)
            }),
            None => target_in(source, point.local),
        }
    }

    /// Names the surface the platform considers focused, which is where the
    /// soft keyboard belongs. Pointer presses activate their surface on
    /// their own; a platform calls this for focus it grants otherwise.
    pub fn set_active_root(&mut self, root: RootId) {
        self.app.text_input_routes.borrow_mut().set_active(root);
    }

    /// The surface the platform considers focused.
    pub fn active_root(&self) -> RootId {
        self.app.text_input_routes.borrow().active()
    }

    fn sync_window_roots(&mut self) {
        let _app_context = self.app.app_context.enter_scope();
        self.sync_window_roots_in_context();
    }

    pub(crate) fn sync_window_roots_in_context(&mut self) {
        let revision = self.app.app_context.window_roots().revision();
        if self.app.window_roots_seen == Some(revision) {
            return;
        }
        self.app.window_roots_seen = Some(revision);
        let entries = cranpose_ui::window_roots();
        for surface in &mut self.surfaces {
            let RootId::Window(id) = surface.id else {
                continue;
            };
            let root = entries
                .iter()
                .find(|entry| entry.node as u64 == id)
                .map(|entry| entry.node);
            surface.set_root(root);
        }
    }

    pub(crate) fn any_surface_dirty(&self) -> bool {
        self.surfaces
            .iter()
            .any(|surface| surface.is_dirty || surface.scene_dirty)
    }

    pub(crate) fn mark_all_dirty(&mut self) {
        for surface in &mut self.surfaces {
            surface.is_dirty = true;
        }
    }

    fn clear_surface_dirt(&mut self) {
        for surface in &mut self.surfaces {
            surface.is_dirty = false;
        }
    }

    fn invalidate_dev_overlay_text(&mut self) {
        for surface in &mut self.surfaces {
            surface.invalidate_dev_overlay_text();
        }
    }

    /// Set development options for debugging and performance monitoring.
    ///
    /// The FPS counter and other overlays are rendered directly by the renderer
    /// (not via composition) to avoid affecting performance measurements.
    pub fn set_dev_options(&mut self, options: DevOptions) {
        self.app.dev_options = options;
        self.invalidate_dev_overlay_text();
        self.app.app_context.enter(request_render_invalidation);
        self.mark_all_dirty();
    }

    /// Get a reference to the current dev options.
    pub fn dev_options(&self) -> &DevOptions {
        &self.app.dev_options
    }

    /// Returns the frame pacing mode set in the dev options.
    pub fn frame_pacing_mode(&self) -> FramePacingMode {
        self.app.dev_options.frame_pacing_mode
    }

    /// Returns the presented-frame cadence in frames per second, as the dev overlay shows it.
    pub fn current_fps(&self) -> f32 {
        self.app.fps_monitor.current_fps()
    }

    /// Returns the frame interval and frame work statistics of the recent presented frames.
    pub fn fps_stats(&self) -> FpsStats {
        self.app.fps_monitor.stats()
    }

    /// Clears the frame statistics and the recomposition count shown by the dev overlay.
    pub fn reset_fps_stats(&mut self) {
        self.app.fps_monitor.reset_stats();
        self.invalidate_dev_overlay_text();
    }

    /// Records when the work for one presented frame started and finished. Platforms call it once
    /// per presented frame to feed the frame statistics.
    pub fn record_presented_frame(
        &mut self,
        frame_started_at: Instant,
        frame_finished_at: Instant,
    ) {
        self.app
            .fps_monitor
            .record_frame_work(frame_started_at, frame_finished_at);
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn record_presented_frame_for_test(
        &mut self,
        frame_started_nanos: u64,
        frame_finished_nanos: u64,
    ) {
        let started = self.app.start_time + std::time::Duration::from_nanos(frame_started_nanos);
        let finished = self.app.start_time + std::time::Duration::from_nanos(frame_finished_nanos);
        self.record_presented_frame(started, finished);
    }

    /// Sets the frame pacing mode and redraws the dev overlay. Platforms read the mode back with
    /// `frame_pacing_mode`.
    pub fn set_frame_pacing_mode(&mut self, mode: FramePacingMode) {
        if self.app.dev_options.frame_pacing_mode == mode {
            return;
        }
        self.app.dev_options.frame_pacing_mode = mode;
        self.invalidate_dev_overlay_text();
        self.app.app_context.enter(request_render_invalidation);
        self.mark_all_dirty();
    }

    /// Where the dev overlay draws the control for `mode`, in logical pixels.
    ///
    /// The overlay is drawn by the renderer rather than composed, so it has no
    /// semantics for a test to search. Without this a test that wants to press
    /// a pacing control has to hard-code a coordinate and silently starts
    /// passing against empty space the moment the overlay's text changes.
    pub fn dev_overlay_control_center(&self, mode: FramePacingMode) -> Option<(f32, f32)> {
        self.surfaces[0].dev_overlay_control_center(mode)
    }

    /// Sets the primary surface's viewport and lays out and renders at once.
    /// The size the surface already has changes nothing.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        if self.surfaces[0].viewport == (width, height) {
            return;
        }
        self.primary().set_viewport(width, height);
        self.process_frame();
    }

    /// Returns the width and height of the primary surface's viewport in logical pixels.
    pub fn viewport_size(&self) -> (f32, f32) {
        self.surfaces[0].viewport
    }

    /// Tells the shell where the primary window sits on the screen. See
    /// [`SurfaceMut::set_screen_origin`].
    pub fn set_screen_origin(&mut self, origin: Option<Point>) {
        self.surfaces[0].screen_origin = origin;
    }

    /// Sets the size in pixels of the buffer the primary surface renders into.
    pub fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.surfaces[0].buffer_size = (width, height);
    }

    /// Returns the width and height in pixels of the buffer the primary surface renders into.
    pub fn buffer_size(&self) -> (u32, u32) {
        self.surfaces[0].buffer_size
    }

    /// Returns the render scene of the primary surface.
    pub fn scene(&self) -> &R::Scene {
        self.surfaces[0].renderer.scene()
    }

    /// Returns the renderer of the primary surface.
    pub fn renderer(&mut self) -> &mut R {
        &mut self.surfaces[0].renderer
    }

    /// Sets the callback the runtime calls when it schedules a frame. Platforms use it to wake
    /// their loop. Other threads can call it, so it must be `Send` and `Sync`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.app.runtime.set_frame_waker(waker);
    }

    /// Sets the callback the runtime calls when it schedules a frame. Platforms use it to wake
    /// their loop. Wasm runs on one thread, so the callback need not be `Send`.
    #[cfg(target_arch = "wasm32")]
    pub fn set_frame_waker(&mut self, waker: impl Fn() + 'static) {
        self.app.runtime.set_frame_waker(waker);
    }

    /// Removes the callback set by `set_frame_waker`.
    pub fn clear_frame_waker(&mut self) {
        self.app.runtime.clear_frame_waker();
    }

    /// Returns whether the shell wants a rendered frame: a frame callback or animation is active,
    /// or a surface's scene is out of date.
    pub fn should_render(&self) -> bool {
        self.app.app_context.enter(|| {
            self.app.wants_frame_in_context()
                || self.surfaces.iter().any(|surface| surface.scene_dirty)
        })
    }

    /// Returns whether the shell has work to run: queued UI tasks, state changes, layout, semantics
    /// or an active animation.
    pub fn needs_update(&self) -> bool {
        self.app.app_context.enter(|| {
            self.app
                .needs_ui_update_in_context(self.any_surface_dirty())
        })
    }

    /// Returns whether queued UI, state, layout, semantics, or due cursor work can be processed
    /// without advancing the frame clock. Awaiting a display frame alone is false.
    pub fn needs_update_without_frame(&self) -> bool {
        self.app.app_context.enter(|| {
            self.any_surface_dirty()
                || self.app.has_stale_work_in_context()
                || self.app.composition.should_recompose()
                || self.app.composition.runtime().has_pending_ui()
                || self.app.app_context.has_pending_semantics_invalidations()
                || cranpose_ui::next_cursor_blink_time().is_some_and(|at| at <= Instant::now())
                || self
                    .surfaces
                    .iter()
                    .any(|surface| surface.renderer_warmup_due(&self.app))
        })
    }

    /// Returns true when the runtime holds work for the UI thread: posted tasks,
    /// continuations from worker threads, or async tasks ready to poll.
    ///
    /// This is the part of [`needs_update`](Self::needs_update) that another
    /// thread is waiting on, told apart from the part that only wants the screen
    /// redrawn. A platform backend running with no surface uses it to compose
    /// for work and stay asleep for animation.
    pub fn has_pending_ui(&self) -> bool {
        self.app.composition.runtime().has_pending_ui()
    }

    /// Runs the tasks woken since they last ran, now rather than at the next
    /// frame. A coroutine whose `delay` ran out resumes when its timer fires,
    /// as Compose's main dispatcher resumes one, and the next frame draws what
    /// it changed. Waiting for the frame instead would resume a 16 ms delay
    /// only every other frame, whenever the timer fires just after one starts.
    /// A platform calls this whenever its loop wakes.
    pub fn run_pending_tasks(&mut self) {
        // The tasks run in the app's context; a wake that brought none, as a
        // frame request does, needs no context.
        if !self.app.composition.runtime().has_pending_ui() {
            return;
        }
        let runtime_handle = self.app.composition.runtime_handle();
        self.app.app_context.enter(|| {
            runtime_handle.with_deferred_state_releases(|| runtime_handle.drain_ui());
        });
    }

    /// Returns true if the primary surface owes the display a frame: stale
    /// pixels, or a renderer that has not warmed its swapchain yet.
    ///
    /// An app that merely holds an open `next_frame()` await - a game loop, a
    /// polling effect - keeps [`Self::needs_update`] true forever without
    /// changing a single pixel. Such an app must still be *ticked* every frame,
    /// but the frame it produces is byte-identical to the last one, and
    /// presenting it costs a full swapchain rotation and pins the panel at its
    /// maximum refresh rate. Callers pair this with
    /// [`FrameUpdateResult::visual_changed`] from the update they just ran,
    /// which reports the work that update actually did.
    /// Note: Cursor blink is now timer-based and uses WaitUntil scheduling, not continuous redraw.
    pub fn needs_redraw(&self) -> bool {
        self.app
            .app_context
            .enter(|| self.surfaces[0].needs_redraw_in_context(&self.app))
    }

    /// Marks the primary surface as dirty, indicating a redraw is needed.
    pub fn mark_dirty(&mut self) {
        self.surfaces[0].is_dirty = true;
    }

    /// Asks for the root content to render again from the top on the next update, with a forced
    /// layout pass.
    pub fn request_root_render(&mut self) {
        self.app.composition.request_root_render();
        self.app.request_forced_layout_pass();
        self.app.app_context.enter(request_render_invalidation);
        self.mark_all_dirty();
    }

    /// Lays the composition out at `density` from now on, and has the primary
    /// renderer draw at that root scale.
    pub fn set_density(&mut self, density: f32) {
        self.surfaces[0].renderer.set_root_scale(density);
        let changed = self.app.app_context.enter(|| {
            let previous = cranpose_ui::current_density().to_bits();
            cranpose_ui::set_density(density);
            previous != cranpose_ui::current_density().to_bits()
        });
        if changed {
            self.request_root_render();
        }
    }

    /// Reports the system font scale the platform is showing text at.
    ///
    /// Hosts call this at startup and on every configuration change. Sizes in
    /// `Sp` follow it, sizes in `Dp` do not, which is what lets an app grow its
    /// text with the user's setting while its layout stays where it was.
    ///
    /// This takes the setting as a plain multiplier. A host whose platform
    /// converts `Sp` through a table of its own — Android 14 and up does — calls
    /// [`AppShell::set_font_scale_curve`] with what the platform answered
    /// instead, because the multiplication is not what that platform does.
    pub fn set_font_scale(&mut self, font_scale: f32) {
        self.set_font_scale_curve(cranpose_ui::FontScaleCurve::linear(font_scale));
    }

    /// Reports the system font scale together with the conversion behind it.
    ///
    /// See [`cranpose_ui::font_scale`]: above a threshold setting Android
    /// resolves a size in `Sp` through a piecewise-linear table rather than by
    /// multiplying, so a host that can read that table hands it over here and
    /// every `Sp` in the app resolves the way the platform's own text does.
    pub fn set_font_scale_curve(&mut self, curve: cranpose_ui::FontScaleCurve) {
        let changed = self.app.app_context.enter(|| {
            let previous = cranpose_ui::current_font_scale_curve();
            cranpose_ui::set_font_scale_curve(curve);
            previous != cranpose_ui::current_font_scale_curve()
        });
        if changed {
            self.request_root_render();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_current_density(&self) -> f32 {
        self.app.app_context.enter(cranpose_ui::current_density)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_current_font_scale(&self) -> f32 {
        self.app.app_context.enter(cranpose_ui::current_font_scale)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_current_font_scale_curve(&self) -> cranpose_ui::FontScaleCurve {
        self.app
            .app_context
            .enter(cranpose_ui::current_font_scale_curve)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_enter_app_context<T>(&self, block: impl FnOnce() -> T) -> T {
        self.app.app_context.enter(block)
    }

    /// Returns true if there are active animations or pending recompositions.
    pub fn has_active_animations(&self) -> bool {
        self.app.composition.should_render()
    }

    /// Returns `true` while a one-shot frame callback waits for the next frame. Perpetual frame
    /// callbacks do not count.
    pub fn has_transient_frame_callbacks(&self) -> bool {
        self.app
            .composition
            .runtime()
            .has_transient_frame_callbacks()
    }

    /// Returns `true` while a pointer button is held down on a control of the primary surface.
    pub fn has_active_pointer_gesture(&self) -> bool {
        self.surfaces[0].has_active_pointer_gesture()
    }

    /// Primary-surface form of [`SurfaceMut::has_active_primary_gesture`].
    pub fn has_active_primary_gesture(&self) -> bool {
        self.surfaces[0].has_active_primary_gesture()
    }

    /// Primary-surface form of [`SurfaceMut::frame_owed`].
    pub fn frame_owed(&self) -> bool {
        self.surfaces[0].frame_owed
    }

    /// Primary-surface form of [`SurfaceMut::take_frame_owed`].
    pub fn take_frame_owed(&mut self) -> bool {
        std::mem::take(&mut self.surfaces[0].frame_owed)
    }

    /// Returns when the next timed event is due: a cursor blink, or a
    /// `delay` that waits for the host to drain the UI work, as on ESP-IDF.
    /// Use this for `ControlFlow::WaitUntil` scheduling.
    pub fn next_event_time(&self) -> Option<Instant> {
        self.app.next_event_time()
    }

    fn compute_frame_schedule(&self) -> FrameSchedule {
        self.surfaces[0].compute_frame_schedule(&self.app, self.any_surface_dirty())
    }

    /// Computes the frame schedule of the primary surface, stores it in the surface's scheduler and
    /// returns it.
    pub fn frame_schedule(&self) -> FrameSchedule {
        let schedule = self.compute_frame_schedule();
        self.surfaces[0].frame_scheduler.record(schedule);
        schedule
    }

    /// Computes and stores the frame schedule of the primary surface, then asks `driver` for the
    /// frame or wake it calls for. Returns the schedule.
    pub fn schedule_platform_frame<D>(&self, driver: &D) -> FrameSchedule
    where
        D: PlatformFrameDriver + ?Sized,
    {
        let schedule = self.compute_frame_schedule();
        self.surfaces[0].frame_scheduler.schedule(schedule, driver);
        schedule
    }

    /// Returns the schedule last stored by `frame_schedule` or `schedule_platform_frame`, without
    /// computing a new one.
    pub fn frame_scheduler_snapshot(&self) -> FrameSchedule {
        self.surfaces[0].frame_scheduler.snapshot()
    }

    /// Timestamp a live input sample against the current animation clock.
    pub fn realtime_pointer_event_time(&self, platform_time_ms: Option<i64>) -> PointerEventTime {
        self.app.realtime_pointer_event_time(platform_time_ms)
    }

    /// Timestamp deterministic input at the most recently processed frame.
    pub fn exact_pointer_event_time(&self, platform_time_ms: Option<i64>) -> PointerEventTime {
        PointerEventTime {
            platform_time_ms,
            animation_time_nanos: self.app.last_frame_time_nanos,
        }
    }

    /// Advances the frame clock by `frame_interval` past the later of the last frame time and the
    /// current time, and runs one update there.
    pub fn update_after_frame_interval(
        &mut self,
        frame_interval: std::time::Duration,
    ) -> FrameUpdateResult {
        let wall_frame_time = self.app.frame_time_nanos_at(Instant::now());
        let base_frame_time = self.app.last_frame_time_nanos.max(wall_frame_time);
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
            .app
            .last_frame_time_nanos
            .saturating_add(frame_interval.as_nanos().min(u128::from(u64::MAX)) as u64);
        self.update_at_frame_time_nanos(frame_time)
    }

    /// Delivers a display frame at a monotonic platform instant, then runs the
    /// continuations it wakes. This is independent of renderer availability.
    /// The instant is normalized to the shell's start, sharing the input clock's epoch.
    /// Repeated or older frame timestamps do not wake frame awaiters again.
    /// Use [`Self::update_without_frame`] to compose and draw the resulting state.
    pub fn dispatch_frame_at(&mut self, frame_time: Instant) {
        let frame_time = self.app.frame_time_nanos_at(frame_time);
        let _app_context = self.app.app_context.enter_scope();
        let runtime_handle = self.app.runtime.runtime_handle();
        runtime_handle.with_deferred_state_releases(|| {
            self.app.last_frame_time_nanos = frame_time.max(self.app.last_frame_time_nanos);
            self.app
                .runtime
                .drain_frame_callbacks(self.app.last_frame_time_nanos);
            self.apply_pointer_claim();
            runtime_handle.drain_ui();
            self.dispatch_requested_mouse_moves(self.app.last_frame_time_nanos);
        });
    }

    /// Processes pending state, input, layout, drawing, and semantics work without
    /// waking frame awaiters. Platforms use this for work between display frames.
    pub fn update_without_frame(&mut self) -> FrameUpdateResult {
        self.update_with_frame_time(None)
    }

    /// Processes one update at a frame timestamp in nanoseconds since this shell
    /// started. Duplicate or older timestamps process UI work without delivering
    /// another frame callback.
    pub fn update_at_frame_time_nanos(&mut self, frame_time: u64) -> FrameUpdateResult {
        self.update_with_frame_time(Some(frame_time))
    }

    /// Out of line: a platform's loop calls the update of each frame from
    /// one place, and inlined there the update's code would spread through
    /// the loop's own.
    #[inline(never)]
    fn update_with_frame_time(&mut self, frame_time: Option<u64>) -> FrameUpdateResult {
        let _app_context = self.app.app_context.enter_scope();
        let mut stages = UpdateStages::start(update_stage_telemetry_threshold_ms().is_some());
        let runtime_handle = self.app.runtime.runtime_handle();
        runtime_handle.with_deferred_state_releases(|| {
            if let Some(frame_time) = frame_time {
                self.app.last_frame_time_nanos = frame_time.max(self.app.last_frame_time_nanos);
                self.app.runtime.drain_frame_callbacks(self.app.last_frame_time_nanos);
                self.apply_pointer_claim();
            }
            stages.reached();
            runtime_handle.drain_ui();
            self.dispatch_requested_mouse_moves(self.app.last_frame_time_nanos);
            stages.reached();
            let should_render = self.app.composition.should_recompose();
            let mut reconcile_attempted = false;
            let mut reconcile_changed = false;
            if should_render {
                log::trace!(
                    target: "cranpose::input",
                    "update begin: should_render=true layout_requested={} scene_dirty={} is_dirty={}",
                    self.app.layout_requested,
                    self.surfaces[0].scene_dirty,
                    self.surfaces[0].is_dirty
                );
                (reconcile_attempted, reconcile_changed) = self.reconcile_in_context();
            }
            stages.reached();
            let result = self.process_frame_in_context(reconcile_changed);
            stages.reached();
            log_update_stage_telemetry(
                &stages,
                UpdateOutcome {
                    should_render,
                    reconcile_attempted,
                    reconcile_changed,
                },
            );
            self.clear_surface_dirt();
            result
        })
    }

    /// Gives a held press that a node claimed during the frame, as a long
    /// press does, to that node alone.
    fn apply_pointer_claim(&mut self) {
        let Some(node) = self.app.app_context.take_pointer_claim() else {
            return;
        };
        self.give_press_to(node);
    }

    /// Out of line: a frame seldom has a claim, so the frame's own code
    /// stays small.
    #[inline(never)]
    fn give_press_to(&mut self, node: NodeId) {
        for index in 0..self.surfaces.len() {
            SurfaceMut::new(self, index).give_press_to(node);
        }
    }

    fn dispatch_requested_mouse_moves(&mut self, frame_time: u64) {
        let pending = self.app.app_context.pending_mouse_moves();
        if pending > 0 {
            self.dispatch_mouse_moves(pending, frame_time);
        }
    }

    /// Moves the cursor of each surface that `pending` requested mouse moves
    /// name. Out of line, as most frames have none.
    #[inline(never)]
    fn dispatch_mouse_moves(&mut self, pending: usize, frame_time: u64) {
        use cranpose_ui::mouse_input::MouseInputTarget;
        for _ in 0..pending {
            let Some(request) = self.app.app_context.take_mouse_move() else {
                break;
            };
            let root = match request.target {
                MouseInputTarget::Primary => RootId::Primary,
                MouseInputTarget::Window(id) => RootId::Window(id),
            };
            if let Some(mut surface) = self.surface(root) {
                surface.set_pointer_source(PointerSource::Mouse);
                surface.set_cursor_at_event_time(request.position.x, request.position.y, PointerEventTime {
                    platform_time_ms: Some((frame_time / 1_000_000).min(i64::MAX as u64) as i64),
                    animation_time_nanos: frame_time,
                });
            }
        }
    }

    fn reconcile_in_context(&mut self) -> (bool, bool) {
        let Some(root_key) = self.app.composition.root_key() else {
            return (false, false);
        };
        match self
            .app
            .composition
            .reconcile(root_key, &mut *self.app.content)
        {
            Ok(changed) => {
                log::trace!(
                    target: "cranpose::input",
                    "reconcile changed={changed}"
                );
                if changed {
                    self.app.fps_monitor.record_recomposition();
                    if self.app.composition_tree_needs_layout() {
                        self.app.request_layout_pass();
                    }
                    request_render_invalidation();
                }
                (true, changed)
            }
            Err(NodeError::Missing { id }) => {
                log::debug!("Recomposition skipped: node {id} no longer exists");
                self.app.request_layout_pass();
                request_render_invalidation();
                (true, false)
            }
            Err(err) => {
                log::error!("recomposition failed: {err}");
                self.app.request_layout_pass();
                request_render_invalidation();
                (true, false)
            }
        }
    }

    /// Runs one update at the current time: state, input, layout, drawing and semantics. Returns
    /// what changed.
    pub fn update(&mut self) -> FrameUpdateResult {
        let frame_time = self.app.frame_time_nanos_at(Instant::now());
        self.update_at_frame_time_nanos(frame_time)
    }
}

impl<R> Drop for AppShell<R>
where
    R: Renderer,
{
    fn drop(&mut self) {
        self.app.runtime.clear_frame_waker();
    }
}

/// Returns the standard root key to pass to the shell constructors. It is the same on every call.
pub fn default_root_key() -> Key {
    location_key(file!(), line!(), column!())
}

#[cfg(test)]
#[path = "tests/app_shell_frame_pacing_tests.rs"]
mod frame_pacing_tests;

#[cfg(test)]
#[path = "tests/app_shell_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/surface_tests.rs"]
mod surface_tests;
