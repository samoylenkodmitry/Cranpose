use icu_properties::{CodePointMapData, props::BidiClass};

use crate::text::unit::TextUnit;

/// Horizontal alignment of the lines of a paragraph within its width.
/// Compose's `TextAlign`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    /// No alignment is set. The value comes from an enclosing style and acts like `Start`
    /// when no style sets it.
    #[default]
    Unspecified,
    /// Aligns lines to the left edge in both text directions.
    Left,
    /// Aligns lines to the right edge in both text directions.
    Right,
    /// Centers each line within the paragraph width.
    Center,
    /// Justified alignment. Lines are not stretched yet; they align like `Start`.
    Justify,
    /// Aligns lines to the start edge: left for left-to-right text, right otherwise.
    Start,
    /// Aligns lines to the end edge: right for left-to-right text, left otherwise.
    End,
}

/// Base direction of a paragraph. Compose's `TextDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextDirection {
    /// No direction is set. The value comes from an enclosing style and acts like `Content`
    /// when no style sets it.
    #[default]
    Unspecified,
    /// Left-to-right.
    Ltr,
    /// Right-to-left.
    Rtl,
    /// Taken from the first strong directional character of the text, left-to-right if the
    /// text has none.
    Content,
    /// Like `Content`, with left-to-right as the fallback.
    ContentOrLtr,
    /// Like `Content`, with right-to-left as the fallback.
    ContentOrRtl,
}

/// How a paragraph is broken into lines. Compose's `LineBreak`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineBreak {
    /// No strategy is set. The value comes from an enclosing style and acts like `Simple`
    /// when no style sets it.
    #[default]
    Unspecified,
    /// Fills each line with as many words as fit. Fast, and the wrap changes little while
    /// the user edits text.
    Simple,
    /// Spreads the words over the lines to even out their lengths, leaving the last line
    /// free to stay short. Costs more than `Simple`.
    Paragraph,
    /// Spreads the words over all lines, the last one included, for short text such as
    /// titles. Costs more than `Simple`.
    Heading,
}

/// Whether words may be split with a hyphen at the end of a line. Compose's `Hyphens`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Hyphens {
    /// No setting. The value comes from an enclosing style and acts like `None` when no
    /// style sets it.
    #[default]
    Unspecified,
    /// Words are never split with a hyphen.
    None,
    /// Words are split at hyphenation points of the text's locale when it makes a line fit.
    Auto,
}

/// Indentation of the lines of a paragraph. Compose's `TextIndent`. Not yet applied by
/// text layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextIndent {
    /// Indent of the first line, as a `TextUnit`.
    pub first_line: TextUnit,
    /// Indent of every line after the first, as a `TextUnit`.
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

/// A text direction with the content-based choices resolved, as used for layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ResolvedTextDirection {
    /// Left-to-right.
    #[default]
    Ltr,
    /// Right-to-left.
    Rtl,
}

impl TextDirection {
    /// Resolves content-based directions from the first strong Unicode bidi
    /// character outside directional isolates. Text without a strong character
    /// uses the variant's fallback, which is LTR except for `ContentOrRtl`.
    pub fn resolve(self, text: &str) -> ResolvedTextDirection {
        match self {
            Self::Ltr => ResolvedTextDirection::Ltr,
            Self::Rtl => ResolvedTextDirection::Rtl,
            Self::Content | Self::ContentOrLtr => {
                resolve_content_direction(text, ResolvedTextDirection::Ltr)
            }
            Self::ContentOrRtl => resolve_content_direction(text, ResolvedTextDirection::Rtl),
            Self::Unspecified => resolve_content_direction(text, ResolvedTextDirection::Ltr),
        }
    }
}

macro_rules! impl_unspecified_fallback {
    ($($choice:ty),*) => {$(
        impl $choice {
            /// Returns true unless the value is `Unspecified`.
            pub fn is_specified(self) -> bool {
                !matches!(self, Self::Unspecified)
            }

            /// Returns `self` when it is specified, otherwise the value returned by `fallback`.
            pub fn take_or_else(self, fallback: impl FnOnce() -> Self) -> Self {
                if self.is_specified() {
                    self
                } else {
                    fallback()
                }
            }
        }
    )*};
}

impl_unspecified_fallback!(LineBreak, Hyphens);

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
