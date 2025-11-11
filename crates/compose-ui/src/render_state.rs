use std::sync::atomic::{AtomicBool, Ordering};

static RENDER_INVALIDATED: AtomicBool = AtomicBool::new(false);
static POINTER_INVALIDATED: AtomicBool = AtomicBool::new(false);
static FOCUS_INVALIDATED: AtomicBool = AtomicBool::new(false);

/// Requests that the renderer rebuild the current scene.
pub fn request_render_invalidation() {
    RENDER_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Returns true if a render invalidation was pending and clears the flag.
pub fn take_render_invalidation() -> bool {
    RENDER_INVALIDATED.swap(false, Ordering::Relaxed)
}

/// Returns true if a render invalidation is pending without clearing it.
pub fn peek_render_invalidation() -> bool {
    RENDER_INVALIDATED.load(Ordering::Relaxed)
}

/// Requests a new pointer-input pass without touching layout or draw dirties.
pub fn request_pointer_invalidation() {
    POINTER_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Returns true if a pointer invalidation was pending and clears the flag.
pub fn take_pointer_invalidation() -> bool {
    POINTER_INVALIDATED.swap(false, Ordering::Relaxed)
}

/// Returns true if a pointer invalidation is pending without clearing it.
pub fn peek_pointer_invalidation() -> bool {
    POINTER_INVALIDATED.load(Ordering::Relaxed)
}

/// Requests a focus recomposition without affecting layout/draw dirties.
pub fn request_focus_invalidation() {
    FOCUS_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Returns true if a focus invalidation was pending and clears the flag.
pub fn take_focus_invalidation() -> bool {
    FOCUS_INVALIDATED.swap(false, Ordering::Relaxed)
}

/// Returns true if a focus invalidation is pending without clearing it.
pub fn peek_focus_invalidation() -> bool {
    FOCUS_INVALIDATED.load(Ordering::Relaxed)
}
