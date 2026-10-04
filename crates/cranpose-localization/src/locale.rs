use std::{fmt, str::FromStr};

use unic_langid::{CharacterDirection, LanguageIdentifier};

use crate::LocalizationError;

/// A visual stress test applied after message formatting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewMode {
    /// Render the selected language unchanged.
    #[default]
    None,
    /// Add accents and length to expose clipped labels.
    Expanded,
    /// Isolate text in a right-to-left paragraph and mirror layout defaults.
    Rtl,
}

/// A validated language tag and optional localization preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locale {
    pub(crate) language: LanguageIdentifier,
    pub(crate) preview: PreviewMode,
}

impl Locale {
    /// Parses a Unicode language identifier such as `sr-Latn` or `ar-EG`.
    pub fn parse(tag: &str) -> Result<Self, LocalizationError> {
        let language = tag
            .parse()
            .map_err(|_| LocalizationError::InvalidLocale(tag.to_owned()))?;
        Ok(Self {
            language,
            preview: PreviewMode::None,
        })
    }

    /// Uses this language with the selected visual stress test.
    pub fn with_preview(mut self, preview: PreviewMode) -> Self {
        self.preview = preview;
        self
    }

    /// Whether this locale's script reads from right to left.
    pub fn is_rtl(&self) -> bool {
        self.preview == PreviewMode::Rtl
            || self.language.character_direction() == CharacterDirection::RTL
    }
}

impl FromStr for Locale {
    type Err = LocalizationError;

    fn from_str(tag: &str) -> Result<Self, Self::Err> {
        Self::parse(tag)
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.language.fmt(formatter)
    }
}

pub(crate) fn preview_text(text: String, preview: PreviewMode) -> String {
    match preview {
        PreviewMode::None => text,
        PreviewMode::Rtl => format!("\u{2067}{text}\u{2069}"),
        PreviewMode::Expanded => {
            let mut output = String::with_capacity(text.len() * 2 + 4);
            output.push('[');
            for ch in text.chars() {
                output.push(match ch {
                    'a' => 'á',
                    'e' => 'ë',
                    'i' => 'ï',
                    'o' => 'ö',
                    'u' => 'ü',
                    'A' => 'Á',
                    'E' => 'Ë',
                    'I' => 'Ï',
                    'O' => 'Ö',
                    'U' => 'Ü',
                    other => other,
                });
                if ch.is_ascii_alphabetic() && "aeiouAEIOU".contains(ch) {
                    output.push('~');
                }
            }
            output.push(']');
            output
        }
    }
}
