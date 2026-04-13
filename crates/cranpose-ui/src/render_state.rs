use cranpose_core::NodeId;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

thread_local! {
    static LAYOUT_REPASS_MANAGER: RefCell<LayoutRepassManager> =
        RefCell::new(LayoutRepassManager::new());
    static DRAW_REPASS_MANAGER: RefCell<DrawRepassManager> =
        RefCell::new(DrawRepassManager::new());
    static RENDER_INVALIDATED: Cell<bool> = const { Cell::new(false) };
    static POINTER_INVALIDATED: Cell<bool> = const { Cell::new(false) };
    static FOCUS_INVALIDATED: Cell<bool> = const { Cell::new(false) };
    static LAYOUT_INVALIDATED: Cell<bool> = const { Cell::new(false) };
    static DENSITY_BITS: Cell<u32> = const { Cell::new(f32::to_bits(1.0)) };
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
    LAYOUT_REPASS_MANAGER.with(|manager| {
        manager.borrow_mut().schedule_repass(node_id);
    });
    // Set the layout-invalidated flag so the app shell knows to process repasses.
    // The app shell will check take_layout_repass_nodes() first (scoped path),
    // and only falls back to global invalidation if the flag is set without any repass nodes.
    LAYOUT_INVALIDATED.with(|flag| flag.set(true));
    // Also request render invalidation so the frame is actually drawn.
    // Without this, programmatic scrolls (e.g., scroll_to_item) wouldn't trigger a redraw
    // until the next user interaction caused a frame request.
    request_render_invalidation();
}

/// Schedules a draw-only repass for a specific node.
///
/// This ensures draw/pointer data stays in sync when modifier updates do not
/// require a layout pass (e.g., draw-only modifier changes).
pub fn schedule_draw_repass(node_id: NodeId) {
    DRAW_REPASS_MANAGER.with(|manager| {
        manager.borrow_mut().schedule_repass(node_id);
    });
}

/// Returns true if any draw repasses are pending.
pub fn has_pending_draw_repasses() -> bool {
    DRAW_REPASS_MANAGER.with(|manager| manager.borrow().has_pending_repass())
}

/// Takes all pending draw repass node IDs.
pub fn take_draw_repass_nodes() -> Vec<NodeId> {
    DRAW_REPASS_MANAGER.with(|manager| manager.borrow_mut().take_dirty_nodes())
}

/// Returns true if any layout repasses are pending.
pub fn has_pending_layout_repasses() -> bool {
    LAYOUT_REPASS_MANAGER.with(|manager| manager.borrow().has_pending_repass())
}

/// Takes all pending layout repass node IDs.
///
/// The caller should iterate over these and call `bubble_layout_dirty` for each.
pub fn take_layout_repass_nodes() -> Vec<NodeId> {
    LAYOUT_REPASS_MANAGER.with(|manager| manager.borrow_mut().take_dirty_nodes())
}

/// Returns the current density scale factor (logical px per dp).
pub fn current_density() -> f32 {
    DENSITY_BITS.with(|bits| f32::from_bits(bits.get()))
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
    DENSITY_BITS.with(|bits| {
        let old_bits = bits.replace(new_bits);
        if old_bits != new_bits {
            LAYOUT_INVALIDATED.with(|flag| flag.set(true));
        }
    });
}

/// Requests that the renderer rebuild the current scene.
pub fn request_render_invalidation() {
    RENDER_INVALIDATED.with(|flag| flag.set(true));
}

/// Returns true if a render invalidation was pending and clears the flag.
pub fn take_render_invalidation() -> bool {
    RENDER_INVALIDATED.with(|flag| flag.replace(false))
}

/// Returns true if a render invalidation is pending without clearing it.
pub fn peek_render_invalidation() -> bool {
    RENDER_INVALIDATED.with(Cell::get)
}

/// Requests a new pointer-input pass without touching layout or draw dirties.
pub fn request_pointer_invalidation() {
    POINTER_INVALIDATED.with(|flag| flag.set(true));
}

/// Returns true if a pointer invalidation was pending and clears the flag.
pub fn take_pointer_invalidation() -> bool {
    POINTER_INVALIDATED.with(|flag| flag.replace(false))
}

/// Returns true if a pointer invalidation is pending without clearing it.
pub fn peek_pointer_invalidation() -> bool {
    POINTER_INVALIDATED.with(Cell::get)
}

/// Requests a focus recomposition without affecting layout/draw dirties.
pub fn request_focus_invalidation() {
    FOCUS_INVALIDATED.with(|flag| flag.set(true));
}

/// Returns true if a focus invalidation was pending and clears the flag.
pub fn take_focus_invalidation() -> bool {
    FOCUS_INVALIDATED.with(|flag| flag.replace(false))
}

/// Returns true if a focus invalidation is pending without clearing it.
pub fn peek_focus_invalidation() -> bool {
    FOCUS_INVALIDATED.with(Cell::get)
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
    LAYOUT_INVALIDATED.with(|flag| flag.set(true));
}

/// Returns true if a layout invalidation was pending and clears the flag.
pub fn take_layout_invalidation() -> bool {
    LAYOUT_INVALIDATED.with(|flag| flag.replace(false))
}

/// Returns true if a layout invalidation is pending without clearing it.
pub fn peek_layout_invalidation() -> bool {
    LAYOUT_INVALIDATED.with(Cell::get)
}

#[cfg(test)]
fn reset_test_state() {
    let _ = take_draw_repass_nodes();
    let _ = take_layout_repass_nodes();
    let _ = take_render_invalidation();
    let _ = take_pointer_invalidation();
    let _ = take_focus_invalidation();
    let _ = take_layout_invalidation();
    set_density(1.0);
    let _ = take_layout_invalidation();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn invalidation_flags_are_thread_local() {
        reset_test_state();
        let (tx, rx) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            reset_test_state();
            request_render_invalidation();
            request_pointer_invalidation();
            request_focus_invalidation();
            request_layout_invalidation();
            set_density(2.0);
            tx.send(()).expect("signal invalidation setup");

            (
                peek_render_invalidation(),
                peek_pointer_invalidation(),
                peek_focus_invalidation(),
                peek_layout_invalidation(),
                current_density(),
            )
        });

        rx.recv().expect("wait for worker invalidation setup");
        assert!(!peek_render_invalidation());
        assert!(!peek_pointer_invalidation());
        assert!(!peek_focus_invalidation());
        assert!(!peek_layout_invalidation());
        assert_eq!(current_density(), 1.0);

        let (render, pointer, focus, layout, density) =
            handle.join().expect("worker invalidation snapshot");
        assert!(render);
        assert!(pointer);
        assert!(focus);
        assert!(layout);
        assert_eq!(density, 2.0);

        reset_test_state();
    }
}
