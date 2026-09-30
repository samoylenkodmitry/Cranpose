use icu_properties::{CodePointMapData, props::BidiClass};

use crate::text::unit::TextUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Unspecified,
    Left,
    Right,
    Center,
    Justify,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextDirection {
    #[default]
    Unspecified,
    Ltr,
    Rtl,
    Content,
    ContentOrLtr,
    ContentOrRtl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineBreak {
    #[default]
    Unspecified,
    Simple,
    Paragraph,
    Heading,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Hyphens {
    #[default]
    Unspecified,
    None,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextIndent {
    pub first_line: TextUnit,
    pub rest_line: TextUnit,
}

impl Default for TextIndent {
    fn default() -> Self {
        Self {
            first_line: TextUnit::Unspecified,
            rest_line: TextUnit::Unspecified,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ResolvedTextDirection {
    #[default]
    Ltr,
    Rtl,
}

impl TextDirection {
    /// Resolves content-based directions from the first strong Unicode bidi
    /// character outside directional isolates. Text without a strong character
    /// uses the variant's fallback, which is LTR except for `ContentOrRtl`.
    pub fn resolve(self, text: &str) -> ResolvedTextDirection {
        match self {
            TextDirection::Ltr => ResolvedTextDirection::Ltr,
            TextDirection::Rtl => ResolvedTextDirection::Rtl,
            TextDirection::Content | TextDirection::ContentOrLtr => {
                resolve_content_direction(text, ResolvedTextDirection::Ltr)
            }
            TextDirection::ContentOrRtl => {
                resolve_content_direction(text, ResolvedTextDirection::Rtl)
            }
            TextDirection::Unspecified => {
                resolve_content_direction(text, ResolvedTextDirection::Ltr)
            }
        }
    }
}

impl LineBreak {
    pub fn is_specified(self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    pub fn take_or_else(self, fallback: impl FnOnce() -> LineBreak) -> LineBreak {
        if self.is_specified() {
            self
        } else {
            fallback()
        }
    }
}

impl Hyphens {
    pub fn is_specified(self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    pub fn take_or_else(self, fallback: impl FnOnce() -> Hyphens) -> Hyphens {
        if self.is_specified() {
            self
        } else {
            fallback()
        }
    }
}

/// Resolves a paragraph's direction, using content with an LTR fallback when
/// no direction is specified.
pub fn resolve_text_direction(
    text: &str,
    text_direction: Option<TextDirection>,
) -> ResolvedTextDirection {
    text_direction.unwrap_or_default().resolve(text)
}

fn resolve_content_direction(text: &str, fallback: ResolvedTextDirection) -> ResolvedTextDirection {
    for (index, byte) in text.bytes().enumerate() {
        if byte.is_ascii_alphabetic() {
            return ResolvedTextDirection::Ltr;
        }
        if !byte.is_ascii() {
            return resolve_unicode_content_direction(&text[index..], fallback);
        }
    }
    fallback
}

fn resolve_unicode_content_direction(
    text: &str,
    fallback: ResolvedTextDirection,
) -> ResolvedTextDirection {
    let classes = CodePointMapData::<BidiClass>::new();
    let mut isolate_depth = 0usize;
    for ch in text.chars() {
        match ch {
            '\u{2066}'..='\u{2068}' => isolate_depth += 1,
            '\u{2069}' => isolate_depth = isolate_depth.saturating_sub(1),
            _ if isolate_depth == 0 => match classes.get(ch) {
                BidiClass::LeftToRight => return ResolvedTextDirection::Ltr,
                BidiClass::RightToLeft | BidiClass::ArabicLetter => {
                    return ResolvedTextDirection::Rtl;
                }
                _ => {}
            },
            _ => {}
        }
    }
    fallback
}

#[cfg(test)]
#[path = "tests/paragraph_tests.rs"]
mod tests;
