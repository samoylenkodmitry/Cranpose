use std::ops::Range;

use crate::{
    bidi_level::{BidiLevel, level_runs},
    font_family_resolver::FontFamilyResolver,
    paragraph::{ResolvedTextDirection, TextDirection},
    script::{Script, script_runs},
    shaping_span::ShapingSpan,
    typeface::Typeface,
};

/// A stretch of text that one typeface shapes in one direction and script,
/// inside one style span.
#[derive(Clone, Debug)]
pub struct TextItem<'a> {
    /// The byte range in the text.
    pub range: Range<usize>,
    /// The bidi embedding level.
    pub level: BidiLevel,
    /// The base level of the paragraph the item is in.
    pub paragraph_level: BidiLevel,
    /// The script.
    pub script: Script,
    /// The typeface that draws it.
    pub typeface: &'a Typeface,
    /// The index of the style span it lies in.
    pub span: usize,
}

/// Splits text into the items a shaper takes, in logical order: paragraphs at
/// bidi paragraph separators, each with the direction `TextDirection` gives
/// it, then the runs of one bidi level, one script, one style span and one
/// typeface of that span. Keeps its scratch storage from one text to the next.
#[derive(Default)]
pub struct Itemizer {
    levels: Vec<(usize, BidiLevel)>,
    scripts: Vec<(usize, Script)>,
    // The end of each font run, its typeface and its span.
    fonts: Vec<(usize, usize, usize)>,
}

impl Itemizer {
    /// Passes the items of `text` to `visit`. `spans` cover the text in
    /// order with no gap.
    pub fn itemize<'r>(
        &mut self,
        text: &str,
        direction: TextDirection,
        spans: &[ShapingSpan<'_>],
        resolver: &'r FontFamilyResolver,
        mut visit: impl FnMut(TextItem<'r>),
    ) {
        let mut start = 0;
        while start < text.len() {
            let end = paragraph_end(text, start);
            self.itemize_paragraph(text, start..end, direction, spans, resolver, &mut visit);
            start = end;
        }
    }

    fn itemize_paragraph<'r>(
        &mut self,
        text: &str,
        paragraph: Range<usize>,
        direction: TextDirection,
        spans: &[ShapingSpan<'_>],
        resolver: &'r FontFamilyResolver,
        visit: &mut impl FnMut(TextItem<'r>),
    ) {
        let Some(content) = text.get(paragraph.clone()) else {
            return;
        };
        let base = match direction.resolve(content) {
            ResolvedTextDirection::Ltr => BidiLevel::LTR,
            ResolvedTextDirection::Rtl => BidiLevel::RTL,
        };
        self.levels.clear();
        level_runs(content, base, paragraph.start, &mut self.levels);
        self.scripts.clear();
        script_runs(content, paragraph.start, &mut self.scripts);
        self.fonts.clear();
        for (index, span) in spans.iter().enumerate() {
            let start = span.range.start.max(paragraph.start);
            let end = span.range.end.min(paragraph.end);
            if let Some(part) = text.get(start..end).filter(|part| !part.is_empty()) {
                resolver.typeface_runs(part, span.font, |run, typeface| {
                    self.fonts.push((start + run.end, typeface, index));
                });
            }
        }
        let (mut level, mut script, mut font) = (0, 0, 0);
        let mut start = paragraph.start;
        while let (
            Some(&(level_end, bidi)),
            Some(&(script_end, writing)),
            Some(&(font_end, typeface, span)),
        ) = (
            self.levels.get(level),
            self.scripts.get(script),
            self.fonts.get(font),
        ) {
            let end = level_end.min(script_end).min(font_end);
            visit(TextItem {
                range: start..end,
                level: bidi,
                paragraph_level: base,
                script: writing,
                typeface: &resolver.typefaces()[typeface],
                span,
            });
            level += usize::from(level_end == end);
            script += usize::from(script_end == end);
            font += usize::from(font_end == end);
            start = end;
        }
    }
}

// The end of the paragraph that starts at `start`: just past the next bidi
// paragraph separator (class B: line feed, carriage return and line feed as
// one, the information separators 1C to 1E, next line and the paragraph
// separator), or the end of `text`. Found by the first byte of each, which
// in UTF-8 starts no other character it is not part of.
fn paragraph_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut at = start;
    while let Some(found) = bytes.get(at..).and_then(|rest| {
        rest.iter()
            .position(|&byte| matches!(byte, b'\n' | b'\r' | 0x1C..=0x1E | 0xC2 | 0xE2))
    }) {
        at += found;
        let after = match &bytes[at..] {
            [b'\r', b'\n', ..] => at + 2,
            [b'\n' | b'\r' | 0x1C..=0x1E, ..] => at + 1,
            [0xC2, 0x85, ..] => at + 2,
            [0xE2, 0x80, 0xA9, ..] => at + 3,
            _ => {
                at += 1;
                continue;
            }
        };
        return after;
    }
    text.len()
}
