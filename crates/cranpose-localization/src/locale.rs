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
    /// Parses a language tag such as `sr-Latn` or `ar-EG`.
    /// Valid BCP 47 extensions and private-use subtags are ignored for catalog selection.
    pub fn parse(tag: &str) -> Result<Self, LocalizationError> {
        let invalid = || LocalizationError::InvalidLocale(tag.to_owned());
        let language = language_identifier(tag)
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
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

fn language_identifier(tag: &str) -> Option<&str> {
    let mut end = tag.len();
    let mut offset = 0;
    let mut extension = false;
    let mut private = false;
    let mut needs_value = false;
    let mut seen = 0u64;
    for part in tag.split('-') {
        if !extension && offset != 0 && part.len() == 1 {
            end = offset - 1;
            extension = true;
        }
        if extension {
            if part.len() == 1 && !private {
                let byte = part.as_bytes()[0].to_ascii_lowercase();
                let index = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'z' => byte - b'a' + 10,
                    _ => return None,
                };
                let flag = 1u64 << index;
                if needs_value || seen & flag != 0 {
                    return None;
                }
                seen |= flag;
                private = byte == b'x';
                needs_value = true;
            } else {
                let minimum = if private { 1 } else { 2 };
                if !(minimum..=8).contains(&part.len())
                    || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
                {
                    return None;
                }
                needs_value = false;
            }
        }
        offset += part.len() + 1;
    }
    (!needs_value).then_some(&tag[..end])
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
