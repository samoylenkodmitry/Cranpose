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
        0x0590..=0x08FF |
        0xFB1D..=0xFDFF |
        0xFE70..=0xFEFF |
        0x10800..=0x10FFF |
        0x1E800..=0x1EEFF
    )
}

#[cfg(test)]
#[path = "tests/paragraph_tests.rs"]
mod tests;
