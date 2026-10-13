//! What a text field shows: its text after its output transformation, and
//! how offsets map between the two, built once per change of either.

use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::text::{OffsetMapping, OutputTransformation, TextFieldState, TextRange};

use crate::text::AnnotatedString;

/// A field's text as shown. Layout, drawing and hit tests work in offsets
/// of the shown text; editing works in offsets of the text.
pub(crate) struct FieldPresentation {
    pub(crate) shown: Rc<AnnotatedString>,
    /// `None` shows the text as it is.
    mapping: Option<OffsetMapping>,
}

impl FieldPresentation {
    pub(crate) fn text(&self) -> &str {
        &self.shown.text
    }

    /// Where an offset of the text is shown.
    pub(crate) fn to_shown(&self, offset: usize) -> usize {
        self.mapping
            .as_ref()
            .map_or(offset, |mapping| mapping.to_shown(offset))
            .min(self.shown.text.len())
    }

    /// The offset of the text a shown offset stands for.
    pub(crate) fn to_source(&self, offset: usize) -> usize {
        self.mapping
            .as_ref()
            .map_or(offset, |mapping| mapping.to_source(offset))
    }

    /// The shown bounds of a range of the text, lower first.
    pub(crate) fn range_to_shown(&self, range: TextRange) -> (usize, usize) {
        (self.to_shown(range.min()), self.to_shown(range.max()))
    }
}

/// Keeps a field's presentation until its state, text or transformation
/// changes, so a frame reads the shown text without copying it.
#[derive(Default)]
pub(crate) struct PresentationCache {
    transformation: RefCell<Option<OutputTransformation>>,
    entry: RefCell<Option<CachedPresentation>>,
}

struct CachedPresentation {
    state: TextFieldState,
    revision: u64,
    presentation: Rc<FieldPresentation>,
}

impl PresentationCache {
    /// Sets the field's transformation, which its composition gives it.
    /// Answers whether it changed.
    pub(crate) fn set_transformation(&self, transformation: Option<OutputTransformation>) -> bool {
        if *self.transformation.borrow() == transformation {
            return false;
        }
        *self.transformation.borrow_mut() = transformation;
        self.entry.borrow_mut().take();
        true
    }

    pub(crate) fn get(&self, state: TextFieldState) -> Rc<FieldPresentation> {
        let revision = state.text_revision();
        if let Some(cached) = self.entry.borrow().as_ref()
            && cached.state == state
            && cached.revision == revision
        {
            return Rc::clone(&cached.presentation);
        }
        let transformation = self.transformation.borrow();
        let presentation = Rc::new(state.with_text(|text| match transformation.as_ref() {
            None => FieldPresentation {
                shown: Rc::new(AnnotatedString::from(text)),
                mapping: None,
            },
            Some(transformation) => {
                let shown = transformation.apply(text);
                FieldPresentation {
                    shown: Rc::new(AnnotatedString::from(shown.text)),
                    mapping: Some(shown.mapping),
                }
            }
        }));
        drop(transformation);
        *self.entry.borrow_mut() = Some(CachedPresentation {
            state,
            revision,
            presentation: Rc::clone(&presentation),
        });
        presentation
    }
}
