use cranpose_text::{
    FontFamilyResolver, LineBreak, ParagraphLengths, ParagraphStyle, TextAlign, TextLayout,
    TextLayoutOptions, TextOverflow,
};

use crate::fonts::{ROBOTO, lay_out_between, lay_out_with, typeface};

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
        if !laid.holds(width, width) {
            return (held, Some(width));
        }
        laid.align(width, width, paragraph.text_align);
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
fn a_line_that_fits_by_an_ellipsis_it_does_not_show_holds_no_narrower_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Start, LineBreak::Simple);
    let options = TextLayoutOptions {
        soft_wrap: false,
        overflow: TextOverflow::Ellipsis,
        ..TextLayoutOptions::default()
    };

    let laid = lay_out_with(
        &resolver,
        "Tea",
        (&style, lengths(0.0, 0.0)),
        200.0,
        options,
    );
    let fits = laid.line_right(0);

    assert!(!laid.is_line_ellipsized(0));
    assert!(laid.holds(0.0, fits + 1.0));
    assert!(!laid.holds(0.0, fits - 1.0));
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

#[test]
fn a_layout_aligned_without_moving_a_glyph_keeps_its_id() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let start = paragraph(TextAlign::Start, LineBreak::Simple);
    let mut laid = lay_out_with(
        &resolver,
        "hello world",
        (&start, ParagraphLengths::default()),
        300.0,
        TextLayoutOptions::default(),
    );
    let id = laid.id();

    laid.align(250.0, 250.0, TextAlign::Start);
    assert_eq!(laid.id(), id, "a start-aligned line stays where it was");
    let at_start = picture(&laid);

    laid.align(250.0, 250.0, TextAlign::Center);
    assert_ne!(laid.id(), id, "centring moves the line");
    let centred = laid.id();

    laid.align(250.0, 250.0, TextAlign::Start);
    assert_ne!(laid.id(), centred, "the start takes the line back");
    assert_eq!(picture(&laid), at_start);
}

// A text cut to its max lines shows no line past them, so the lines past
// them hold no width: a layout keeps the widths at which what it shows
// stays, as an animated width asks of a card body each frame. A clipped
// layout stops holding only where a shown line breaks elsewhere.
#[test]
fn lines_past_the_max_lines_hold_no_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let style = paragraph(TextAlign::Start, LineBreak::Simple);
    let lengths = ParagraphLengths::default();
    for overflow in [TextOverflow::Clip, TextOverflow::Ellipsis] {
        let options = TextLayoutOptions {
            overflow,
            max_lines: 2,
            ..TextLayoutOptions::default()
        };
        let shown = |width| {
            let laid = lay_out_with(&resolver, TEXT, (&style, lengths), width, options);
            (0..laid.line_count())
                .map(|line| laid.line_start(line)..laid.line_end(line))
                .collect::<Vec<_>>()
        };
        for step in 0..40 {
            let from = 40.0 + 2.5 * step as f32;
            // Each width held lays out as a fresh layout there.
            let (_, first_not) = held_widths(
                &resolver,
                TEXT,
                (&style, lengths),
                (from, from + 30.0),
                options,
            );
            let Some(first_not) = first_not.filter(|_| overflow == TextOverflow::Clip) else {
                continue;
            };
            assert_ne!(
                shown(first_not - 0.25),
                shown(first_not),
                "from {from}: no shown line moves at {first_not}"
            );
        }
    }
}

const CARD: &str = "Ea elit veniam et laboris enim ut ipsum";

// Lays `CARD` out at `size` from no min width up to `width`, as a parent
// that lets text take its own width does.
fn lay_out_card(
    resolver: &FontFamilyResolver,
    size: f32,
    lengths: ParagraphLengths,
    width: f32,
    options: TextLayoutOptions,
) -> TextLayout {
    let style = paragraph(TextAlign::Start, LineBreak::Simple);
    lay_out_between(
        resolver,
        (CARD, size),
        (&style, lengths),
        (0.0, width),
        options,
    )
}

// Calls `each` with every size from 2 px up to 4 px in steps of `step`
// hundredths of a pixel and every width from 15 px up to 30 px past the
// first indent in steps of a third of a pixel, without and with indents, at
// density 3.
fn for_card_layouts(step: usize, mut each: impl FnMut(f32, ParagraphLengths, f32)) {
    for lengths in [ParagraphLengths::default(), lengths(12.5, 4.0)] {
        let lengths = ParagraphLengths {
            grid: 3.0,
            ..lengths
        };
        for hundredth in (200..=400).step_by(step) {
            for third in 45..=90 {
                let width = third as f32 / 3.0 + lengths.first_line_indent;
                each(hundredth as f32 / 100.0, lengths, width);
            }
        }
    }
}

// A wrapped text laid out again at its own width breaks into the same lines,
// so its layout stands for that width, as a parent that measures it there
// asks of it, at any density. The widest line of a layout can end a hair
// under its own width, closer than any margin taken around the ends of its
// hold.
#[test]
fn a_wrapped_text_holds_at_its_own_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let options = TextLayoutOptions::default();
    let mut missed = Vec::new();
    for_card_layouts(5, |size, lengths, width| {
        let mut laid = lay_out_card(&resolver, size, lengths, width, options);
        let own = laid.width();
        if laid.line_count() < 2 {
            return;
        }
        if !laid.holds(0.0, own) {
            missed.push((size, width, own));
            return;
        }
        laid.align(0.0, own, TextAlign::Start);
        let fresh = lay_out_card(&resolver, size, lengths, own, options);
        assert_eq!(picture(&laid), picture(&fresh), "{size} at {own}");
    });
    assert!(missed.is_empty(), "no hold at their own width: {missed:?}");
}

// The max widths a layout keeps end exactly where a fresh layout changes: a
// fresh layout at either end shows what it shows, and one a float past
// either end does not. A margin around them would lay the text out again at
// widths where nothing changes.
#[test]
fn the_max_widths_a_layout_keeps_end_where_a_fresh_layout_changes() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for options in [
        TextLayoutOptions::default(),
        TextLayoutOptions {
            overflow: TextOverflow::Ellipsis,
            max_lines: 2,
            ..TextLayoutOptions::default()
        },
    ] {
        for_card_layouts(20, |size, lengths, width| {
            let laid = lay_out_card(&resolver, size, lengths, width, options);
            let (low, high) = laid.max_widths_kept(0.0);
            // A layout that keeps no width but its own has no ends to test.
            if laid.line_count() < 2 || high <= low {
                return;
            }
            let shown = picture(&laid);
            let at = |width: f32| picture(&lay_out_card(&resolver, size, lengths, width, options));
            let case = || format!("{size} px at {width} with {options:?}: {low}..{high}");
            assert_eq!(at(low), shown, "{}", case());
            assert_eq!(at(high), shown, "{}", case());
            assert_ne!(at(low.next_down()), shown, "{}", case());
            assert_ne!(at(high.next_up()), shown, "{}", case());
        });
    }
}
