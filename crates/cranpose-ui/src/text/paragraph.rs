use crate::text::unit::TextUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Unspecified, // Kotlin distinction
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

pub fn resolve_text_direction(
    text: &str,
    text_direction: Option<TextDirection>,
) -> ResolvedTextDirection {
    text_direction.unwrap_or_default().resolve(text)
}

fn resolve_content_direction(text: &str, fallback: ResolvedTextDirection) -> ResolvedTextDirection {
    for ch in text.chars() {
        if is_rtl_char(ch) {
            return ResolvedTextDirection::Rtl;
        }
        if ch.is_alphabetic() {
            return ResolvedTextDirection::Ltr;
        }
    }
    fallback
}

fn is_rtl_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0590..=0x08FF | // Hebrew, Arabic, Syriac, Thaana, NKo, Samaritan, Mandaic, Arabic ext
        0xFB1D..=0xFDFF | // Hebrew/Arabic presentation forms
        0xFE70..=0xFEFF | // Arabic presentation forms B
        0x10800..=0x10FFF | // Cypriot, Imperial Aramaic and other RTL historical scripts
        0x1E800..=0x1EEFF // Adlam and Arabic mathematical alphabetic symbols
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_content_direction_detects_rtl_script() {
        assert_eq!(
            TextDirection::Content.resolve("שלום"),
            ResolvedTextDirection::Rtl
        );
    }

    #[test]
    fn resolve_content_direction_detects_ltr_script() {
        assert_eq!(
            TextDirection::Content.resolve("Compose"),
            ResolvedTextDirection::Ltr
        );
    }

    #[test]
    fn resolve_content_or_rtl_falls_back_to_rtl() {
        assert_eq!(
            TextDirection::ContentOrRtl.resolve("12345"),
            ResolvedTextDirection::Rtl
        );
    }

    #[test]
    fn resolve_text_direction_defaults_to_ltr_for_unspecified() {
        assert_eq!(
            resolve_text_direction("12345", None),
            ResolvedTextDirection::Ltr
        );
    }

    #[test]
    fn resolve_text_direction_uses_content_for_unspecified() {
        assert_eq!(
            resolve_text_direction("שלום", Some(TextDirection::Unspecified)),
            ResolvedTextDirection::Rtl
        );
    }

    #[test]
    fn line_break_take_or_else_uses_fallback_for_unspecified() {
        let value = LineBreak::Unspecified.take_or_else(|| LineBreak::Simple);
        assert_eq!(value, LineBreak::Simple);
        assert!(LineBreak::Simple.is_specified());
    }

    #[test]
    fn hyphens_take_or_else_uses_fallback_for_unspecified() {
        let value = Hyphens::Unspecified.take_or_else(|| Hyphens::None);
        assert_eq!(value, Hyphens::None);
        assert!(Hyphens::Auto.is_specified());
    }
}
