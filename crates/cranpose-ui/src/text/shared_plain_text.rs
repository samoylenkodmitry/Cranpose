//! Shared copies of plain, style-free strings that draw scopes lower every
//! frame, pooled per thread.

use std::rc::Rc;

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
pub fn shared_plain_annotated_string(text: &str) -> Rc<AnnotatedString> {
    with_shared_plain_text(text, |shared| Rc::clone(&shared.annotated))
}

/// The [`RenderString`] of [`shared_plain_annotated_string`]'s string for
/// `text`, converted once while the pool holds it, so a canvas redrawing the
/// same characters hands the renderer the same allocation every frame.
pub fn shared_plain_render_string(text: &str) -> std::sync::Arc<RenderString> {
    with_shared_plain_text(text, |shared| {
        std::sync::Arc::clone(
            shared
                .render
                .get_or_init(|| std::sync::Arc::new(shared.annotated.render_string())),
        )
    })
}

pub(crate) struct SharedPlainText {
    annotated: Rc<AnnotatedString>,
    render: std::cell::OnceCell<std::sync::Arc<RenderString>>,
}

impl cranpose_ui_graphics::PooledValue for SharedPlainText {
    fn is_held(&self) -> bool {
        self.annotated.is_held()
            || self
                .render
                .get()
                .is_some_and(|render| std::sync::Arc::strong_count(render) > 1)
    }
}

fn with_shared_plain_text<R>(text: &str, read: impl FnOnce(&SharedPlainText) -> R) -> R {
    use std::hash::{Hash, Hasher};

    let mut hasher = cranpose_ui_graphics::FxHasher::default();
    text.hash(&mut hasher);
    crate::thread_state::with(|state| {
        state.shared_plain_texts.borrow_mut().with(
            hasher.finish(),
            |shared| shared.annotated.text == text,
            || SharedPlainText {
                annotated: Rc::new(AnnotatedString::new(text.to_owned())),
                render: std::cell::OnceCell::new(),
            },
            read,
        )
    })
}

#[cfg(test)]
#[path = "tests/shared_plain_text_tests.rs"]
mod tests;
