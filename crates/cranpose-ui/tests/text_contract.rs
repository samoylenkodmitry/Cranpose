use cranpose_ui::{
    AppContext, PreparedTextLayout, TextLayoutOptions, TextOverflow, TextStyle,
    prepare_text_layout,
    text::{AnnotatedString, SpanStyle, TextUnit},
};

// The font size at which a character of Cranpose Test Mono, the font tests
// lay text out with, is `CHAR_WIDTH` wide and a line is 10 tall.
pub(crate) const FONT_SIZE_SP: f32 = 10.0;
pub(crate) const CHAR_WIDTH: f32 = 6.0;

pub(crate) fn contract_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE_SP),
            ..Default::default()
        },
        ..Default::default()
    }
}

// The text each line of `prepared` shows.
pub(crate) fn layout_lines(prepared: &PreparedTextLayout) -> Vec<String> {
    let layout = &prepared.layout;
    (0..layout.line_count())
        .map(|line| {
            prepared.text.text[layout.line_start(line)..layout.line_visible_end(line)].to_owned()
        })
        .collect()
}

// The lines `text` wraps into at `max_chars` characters.
pub(crate) fn wrapped_lines(text: &str, max_chars: usize) -> Vec<String> {
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };
    AppContext::new().enter(|| {
        layout_lines(&prepare_text_layout(
            &AnnotatedString::from(text),
            &contract_style(),
            options,
            Some(max_chars as f32 * CHAR_WIDTH),
        ))
    })
}

// The glyphs of `line` of `prepared` as text, left to right: each glyph's
// character, `…` for an ellipsis, with the spaces that hang past the line's
// end left out. Only the runs `keep` lets through count.
fn drawn_line(
    prepared: &PreparedTextLayout,
    line: usize,
    keep: impl Fn(&cranpose_text::GlyphRun<'_>) -> bool,
) -> String {
    let text = &prepared.text.text;
    let drawn: String = prepared
        .layout
        .glyph_runs(line)
        .filter(|run| keep(run))
        .flat_map(|run| {
            let ellipsis = run.typeface.glyph('\u{2026}');
            run.positions()
                .map(move |glyph| {
                    if Some(glyph.id) == ellipsis {
                        '\u{2026}'
                    } else {
                        text[glyph.offset..].chars().next().unwrap_or('\u{fffd}')
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    drawn.trim_end().to_owned()
}

// The text each line of `prepared` draws: see `drawn_line`.
pub(crate) fn drawn_lines(prepared: &PreparedTextLayout) -> Vec<String> {
    (0..prepared.layout.line_count())
        .map(|line| drawn_line(prepared, line, |_| true))
        .collect()
}

// The stretches of text the lines of `prepared` draw at `font_size_sp`
// between their ellipses.
pub(crate) fn drawn_at_size(prepared: &PreparedTextLayout, font_size_sp: f32) -> Vec<String> {
    (0..prepared.layout.line_count())
        .flat_map(|line| {
            drawn_line(prepared, line, |run| run.size == font_size_sp)
                .split('\u{2026}')
                .filter(|stretch| !stretch.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}
