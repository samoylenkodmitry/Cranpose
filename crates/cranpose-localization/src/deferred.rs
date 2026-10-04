use std::rc::Rc;

use crate::{Argument, LocalizationError, Message, Translator};

/// A message captured outside composition and translated when displayed.
/// Borrowed arguments become owned; cloning a request shares their storage.
/// Use `message!` to construct one with checked source text and named arguments.
#[derive(Clone)]
pub struct DeferredMessage {
    message: &'static Message,
    arguments: Option<Rc<[Argument<'static>]>>,
}

impl DeferredMessage {
    /// Captures arguments for a checked static message without formatting it.
    pub fn new<const N: usize>(message: &'static Message, arguments: [Argument<'_>; N]) -> Self {
        Self {
            message,
            arguments: (N != 0).then(|| {
                arguments
                    .into_iter()
                    .map(|mut argument| argument.take_owned())
                    .collect()
            }),
        }
    }

    /// Formats with the current application translator, or the source language.
    pub fn format(&self, translator: Option<&Translator>) -> Result<String, LocalizationError> {
        let arguments = self.arguments.as_deref().unwrap_or_default();
        match translator {
            Some(translator) => translator.format(self.message, arguments),
            None => self.message.format_source(arguments),
        }
    }

    /// Readable source text when a malformed runtime catalog cannot be formatted.
    pub fn fallback_text(&self) -> String {
        self.message
            .fallback_text(self.arguments.as_deref().unwrap_or_default())
    }
}

impl PartialEq for DeferredMessage {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.message, other.message) && self.arguments == other.arguments
    }
}

impl std::fmt::Debug for DeferredMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeferredMessage")
            .field("namespace", &self.message.namespace())
            .field("id", &self.message.id())
            .field("arguments", &self.arguments)
            .finish()
    }
}
