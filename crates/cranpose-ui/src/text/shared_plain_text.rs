//! Shared copies of plain, style-free strings that draw scopes lower every
//! frame, pooled per thread.

use std::sync::Arc;

use cranpose_text::{AnnotatedString, RenderString};

/// Returns the shared [`AnnotatedString`] for a plain, style-free string,
/// reusing the copy made on an earlier frame when the content matches.
///
/// Draw scopes lower every `draw_text*` call through an `AnnotatedString` on
/// every frame — once to measure and once to emit — and a HUD label or score
/// counter has the same characters frame after frame. Without this pool each
/// pass re-copied a string it had copied the frame before, only to hash it and
/// hit a layout cache that is keyed by content anyway. Entries are verified by
/// content on hit, so a hash collision costs a fresh copy, never wrong text.
/// The pool keeps the strings that something still holds, as
/// [`cranpose_ui_graphics::ContentPool`] does.
pub fn shared_plain_annotated_string(text: &str) -> AnnotatedString {
    with_shared_plain_text(text, |shared| shared.0.clone())
}

/// The [`RenderString`] of [`shared_plain_annotated_string`]'s string for
/// `text`: its content, so a canvas redrawing the same characters hands the
/// renderer the same allocation every frame.
pub fn shared_plain_render_string(text: &str) -> Arc<RenderString> {
    with_shared_plain_text(text, |shared| Arc::clone(shared.0.render_string()))
}

pub(crate) struct SharedPlainText(AnnotatedString);

impl cranpose_ui_graphics::PooledValue for SharedPlainText {
    fn is_held(&self) -> bool {
        Arc::strong_count(self.0.render_string()) > 1
    }
}

fn with_shared_plain_text<R>(text: &str, read: impl FnOnce(&SharedPlainText) -> R) -> R {
    use std::hash::{Hash, Hasher};

    let mut hasher = cranpose_ui_graphics::FxHasher::default();
    text.hash(&mut hasher);
    crate::thread_state::with(|state| {
        state.shared_plain_texts.borrow_mut().with(
            hasher.finish(),
            |shared| shared.0.text() == text,
            || SharedPlainText(AnnotatedString::new(text.to_owned())),
            read,
        )
    })
}

#[cfg(test)]
#[path = "tests/shared_plain_text_tests.rs"]
mod tests;
