// Where the paragraph's words lie on its sheet: the same layout its texts
// take, worked out from the same text measures, so the boxes ruled round
// the lines and words of the flat copy, the word a pointer points at and the
// routes along the lines all fall where the words show.

use cranpose_ui::{
    text::{
        get_cursor_x_for_offset, wrapped_line_ranges, AnnotatedString, TextLayoutOptions, TextUnit,
    },
    Color, Rect, TextStyle,
};

use super::lettering::{headline, strong, text};

pub(super) const HEADING: &str = "One paragraph,\nshown twice";
pub(super) const BODY: &str =
    "Both copies are the same paragraph. Select a word on one and it is selected on the other.";
pub(super) const EDIT_LINE: &str = "You can edit this line.";

// One of the sheet's three texts, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Block {
    Heading,
    Body,
    Edit,
}

impl Block {
    pub(super) const ALL: [Self; 3] = [Self::Heading, Self::Body, Self::Edit];

    // The block's text style in `ink`, at unit `u`.
    pub(super) fn style(self, u: f32, ink: Color) -> TextStyle {
        let size = self.font_size(u);
        let mut style = match self {
            Self::Heading => headline(size, ink),
            Self::Body => text(size, ink),
            Self::Edit => strong(size, ink),
        };
        style.paragraph_style.line_height = TextUnit::Sp(self.line_height(u));
        style
    }

    fn font_size(self, u: f32) -> f32 {
        match self {
            Self::Heading => 72.0 * u,
            Self::Body | Self::Edit => 34.0 * u,
        }
    }

    fn line_height(self, u: f32) -> f32 {
        match self {
            Self::Heading => 72.0 * u * 1.07,
            Self::Body | Self::Edit => 34.0 * u * 1.36,
        }
    }
}

// The sheet's layout: its size, where each text starts, and the box of
// each line and word, in sheet pixels from its top left corner.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SheetLayout {
    pub u: f32,
    // The left edge of every text.
    pub left: f32,
    // The width the texts wrap at.
    pub width: f32,
    // The top of each block, as [`Block::ALL`] orders them.
    pub tops: [f32; 3],
    pub lines: Vec<Rect>,
    pub words: Vec<Rect>,
    // The cobalt rule under the line that can be edited.
    pub underline: Rect,
}

impl SheetLayout {
    // The layout of a sheet `sheet_height` tall at unit `u` whose
    // editable line reads `edit`.
    pub(super) fn new(u: f32, sheet_height: f32, edit: &str) -> Self {
        let (left, width, gap) = (56.0 * u, 622.0 * u, 22.0 * u);
        let rule = (3.4 + 3.0) * u;
        let texts = [HEADING, BODY, edit];
        let mut layout = Self {
            u,
            left,
            width,
            tops: [0.0; 3],
            lines: Vec::new(),
            words: Vec::new(),
            underline: Rect::default(),
        };
        let ranges = Block::ALL.map(|block| {
            let index = block as usize;
            wrapped_line_ranges(
                &AnnotatedString::from(texts[index]),
                &block.style(u, Color::BLACK),
                TextLayoutOptions::default(),
                Some(width),
            )
        });
        let heights = Block::ALL.map(|block| {
            let lines = ranges[block as usize].len().max(1) as f32;
            lines * block.line_height(u) + if block == Block::Edit { rule } else { 0.0 }
        });
        let total = heights.iter().sum::<f32>() + gap * 2.0;
        let mut top = (sheet_height - total) * 0.5;
        for block in Block::ALL {
            let index = block as usize;
            layout.tops[index] = top;
            layout.measure(block, texts[index], &ranges[index], top);
            top += heights[index] + gap;
        }
        let edit_line = layout.lines.last().copied().unwrap_or_default();
        layout.underline = Rect {
            x: left,
            y: layout.tops[2] + Block::Edit.line_height(u) + 3.4 * u,
            width: edit_line.x + edit_line.width - left,
            height: 3.0 * u,
        };
        layout
    }

    fn measure(&mut self, block: Block, text: &str, ranges: &[std::ops::Range<usize>], top: f32) {
        let u = self.u;
        let style = block.style(u, Color::BLACK);
        let line_height = block.line_height(u);
        let height = (line_height * 0.95).min(block.font_size(u) * 1.14);
        for (row, range) in ranges.iter().enumerate() {
            let line = &text[range.clone()];
            let shaped = AnnotatedString::from(line);
            let y = top + row as f32 * line_height + (line_height - height) * 0.5;
            let first = self.words.len();
            let mut start = None;
            for (at, character) in line.char_indices().chain([(line.len(), ' ')]) {
                match (character.is_whitespace(), start) {
                    (false, None) => start = Some(at),
                    (true, Some(from)) => {
                        let x = get_cursor_x_for_offset(&shaped, &style, from);
                        let to = get_cursor_x_for_offset(&shaped, &style, at);
                        self.words.push(Rect {
                            x: self.left + x,
                            y,
                            width: to - x,
                            height,
                        });
                        start = None;
                    }
                    _ => {}
                }
            }
            if let (Some(head), Some(tail)) = (self.words.get(first), self.words.last()) {
                self.lines.push(Rect {
                    x: head.x,
                    y,
                    width: tail.x + tail.width - head.x,
                    height,
                });
            }
        }
    }

    // The word nearest `(x, y)`, within twelve units, or `None`.
    pub(super) fn word_near(&self, x: f32, y: f32) -> Option<Rect> {
        let mut nearest = 12.0 * self.u;
        let mut found = None;
        for word in &self.words {
            let dx = (word.x - x).max(x - word.x - word.width).max(0.0);
            let dy = (word.y - y).max(y - word.y - word.height).max(0.0);
            if dx + dy < nearest {
                nearest = dx + dy;
                found = Some(*word);
            }
        }
        found
    }
}
