//! Compose's `InputTransformation`: a text field's filter on the edits a
//! person makes.

use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use super::{TextFieldBuffer, TextFieldValue, TextRange};

/// Changes or rejects each edit a person makes in a text field, as
/// Compose's `InputTransformation`: typing, pasting, deleting, cutting and
/// IME composition. Edits the app makes through
/// [`TextFieldState::edit`](super::TextFieldState::edit) pass unchanged, and
/// edits that only move the cursor or selection skip it.
///
/// A transformation gets the value before the edit and the buffer after it,
/// and changes the buffer. It must not edit the field's state itself.
#[derive(Clone)]
pub struct InputTransformation(Rc<Transform>);

type Transform = dyn Fn(&TextFieldValue, &mut TextFieldBuffer);

impl InputTransformation {
    pub fn new(transform: impl Fn(&TextFieldValue, &mut TextFieldBuffer) + 'static) -> Self {
        Self(Rc::new(transform))
    }

    /// Rejects an edit that leaves more than `max` characters (extended
    /// grapheme clusters), as Compose's `InputTransformation.maxLength`
    /// does, unless the edit shortens the text: a field the app filled past
    /// the limit can still be deleted from.
    pub fn max_length(max: usize) -> Self {
        Self::new(move |original, buffer| {
            if buffer.len() >= original.text.len()
                && buffer.text().graphemes(true).nth(max).is_some()
            {
                buffer.reset_to(original);
            }
        })
    }

    /// Accepts an edit unless `filter` gives a replacement for the text it
    /// proposes: Compose's `InputTransformation.byValue`. `filter` gets the
    /// text before the edit and the proposed text. Returning the text before
    /// the edit rejects the edit; another replacement puts the cursor at its
    /// end.
    pub fn by_value(filter: impl Fn(&str, &str) -> Option<String> + 'static) -> Self {
        Self::new(move |original, buffer| {
            let Some(replacement) = filter(&original.text, buffer.text()) else {
                return;
            };
            if replacement == original.text {
                buffer.reset_to(original);
            } else {
                buffer.replace(TextRange::all(buffer.len()), &replacement);
            }
        })
    }

    /// This transformation, then `next` on what it leaves: Compose's `then`.
    pub fn then(self, next: Self) -> Self {
        Self::new(move |original, buffer| {
            (self.0)(original, buffer);
            (next.0)(original, buffer);
        })
    }

    pub(crate) fn transform(&self, original: &TextFieldValue, buffer: &mut TextFieldBuffer) {
        (self.0)(original, buffer);
    }
}

impl std::fmt::Debug for InputTransformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InputTransformation")
    }
}

impl PartialEq for InputTransformation {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
