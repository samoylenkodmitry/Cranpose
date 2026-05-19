use cranpose_core::{current_runtime_handle, NodeId, SnapshotStateObserver};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
#[cfg(not(any(test, feature = "test-helpers")))]
use std::sync::OnceLock;
#[cfg(test)]
use std::sync::OnceLock;

struct RenderState {
    layout_repasses: Mutex<LayoutRepassManager>,
    draw_repasses: Mutex<DrawRepassManager>,
    modifier_slice_repasses: Mutex<LayoutRepassManager>,
    render_invalidated: AtomicBool,
    pointer_invalidated: AtomicBool,
    focus_invalidated: AtomicBool,
    layout_invalidated: AtomicBool,
    density_bits: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawObservationScope {
    node_id: NodeId,
    command_index: usize,
}

impl DrawObservationScope {
    pub(crate) fn new(node_id: NodeId, command_index: usize) -> Self {
        Self {
            node_id,
            command_index,
        }
    }
}

std::thread_local! {
    static DRAW_OBSERVER: SnapshotStateObserver = {
        let observer = SnapshotStateObserver::new(|callback| {
            if let Some(runtime) = current_runtime_handle() {
                runtime.enqueue_ui_task(callback);
            } else {
                callback();
            }
        });
        observer.start();
        observer
    };
}

pub(crate) fn observe_draw_reads<R>(scope: DrawObservationScope, block: impl FnOnce() -> R) -> R {
    DRAW_OBSERVER.with(|observer| {
        observer.observe_reads(
            scope,
            |scope| {
                schedule_draw_repass(scope.node_id);
            },
            block,
        )
    })
}

pub(crate) fn clear_draw_observations_for_node(node_id: NodeId) {
    DRAW_OBSERVER.with(|observer| {
        observer.clear_if(|scope| {
            scope
                .downcast_ref::<DrawObservationScope>()
                .is_some_and(|scope| scope.node_id == node_id)
        });
    });
}

impl RenderState {
    fn new() -> Self {
        Self {
            layout_repasses: Mutex::new(LayoutRepassManager::new()),
            draw_repasses: Mutex::new(DrawRepassManager::new()),
            modifier_slice_repasses: Mutex::new(LayoutRepassManager::new()),
            render_invalidated: AtomicBool::new(false),
            pointer_invalidated: AtomicBool::new(false),
            focus_invalidated: AtomicBool::new(false),
            layout_invalidated: AtomicBool::new(false),
            density_bits: AtomicU32::new(f32::to_bits(1.0)),
        }
    }
}

#[cfg(not(any(test, feature = "test-helpers")))]
fn with_render_state<R>(f: impl FnOnce(&RenderState) -> R) -> R {
    static STATE: OnceLock<RenderState> = OnceLock::new();
    f(STATE.get_or_init(RenderState::new))
}

#[cfg(any(test, feature = "test-helpers"))]
fn with_render_state<R>(f: impl FnOnce(&RenderState) -> R) -> R {
    std::thread_local! {
        static STATE: RenderState = RenderState::new();
    }
    STATE.with(f)
}

/// Manages scoped layout invalidations for specific nodes.
///
/// Similar to PointerDispatchManager, this tracks which specific nodes
/// need layout invalidation rather than forcing a global invalidation.
struct LayoutRepassManager {
    dirty_nodes: HashSet<NodeId>,
}

impl LayoutRepassManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
        }
    }

    fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        self.dirty_nodes.drain().collect()
    }
}

/// Tracks draw-only invalidations so render data can be refreshed without layout.
struct DrawRepassManager {
    dirty_nodes: HashSet<NodeId>,
}

impl DrawRepassManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
        }
    }

    fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        self.dirty_nodes.drain().collect()
    }
}

/// Schedules a layout repass for a specific node.
///
/// **This is the preferred way to invalidate layout for local changes** (e.g., scroll, single-node mutations).
///
/// The app shell will call `take_layout_repass_nodes()` and bubble dirty flags up the tree
/// via `bubble_layout_dirty`. This gives you **O(subtree) performance** - only the affected
/// subtree is remeasured, and layout caches for other parts of the app remain valid.
///
/// # Implementation Note
///
/// This sets the `LAYOUT_INVALIDATED` flag to signal the app shell there's work to do,
/// but the flag alone does NOT trigger global cache invalidation. The app shell checks
/// `take_layout_repass_nodes()` first and processes scoped repasses. Global cache invalidation
/// only happens if the flag is set AND there are no scoped repasses (a rare fallback case).
///
/// # For Global Invalidation
///
/// For rare global events (window resize, global scale changes), use `request_layout_invalidation()` instead.
pub fn schedule_layout_repass(node_id: NodeId) {
    with_render_state(|state| {
        state
            .layout_repasses
            .lock()
            .expect("layout repass manager poisoned")
            .schedule_repass(node_id);
        state.layout_invalidated.store(true, Ordering::Relaxed);
    });
    // Set the layout-invalidated flag so the app shell knows to process repasses.
    // The app shell will check take_layout_repass_nodes() first (scoped path),
    // and only falls back to global invalidation if the flag is set without any repass nodes.
    // Also request render invalidation so the frame is actually drawn.
    // Without this, programmatic scrolls (e.g., scroll_to_item) wouldn't trigger a redraw
    // until the next user interaction caused a frame request.
    request_render_invalidation();
}

pub(crate) fn schedule_modifier_slices_repass(node_id: NodeId) {
    with_render_state(|state| {
        state
            .modifier_slice_repasses
            .lock()
            .expect("modifier slice repass manager poisoned")
            .schedule_repass(node_id);
    });
    schedule_layout_repass(node_id);
}

/// Schedules a draw-only repass for a specific node.
///
/// This ensures draw/pointer data stays in sync when modifier updates do not
/// require a layout pass (e.g., draw-only modifier changes).
pub fn schedule_draw_repass(node_id: NodeId) {
    with_render_state(|state| {
        state
            .draw_repasses
            .lock()
            .expect("draw repass manager poisoned")
            .schedule_repass(node_id);
    });
    request_render_invalidation();
}

/// Returns true if any draw repasses are pending.
pub fn has_pending_draw_repasses() -> bool {
    with_render_state(|state| {
        state
            .draw_repasses
            .lock()
            .expect("draw repass manager poisoned")
            .has_pending_repass()
    })
}

/// Takes all pending draw repass node IDs.
pub fn take_draw_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| {
        state
            .draw_repasses
            .lock()
            .expect("draw repass manager poisoned")
            .take_dirty_nodes()
    })
}

/// Returns true if any layout repasses are pending.
pub fn has_pending_layout_repasses() -> bool {
    with_render_state(|state| {
        state
            .layout_repasses
            .lock()
            .expect("layout repass manager poisoned")
            .has_pending_repass()
    })
}

/// Takes all pending layout repass node IDs.
///
/// The caller should iterate over these and call `bubble_layout_dirty` for each.
pub fn take_layout_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| {
        state
            .layout_repasses
            .lock()
            .expect("layout repass manager poisoned")
            .take_dirty_nodes()
    })
}

pub(crate) fn take_modifier_slice_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| {
        state
            .modifier_slice_repasses
            .lock()
            .expect("modifier slice repass manager poisoned")
            .take_dirty_nodes()
    })
}

/// Returns the current density scale factor (logical px per dp).
pub fn current_density() -> f32 {
    with_render_state(|state| f32::from_bits(state.density_bits.load(Ordering::Relaxed)))
}

/// Updates the current density scale factor.
///
/// This triggers a global layout invalidation when the value changes because
/// density impacts layout, text measurement, and input thresholds.
pub fn set_density(density: f32) {
    let normalized = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    let new_bits = normalized.to_bits();
    with_render_state(|state| {
        let old_bits = state.density_bits.swap(new_bits, Ordering::Relaxed);
        if old_bits != new_bits {
            state.layout_invalidated.store(true, Ordering::Relaxed);
        }
    });
}

/// Requests that the renderer rebuild the current scene.
pub fn request_render_invalidation() {
    with_render_state(|state| state.render_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a render invalidation was pending and clears the flag.
pub fn take_render_invalidation() -> bool {
    with_render_state(|state| state.render_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a render invalidation is pending without clearing it.
pub fn peek_render_invalidation() -> bool {
    with_render_state(|state| state.render_invalidated.load(Ordering::Relaxed))
}

/// Requests a new pointer-input pass without touching layout or draw dirties.
pub fn request_pointer_invalidation() {
    with_render_state(|state| state.pointer_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a pointer invalidation was pending and clears the flag.
pub fn take_pointer_invalidation() -> bool {
    with_render_state(|state| state.pointer_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a pointer invalidation is pending without clearing it.
pub fn peek_pointer_invalidation() -> bool {
    with_render_state(|state| state.pointer_invalidated.load(Ordering::Relaxed))
}

/// Requests a focus recomposition without affecting layout/draw dirties.
pub fn request_focus_invalidation() {
    with_render_state(|state| state.focus_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a focus invalidation was pending and clears the flag.
pub fn take_focus_invalidation() -> bool {
    with_render_state(|state| state.focus_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a focus invalidation is pending without clearing it.
pub fn peek_focus_invalidation() -> bool {
    with_render_state(|state| state.focus_invalidated.load(Ordering::Relaxed))
}

/// Requests a **global** layout re-run.
///
/// # ⚠️ WARNING: Extremely Expensive - O(entire app size)
///
/// This triggers internal cache invalidation that forces **every node** in the app
/// to re-measure, even if nothing changed. This is a performance footgun!
///
/// ## Valid Use Cases (rare!)
///
/// Only use this for **true global changes** that affect layout computation everywhere:
/// - Window/viewport resize
/// - Global font scale or density changes
/// - System-wide theme changes that affect layout
/// - Debug toggles that change layout behavior globally
///
/// ## For Local Changes - DO NOT USE THIS
///
/// **If you're invalidating layout for scroll, a single widget update, or any local change,
/// you MUST use the scoped repass mechanism instead:**
///
/// ```text
/// cranpose_ui::schedule_layout_repass(node_id);
/// ```
///
/// Scoped repasses give you O(subtree) performance instead of O(app), and they don't
/// invalidate caches across the entire app.
pub fn request_layout_invalidation() {
    with_render_state(|state| state.layout_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a layout invalidation was pending and clears the flag.
pub fn take_layout_invalidation() -> bool {
    with_render_state(|state| state.layout_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a layout invalidation is pending without clearing it.
pub fn peek_layout_invalidation() -> bool {
    with_render_state(|state| state.layout_invalidated.load(Ordering::Relaxed))
}

#[cfg(any(test, feature = "test-helpers"))]
#[doc(hidden)]
pub fn reset_render_state_for_tests() {
    let _ = take_draw_repass_nodes();
    let _ = take_layout_repass_nodes();
    let _ = take_modifier_slice_repass_nodes();
    let _ = take_render_invalidation();
    let _ = take_pointer_invalidation();
    let _ = take_focus_invalidation();
    let _ = take_layout_invalidation();
    set_density(1.0);
    let _ = take_layout_invalidation();
}

#[cfg(test)]
pub(crate) fn render_state_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match TEST_LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};

    #[test]
    fn invalidation_flags_are_shared_across_threads() {
        let state = Arc::new(RenderState::new());
        let (tx, rx) = mpsc::channel();
        let worker_state = Arc::clone(&state);

        let handle = std::thread::spawn(move || {
            worker_state
                .render_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .pointer_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .focus_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .layout_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .density_bits
                .store(f32::to_bits(2.0), Ordering::Relaxed);
            tx.send(()).expect("signal invalidation setup");

            f32::from_bits(worker_state.density_bits.load(Ordering::Relaxed))
        });

        rx.recv().expect("wait for worker invalidation setup");
        assert!(state.render_invalidated.load(Ordering::Relaxed));
        assert!(state.pointer_invalidated.load(Ordering::Relaxed));
        assert!(state.focus_invalidated.load(Ordering::Relaxed));
        assert!(state.layout_invalidated.load(Ordering::Relaxed));
        assert_eq!(
            f32::from_bits(state.density_bits.load(Ordering::Relaxed)),
            2.0
        );
        assert!(state.render_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.pointer_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.focus_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.layout_invalidated.swap(false, Ordering::Relaxed));

        let density = handle.join().expect("worker invalidation snapshot");
        assert_eq!(density, 2.0);
        assert!(!state.render_invalidated.load(Ordering::Relaxed));
        assert!(!state.pointer_invalidated.load(Ordering::Relaxed));
        assert!(!state.focus_invalidated.load(Ordering::Relaxed));
        assert!(!state.layout_invalidated.load(Ordering::Relaxed));
    }
}
