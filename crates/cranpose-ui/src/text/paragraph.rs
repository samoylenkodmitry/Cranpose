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
