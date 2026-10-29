use cranpose_text::{
    CaretDirection, FontFamily, FontFamilyResolver, LayoutInput, ParagraphLengths, ParagraphStyle,
    ShapingSpan, TextAlign, TextDirection, TextLayout, TextLayoutOptions, TextLayouter,
    TextOverflow,
};

use crate::fonts::{ARABIC_VF, DEVANAGARI_VF, ROBOTO, lay_out, span, typeface};

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    direction: TextDirection,
    align: TextAlign,
    width: f32,
) -> TextLayout {
    let paragraph = ParagraphStyle {
        text_direction: direction,
        text_align: align,
        ..ParagraphStyle::default()
    };
    lay_out(
        resolver,
        text,
        &paragraph,
        width,
        TextLayoutOptions::default(),
    )
}

fn unwrapped(resolver: &FontFamilyResolver, text: &str) -> TextLayout {
    layout(
        resolver,
        text,
        TextDirection::Content,
        TextAlign::Start,
        f32::INFINITY,
    )
}

fn lines(layout: &TextLayout, text: &str) -> Vec<String> {
    (0..layout.line_count())
        .map(|line| {
            text.get(layout.line_start(line)..layout.line_end(line))
                .expect("lines end on character boundaries")
                .to_owned()
        })
        .collect()
}

#[test]
fn lines_wrap_after_the_last_word_that_fits() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    // Letters of one byte each, and of two.
    for (first, second) in [
        ("The quick", "brown fox"),
        ("Быстрая бурая", "лиса прыгает"),
    ] {
        let text = format!("{first} {second}");
        let width = unwrapped(&resolver, first)
            .width()
            .max(unwrapped(&resolver, second).width())
            + 1.0;

        let wrapped = layout(
            &resolver,
            &text,
            TextDirection::Ltr,
            TextAlign::Start,
            width,
        );

        assert_eq!(lines(&wrapped, &text), [format!("{first} "), second.into()]);
        assert_eq!(wrapped.line_visible_end(0), first.len());
        assert!(wrapped.line_right(0) <= width);
    }
}

#[test]
fn hard_breaks_end_lines_and_a_final_break_adds_an_empty_line() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    assert_eq!(lines(&unwrapped(&resolver, "a\nb"), "a\nb"), ["a\n", "b"]);
    assert_eq!(lines(&unwrapped(&resolver, "a\n"), "a\n"), ["a\n", ""]);
    assert_eq!(lines(&unwrapped(&resolver, ""), ""), [""]);
    let empty = unwrapped(&resolver, "");
    assert!(
        empty.height() > 0.0,
        "an empty text keeps one line's height"
    );
}

#[test]
fn each_hard_break_ends_a_line() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    // At each place in a word of eight bytes and after it, behind a tab, a
    // degree sign and a quote mark, whose first bytes may start one.
    for separator in [
        "\n", "\r", "\r\n", "\u{0B}", "\u{0C}", "\u{85}", "\u{2028}", "\u{2029}",
    ] {
        for before in 0..18 {
            let text = format!("\t\u{B0}\u{2019}{}{separator}b", "a".repeat(before));

            assert_eq!(
                lines(&unwrapped(&resolver, &text), &text),
                [&text[..text.len() - 1], "b"],
                "{separator:?} {before}"
            );
        }
    }
    // Those characters alone break no line.
    let text = "\t\u{B0}\u{2019}".repeat(5);
    assert_eq!(lines(&unwrapped(&resolver, &text), &text), [text.as_str()]);
}

#[test]
fn a_line_break_draws_no_glyph() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for text in ["A\nB", "A \r\nB", "A\u{2029}B"] {
        let laid = unwrapped(&resolver, text);
        let glyphs: Vec<_> = (0..laid.line_count())
            .flat_map(|line| {
                laid.glyph_runs(line)
                    .flat_map(|run| run.positions().map(|glyph| glyph.id.0).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(!glyphs.contains(&0), "{text:?}: {glyphs:?}");
        assert_eq!(laid.line_count(), 2, "{text:?}");
    }
}

#[test]
fn a_word_wider_than_the_line_breaks_between_characters() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "Pneumonoultramicroscopic";
    let width = unwrapped(&resolver, "Pneum").width() + 0.5;

    let wrapped = layout(&resolver, text, TextDirection::Ltr, TextAlign::Start, width);
    let pieces = lines(&wrapped, text);

    assert!(pieces.len() > 3, "{pieces:?}");
    assert_eq!(pieces.concat(), text);
    assert!((0..wrapped.line_count()).all(|line| wrapped.line_right(line) <= width));
}

#[test]
fn right_to_left_lines_align_right_and_carets_run_leftward() {
    let resolver = FontFamilyResolver::new([typeface(ARABIC_VF)]);
    let text = "مرحبا بالعالم";

    let laid = layout(
        &resolver,
        text,
        TextDirection::Content,
        TextAlign::Start,
        400.0,
    );

    assert!(laid.line_left(0) > 100.0, "{}", laid.line_left(0));
    assert!((laid.line_right(0) - 400.0).abs() < 0.01);
    let start = laid.horizontal_position(0, CaretDirection::Primary);
    let end = laid.horizontal_position(text.len(), CaretDirection::Primary);
    assert!((start - laid.line_right(0)).abs() < 0.01, "{start}");
    assert!((end - laid.line_left(0)).abs() < 0.01, "{end}");
}

#[test]
fn each_alignment_puts_the_slack_on_its_side_of_the_paragraph_direction() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let cases = [
        (TextDirection::Ltr, "שלום", false),
        (TextDirection::Rtl, "hello", true),
        (TextDirection::Content, "hello", false),
        (TextDirection::Content, "שלום", true),
        (TextDirection::ContentOrLtr, "123.45", false),
        (TextDirection::ContentOrRtl, "123.45", true),
        (TextDirection::Unspecified, "hello", false),
        (TextDirection::Unspecified, "שלום", true),
        (TextDirection::Content, "١٢٣ Latin", false),
        (TextDirection::Content, "\u{200f}Latin", true),
        (TextDirection::Content, "\u{200e}שלום", false),
        (TextDirection::Content, "\u{2067}שלום\u{2069}Latin", false),
        (TextDirection::Content, "\u{2066}Latin\u{2069}שלום", true),
        (TextDirection::ContentOrLtr, "١٢٣", false),
        (TextDirection::ContentOrRtl, "١٢٣", true),
    ];
    // The share of the slack before the line in a left-to-right and in a
    // right-to-left paragraph.
    let alignments = [
        (TextAlign::Left, 0.0, 0.0),
        (TextAlign::Right, 1.0, 1.0),
        (TextAlign::Center, 0.5, 0.5),
        (TextAlign::Start, 0.0, 1.0),
        (TextAlign::End, 1.0, 0.0),
        (TextAlign::Justify, 0.0, 1.0),
        (TextAlign::Unspecified, 0.0, 1.0),
    ];
    for (direction, text, rtl) in cases {
        for (align, ltr_share, rtl_share) in alignments {
            let laid = layout(&resolver, text, direction, align, 400.0);
            let slack = 400.0 - (laid.line_right(0) - laid.line_left(0));
            let share = if rtl { rtl_share } else { ltr_share };
            assert!(
                (laid.line_left(0) - share * slack).abs() < 0.01,
                "{align:?}, {direction:?}, {text:?}: the line starts at {}",
                laid.line_left(0)
            );
        }
    }
}

#[test]
fn spaces_after_a_line_hang_past_its_end() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let centered = |text| {
        layout(
            &resolver,
            text,
            TextDirection::Content,
            TextAlign::Center,
            200.0,
        )
    };
    let positions = |laid: &TextLayout| {
        laid.glyph_runs(0)
            .flat_map(|run| run.positions().collect::<Vec<_>>())
            .take(3)
            .collect::<Vec<_>>()
    };

    let bare = centered("Tea");
    let spaced = centered("Tea   ");

    assert_eq!(positions(&spaced), positions(&bare));
    assert_eq!(spaced.line_right(0), bare.line_right(0));
}

#[test]
fn a_caret_between_directions_has_a_primary_and_a_secondary_place() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let text = "abc مرحبا";
    let laid = unwrapped(&resolver, text);
    let boundary = "abc ".len();

    let primary = laid.horizontal_position(boundary, CaretDirection::Primary);
    let secondary = laid.horizontal_position(boundary, CaretDirection::Secondary);

    // The primary caret ends the left-to-right text; the secondary one
    // starts the Arabic word, at its right edge.
    assert!(
        (primary - laid.horizontal_position(3, CaretDirection::Primary) - space_width(&laid)).abs()
            < 0.01
    );
    assert!((secondary - laid.line_right(0)).abs() < 0.01, "{secondary}");
}

fn space_width(laid: &TextLayout) -> f32 {
    laid.horizontal_position(4, CaretDirection::Primary)
        - laid.horizontal_position(3, CaretDirection::Primary)
}

#[test]
fn a_position_maps_back_to_the_offset_of_its_caret() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    for text in ["Hello, world", "مرحبا بالعالم"] {
        let laid = unwrapped(&resolver, text);
        let baseline = laid.line_baseline(0);
        for (offset, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let x = laid.horizontal_position(offset, CaretDirection::Primary);
            assert_eq!(
                laid.offset_for_position(x, baseline),
                offset,
                "{text:?} at {offset}"
            );
        }
    }
}

#[test]
fn a_line_with_a_taller_fallback_font_makes_room_for_it() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);
    let latin = unwrapped(&resolver, "abc");
    let mixed = unwrapped(&resolver, "abc नमस्ते");

    let height = |laid: &TextLayout| laid.line_bottom(0) - laid.line_top(0);
    assert!(
        height(&mixed) > height(&latin),
        "{} {}",
        height(&mixed),
        height(&latin)
    );
}

#[test]
fn glyph_runs_follow_each_other_from_the_line_start() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);
    let laid = unwrapped(&resolver, "abc नमस्ते def");

    // "abc ", the Hindi word with the space after it, and "def".
    let starts: Vec<f32> = laid
        .glyph_runs(0)
        .filter_map(|run| run.positions().next().map(|glyph| glyph.x))
        .collect();
    assert_eq!(starts.len(), 3);
    assert!((starts[0] - laid.line_left(0)).abs() < 0.01, "{starts:?}");
    assert!(
        starts.windows(2).all(|pair| pair[0] < pair[1]),
        "{starts:?}"
    );
}

#[test]
fn an_arabic_word_broken_across_lines_takes_its_end_and_start_forms() {
    let resolver = FontFamilyResolver::new([typeface(ARABIC_VF)]);
    let word = "بالعالمبالعالم";
    let whole = unwrapped(&resolver, word);
    let broken = layout(
        &resolver,
        word,
        TextDirection::Content,
        TextAlign::Start,
        whole.width() / 2.0,
    );

    assert!(broken.line_count() > 1);
    let glyphs = |laid: &TextLayout, line: usize| -> Vec<u32> {
        laid.glyph_runs(line)
            .flat_map(|run| run.positions().map(|glyph| glyph.id.0).collect::<Vec<_>>())
            .collect()
    };
    let joined: Vec<u32> = (0..broken.line_count())
        .flat_map(|line| glyphs(&broken, line))
        .collect();
    let mut unbroken = glyphs(&whole, 0);
    let mut rejoined = joined;
    unbroken.sort_unstable();
    rejoined.sort_unstable();
    assert_ne!(rejoined, unbroken, "the letters at the break change form");
}

#[test]
fn laying_out_into_an_earlier_layout_gives_what_a_new_one_gives() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let mut layouter = TextLayouter::default();
    let mut reused = TextLayout::default();
    let texts = [
        "The quick brown fox jumps over the lazy dog",
        "مرحبا بالعالم",
        "a\nb",
    ];
    for text in texts {
        let spans = [span(0..text.len(), &family, 400)];
        let layout_input = LayoutInput {
            text,
            spans: &spans,
            paragraph: &paragraph,
            lengths: ParagraphLengths::default(),
            min_width: 0.0,
            max_width: 120.0,
            options: TextLayoutOptions::default(),
        };
        layouter.layout_into(&layout_input, &resolver, &mut reused);
        let fresh = TextLayouter::default().layout(&layout_input, &resolver);
        assert_eq!(reused.line_count(), fresh.line_count(), "{text}");
        for line in 0..fresh.line_count() {
            let glyphs = |laid: &TextLayout| -> Vec<(u32, u32, u32)> {
                laid.glyph_runs(line)
                    .flat_map(|run| {
                        run.positions()
                            .map(|glyph| (glyph.id.0, glyph.x.to_bits(), glyph.y.to_bits()))
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            assert_eq!(glyphs(&reused), glyphs(&fresh), "{text} line {line}");
            assert_eq!(reused.line_end(line), fresh.line_end(line));
        }
    }
}

#[test]
fn a_line_grows_to_hold_text_shifted_off_its_baseline() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let laid_with_shift = |shift: f32| {
        let spans = [
            span(0..5, &family, 400),
            ShapingSpan {
                baseline_shift: shift,
                ..span(5..10, &family, 400)
            },
        ];
        TextLayouter::default().layout(
            &LayoutInput {
                text: "plainmoved",
                spans: &spans,
                paragraph: &ParagraphStyle::default(),
                lengths: ParagraphLengths::default(),
                min_width: 0.0,
                max_width: f32::INFINITY,
                options: TextLayoutOptions::default(),
            },
            &resolver,
        )
    };
    let plain = laid_with_shift(0.0);
    let raised = laid_with_shift(6.0);
    let lowered = laid_with_shift(-6.0);

    assert!((raised.height() - plain.height() - 6.0).abs() < 0.01);
    assert!((raised.line_baseline(0) - plain.line_baseline(0) - 6.0).abs() < 0.01);
    assert!((lowered.height() - plain.height() - 6.0).abs() < 0.01);
    assert!((lowered.line_baseline(0) - plain.line_baseline(0)).abs() < 0.01);
}

// What a layout shows: its size, whether it overflowed, and per line its
// text, its ellipsis and its glyphs where they draw.
type Shown = (
    u32,
    u32,
    bool,
    bool,
    Vec<(usize, usize, bool, Vec<(u32, u32, u32)>)>,
);

fn shown(layout: &TextLayout) -> Shown {
    let lines = (0..layout.line_count())
        .map(|line| {
            let glyphs = layout
                .glyph_runs(line)
                .flat_map(|run| {
                    run.positions()
                        .map(|glyph| (glyph.id.0, glyph.x.to_bits(), glyph.y.to_bits()))
                        .collect::<Vec<_>>()
                })
                .collect();
            (
                layout.line_start(line),
                layout.line_end(line),
                layout.is_line_ellipsized(line),
                glyphs,
            )
        })
        .collect();
    (
        layout.width().to_bits(),
        layout.height().to_bits(),
        layout.did_overflow_width(),
        layout.did_overflow_height(),
        lines,
    )
}

#[test]
fn a_kerned_pair_draws_the_glyphs_its_letters_draw_alone() {
    // Kerning moves the V toward each A, so a break between them would
    // change their places.
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let ids = |text: &str| -> Vec<u32> {
        unwrapped(&resolver, text)
            .glyph_runs(0)
            .flat_map(|run| run.positions().map(|glyph| glyph.id.0).collect::<Vec<_>>())
            .collect()
    };

    let alone = [ids("A"), ids("V"), ids("A")].concat();
    assert_eq!(ids("AVA"), alone);
}

#[test]
fn a_combining_mark_draws_over_its_base_at_its_offset() {
    // Roboto has no precomposed q with an acute, so the acute takes no
    // advance and draws back over the q.
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let layout = unwrapped(&resolver, "q\u{301}x");
    let xs: Vec<f32> = layout
        .glyph_runs(0)
        .flat_map(|run| run.positions().map(|glyph| glyph.x).collect::<Vec<_>>())
        .collect();

    let [q, acute, x] = xs[..] else {
        panic!("three glyphs, not {xs:?}");
    };
    assert!(q < acute && acute < x, "{xs:?}");
}

#[test]
fn a_layout_from_the_glyphs_of_another_size_gives_what_a_new_one_gives() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let text = "AVA Toyota 123.45 We fly over the lazy dog";
    let small = [span(0..text.len(), &family, 400)];
    let large = [ShapingSpan {
        size: 24.0,
        ..span(0..text.len(), &family, 400)
    }];
    let at_small = LayoutInput {
        text,
        spans: &small,
        paragraph: &paragraph,
        lengths: ParagraphLengths::default(),
        min_width: 0.0,
        max_width: 90.0,
        options: TextLayoutOptions::default(),
    };
    let at_large = LayoutInput {
        spans: &large,
        ..at_small
    };
    let mut layouter = TextLayouter::default();
    let shaped = layouter.layout(&at_small, &resolver);
    let fresh = TextLayouter::default().layout(&at_large, &resolver);
    let relaid = layouter.relayout(&at_large, &resolver, &shaped);
    assert_eq!(shown(&relaid), shown(&fresh));
}

#[test]
fn a_layout_from_the_glyphs_of_another_width_gives_what_a_new_one_gives() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let mut layouter = TextLayouter::default();
    let texts = [
        "AVA Toyota 123.45 We fly over the lazy dog",
        "مرحبا بالعالم الجميل اليوم",
        "Total: مرحبا 42 bonjour\nsecond line",
        // A combining mark draws at an offset from the pen, and items after
        // it draw at the pen.
        "q\u{301}uays q\u{301}uote مرحبا q\u{301}uick fox",
    ];
    let wrapped = TextLayoutOptions::default();
    let ellipsized = TextLayoutOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: 1,
        ..wrapped
    };
    // Every width from every other: a line cut inside a word shapes its
    // parts again, and an ellipsis adds glyphs to the layout it is in.
    let frames = [
        (f32::INFINITY, wrapped),
        (90.0, wrapped),
        (60.0, ellipsized),
        (25.0, wrapped),
    ];
    for text in texts {
        let spans = [span(0..text.len(), &family, 400)];
        let input = |(max_width, options)| LayoutInput {
            text,
            spans: &spans,
            paragraph: &paragraph,
            lengths: ParagraphLengths::default(),
            min_width: 0.0,
            max_width,
            options,
        };
        for from in frames {
            let shaped = layouter.layout(&input(from), &resolver);
            for to in frames {
                let fresh = TextLayouter::default().layout(&input(to), &resolver);
                let relaid = layouter.relayout(&input(to), &resolver, &shaped);
                assert_eq!(
                    shown(&relaid),
                    shown(&fresh),
                    "{text} from {from:?} to {to:?}"
                );
            }
        }
    }
}

#[test]
fn a_text_laid_out_again_into_the_same_lines_equals_its_first_layout() {
    // A renderer compares layouts to find what it must draw again: a text
    // laid out anew, as a string drawn again each frame is, draws the same
    // though it takes another id.
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "Wrapped text lays out the same twice";
    let first = layout(
        &resolver,
        text,
        TextDirection::Content,
        TextAlign::Start,
        90.0,
    );
    let again = layout(
        &resolver,
        text,
        TextDirection::Content,
        TextAlign::Start,
        90.0,
    );
    assert_ne!(first.id(), again.id(), "each layout takes an id of its own");
    assert_eq!(first, again, "the same lines draw the same");
    let wider = layout(
        &resolver,
        text,
        TextDirection::Content,
        TextAlign::Start,
        140.0,
    );
    assert_ne!(first, wider, "other lines draw otherwise");
}
