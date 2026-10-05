use crate::{
    SharedText,
    localization::{DeferredMessage, localized_message},
};

/// UI text that can retain either literal content or a deferred translation.
/// Deferred text resolves during composition and follows the nearest language provider.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UiText {
    /// No text.
    #[default]
    Empty,
    /// Literal content, such as an external error or a user-provided document title.
    Text(SharedText),
    /// A translation request whose arguments have already been captured.
    Message(DeferredMessage),
}

impl UiText {
    /// Resolves the current text during composition.
    #[track_caller]
    pub fn resolve(&self) -> SharedText {
        match self {
            Self::Empty => "".into(),
            Self::Text(text) => text.clone(),
            Self::Message(message) => localized_message(message),
        }
    }

    /// Whether no content is present, without formatting a deferred request.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty) || matches!(self, Self::Text(text) if text.is_empty())
    }

    /// Removes the text and releases any retained message arguments.
    pub fn clear(&mut self) {
        *self = Self::Empty;
    }
}

impl From<DeferredMessage> for UiText {
    fn from(message: DeferredMessage) -> Self {
        Self::Message(message)
    }
}

impl From<SharedText> for UiText {
    fn from(text: SharedText) -> Self {
        Self::Text(text)
    }
}

impl From<String> for UiText {
    fn from(text: String) -> Self {
        Self::Text(text.into())
    }
}

impl From<&'static str> for UiText {
    fn from(text: &'static str) -> Self {
        Self::Text(text.into())
    }
}

impl std::fmt::Display for UiText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => Ok(()),
            Self::Text(text) => formatter.write_str(text.as_str()),
            Self::Message(message) => formatter.write_str(&message.fallback_text()),
        }
    }
}

impl crate::widgets::text::IntoTextSource for UiText {
    #[track_caller]
    fn into_text_source(self) -> crate::widgets::text::TextSource {
        self.resolve().into_text_source()
    }
}
