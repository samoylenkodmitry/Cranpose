//! How much memory the device has left, as its host reports it, for the
//! current shell. Lazy layouts and the text engine read it to decide how
//! much they keep.

use cranpose_ui_graphics::MemoryHeadroom;

/// The memory the host of the current shell last reported it has left:
/// [`MemoryHeadroom::Ample`] until it reports.
pub fn current_memory_headroom() -> MemoryHeadroom {
    crate::render_state::current_app_context()
        .map_or(MemoryHeadroom::Ample, |context| context.memory_headroom())
}

/// Sets the memory the current shell's host has left. Lazy layouts read it on
/// their next measure. Call it on the shell's thread, inside its context.
pub fn set_memory_headroom(headroom: MemoryHeadroom) {
    crate::render_state::require_current_app_context("a memory headroom report")
        .set_memory_headroom(headroom);
}
