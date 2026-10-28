use cranpose_text::{
    FontFamilyResolver, LineBreak, ParagraphLengths, ParagraphStyle, TextAlign, TextLayout,
    TextLayoutOptions, TextOverflow,
};

use crate::fonts::{ROBOTO, lay_out_with, typeface};

const TEXT: &str = "The quick brown fox jumps over the lazy dog and runs into the woods";

// Everything a layout shows: each line's text, edges and glyph positions,
// and the layout's size and overflow.
fn picture(laid: &TextLayout) -> Vec<String> {
    let mut lines: Vec<String> = (0..laid.line_count())
        .map(|line| {
            let glyphs: Vec<_> = laid
                .glyph_runs(line)
                .flat_map(|run| run.positions().collect::<Vec<_>>())
                .collect();
            format!(
                "{}..{} {} {} {glyphs:?}",
                laid.line_start(line),
                laid.line_end(line),
                laid.line_left(line),
                laid.line_right(line)
            )
        })
        .collect();
    lines.push(format!(
        "{} {} {}",
        laid.width(),
        laid.height(),
        laid.did_overflow_width()
    ));
    lines
}

// The widths from `from` up to `to` in steps of a quarter pixel that `laid`
// holds, and the first it does not, checking that `align` at each held
// width gives the picture of a fresh layout there.
fn held_widths(
    resolver: &FontFamilyResolver,
    text: &str,
    (paragraph, lengths): (&ParagraphStyle, ParagraphLengths),
    (from, to): (f32, f32),
    options: TextLayoutOptions,
) -> (usize, Option<f32>) {
    let mut laid = lay_out_with(resolver, text, (paragraph, lengths), from, options);
    let mut held = 0;
    let mut width = from;
    while width < to {
        width += 0.25;
        if !laid.holds(0.0, width) {
            return (held, Some(width));
        }
        laid.align(0.0, width, paragraph.text_align);
        let fresh = lay_out_with(resolver, text, (paragraph, lengths), width, options);
        assert_eq!(picture(&laid), picture(&fresh), "at {width}");
        held += 1;
    }
    (held, None)
}

fn paragraph(text_align: TextAlign, line_break: LineBreak) -> ParagraphStyle {
    ParagraphStyle {
        text_align,
        line_break,
        ..ParagraphStyle::default()
    }
}

fn lengths(first: f32, rest: f32) -> ParagraphLengths {
    ParagraphLengths {
        first_line_indent: first,
        rest_line_indent: rest,
        ..ParagraphLengths::default()
    }
}

#[test]
fn a_wrapped_paragraph_holds_the_widths_that_keep_its_breaks_and_aligns_to_them_as_a_fresh_lay_out_with()
 {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for align in [
        TextAlign::Start,
        TextAlign::Center,
        TextAlign::End,
        TextAlign::Justify,
    ] {
        let style = paragraph(align, LineBreak::Simple);

        let (held, first_not) = held_widths(
            &resolver,
            TEXT,
            (&style, lengths(12.0, 4.0)),
            (150.0, 400.0),
            TextLayoutOptions::default(),
        );

        let first_not = first_not.expect("a wider width takes up a word");
        let before = lay_out_with(
            &resolver,
            TEXT,
            (&style, lengths(12.0, 4.0)),
            first_not - 0.25,
            TextLayoutOptions::default(),
        );
        let after = lay_out_with(
            &resolver,
            TEXT,
            (&style, lengths(12.0, 4.0)),
            first_not,
            TextLayoutOptions::default(),
        );
        assert!(held > 0, "{align:?}");
        assert_ne!(
            (0..before.line_count())
                .map(|line| before.line_end(line))
                .collect::<Vec<_>>(),
            (0..after.line_count())
                .map(|line| after.line_end(line))
                .collect::<Vec<_>>(),
            "{align:?} at {first_not}"
        );
    }
}

#[test]
fn an_end_ellipsis_holds_the_widths_that_cut_its_line_at_the_same_cluster() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Start, LineBreak::Simple);
    let options = TextLayoutOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: 1,
        ..TextLayoutOptions::default()
    };

    let (held, first_not) = held_widths(
        &resolver,
        TEXT,
        (&style, lengths(0.0, 0.0)),
        (100.0, 140.0),
        options,
    );

    assert!(first_not.is_some());
    assert!(held < 40, "{held}");
}

#[test]
fn text_that_does_not_wrap_holds_every_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Center, LineBreak::Simple);
    let options = TextLayoutOptions {
        soft_wrap: false,
        ..TextLayoutOptions::default()
    };

    let (held, first_not) = held_widths(
        &resolver,
        "one line\nanother line",
        (&style, lengths(0.0, 0.0)),
        (10.0, 60.0),
        options,
    );

    assert_eq!((held, first_not), (200, None));
}

#[test]
fn even_breaking_holds_no_other_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Start, LineBreak::Paragraph);

    let laid = lay_out_with(
        &resolver,
        TEXT,
        (&style, lengths(0.0, 0.0)),
        200.0,
        TextLayoutOptions::default(),
    );

    assert!(!laid.holds(0.0, 200.25));
}

#[test]
fn an_indented_line_with_an_end_ellipsis_keeps_within_the_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Start, LineBreak::Simple);
    let options = TextLayoutOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: 1,
        ..TextLayoutOptions::default()
    };

    let laid = lay_out_with(
        &resolver,
        TEXT,
        (&style, lengths(30.0, 0.0)),
        150.0,
        options,
    );

    assert!(laid.is_line_ellipsized(0));
    assert!(laid.line_right(0) <= 150.0, "{}", laid.line_right(0));
}

#[cfg(feature = "hyphenation-embedded")]
#[test]
fn hyphenated_lines_hold_the_widths_that_keep_their_hyphenation_points() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = ParagraphStyle {
        hyphens: cranpose_text::Hyphens::Auto,
        ..paragraph(TextAlign::Start, LineBreak::Simple)
    };
    let mut changes = 0;
    let mut from = 40.0;
    while from < 200.0 {
        let (held, first_not) = held_widths(
            &resolver,
            "internationalization understanding",
            (&style, lengths(0.0, 0.0)),
            (from, 200.0),
            TextLayoutOptions::default(),
        );
        let Some(first_not) = first_not else {
            break;
        };
        assert!(held < 640);
        changes += 1;
        from = first_not;
    }
    assert!(changes > 3, "{changes}");
}
