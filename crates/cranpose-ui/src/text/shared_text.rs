use std::{fmt, ops::Deref};

use super::AnnotatedString;
use crate::widgets::text::{IntoTextSource, StaticText, TextSource};

/// Immutable text shared by widgets, localized results, and retained control labels.
#[derive(Clone, Debug)]
pub struct SharedText(Storage);

#[derive(Clone, Debug)]
enum Storage {
    Static(&'static str),
    Annotated(AnnotatedString),
}

impl SharedText {
    /// The plain text used by accessibility and native string APIs.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Storage::Static(text) => text,
            Storage::Annotated(text) => text.text(),
        }
    }
}

impl PartialEq for SharedText {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Storage::Static(a), Storage::Static(b)) => a == b,
            (Storage::Annotated(a), Storage::Annotated(b)) => a == b,
            (Storage::Static(plain), Storage::Annotated(annotated))
            | (Storage::Annotated(annotated), Storage::Static(plain)) => annotated.is_plain(plain),
        }
    }
}

impl From<String> for SharedText {
    fn from(value: String) -> Self {
        Self(Storage::Annotated(AnnotatedString::from(value)))
    }
}

impl From<&'static str> for SharedText {
    fn from(value: &'static str) -> Self {
        Self(Storage::Static(value))
    }
}

impl From<AnnotatedString> for SharedText {
    fn from(value: AnnotatedString) -> Self {
        Self(Storage::Annotated(value))
    }
}

impl From<SharedText> for String {
    fn from(value: SharedText) -> Self {
        match value.0 {
            Storage::Static(text) => text.to_owned(),
            Storage::Annotated(annotated) => annotated.text().to_owned(),
        }
    }
}

impl Deref for SharedText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SharedText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl IntoTextSource for SharedText {
    fn into_text_source(self) -> TextSource {
        match self.0 {
            Storage::Static(text) => text.into_text_source(),
            Storage::Annotated(text) => TextSource::Static(text),
        }
    }

    fn static_text(&self) -> Option<StaticText<'_>> {
        Some(match &self.0 {
            Storage::Static(text) => StaticText::Plain(text),
            Storage::Annotated(text) => StaticText::Annotated(text),
        })
    }
}
