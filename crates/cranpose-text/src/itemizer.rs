use std::ops::Range;

use crate::{
    bidi_level::{BidiLevel, level_runs},
    font_family_resolver::{FontFamilyResolver, FontRequest},
    paragraph::{ResolvedTextDirection, TextDirection},
    script::{Script, script_runs},
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
    /// Passes the items of `text` to `visit`. `spans` are the style spans of
    /// the text in order, each a byte range and the font it asks for; they
    /// cover the text with no gap.
    pub fn itemize<'r>(
        &mut self,
        text: &str,
        direction: TextDirection,
        spans: &[(Range<usize>, FontRequest<'_>)],
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
        spans: &[(Range<usize>, FontRequest<'_>)],
        resolver: &'r FontFamilyResolver,
        visit: &mut impl FnMut(TextItem<'r>),
    ) {
        let content = &text[paragraph.clone()];
        let base = match direction.resolve(content) {
            ResolvedTextDirection::Ltr => BidiLevel::LTR,
            ResolvedTextDirection::Rtl => BidiLevel::RTL,
        };
        self.levels.clear();
        level_runs(content, base, paragraph.start, &mut self.levels);
        self.scripts.clear();
        script_runs(content, paragraph.start, &mut self.scripts);
        self.fonts.clear();
        for (span, (range, request)) in spans.iter().enumerate() {
            let start = range.start.max(paragraph.start);
            let end = range.end.min(paragraph.end);
            if start < end {
                resolver.typeface_runs(&text[start..end], *request, |run, typeface| {
                    self.fonts.push((start + run.end, typeface, span));
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
// separator), or the end of `text`.
fn paragraph_end(text: &str, start: usize) -> usize {
    let rest = &text[start..];
    rest.char_indices()
        .find(|&(_, ch)| {
            matches!(
                ch,
                '\n' | '\r' | '\u{1C}'..='\u{1E}' | '\u{85}' | '\u{2029}'
            )
        })
        .map_or(text.len(), |(at, ch)| {
            let after = start + at + ch.len_utf8();
            if ch == '\r' && text[after..].starts_with('\n') {
                after + 1
            } else {
                after
            }
        })
}
