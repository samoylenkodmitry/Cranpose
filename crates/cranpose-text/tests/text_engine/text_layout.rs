use cranpose_text::{
    CaretDirection, FontFamily, FontFamilyResolver, LayoutInput, TextAlign, TextDirection,
    TextLayout, TextLayoutOptions, TextLayouter,
};

use crate::fonts::{ARABIC_VF, DEVANAGARI_VF, ROBOTO, span, typeface};

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    direction: TextDirection,
    align: TextAlign,
    width: f32,
) -> TextLayout {
    let family = FontFamily::Default;
    let spans = [span(0..text.len(), &family, 400)];
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            direction,
            align,
            width,
            options: TextLayoutOptions::default(),
        },
        resolver,
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
    let text = "The quick brown fox";
    let width = unwrapped(&resolver, "The quick")
        .width()
        .max(unwrapped(&resolver, "brown fox").width())
        + 1.0;

    let wrapped = layout(&resolver, text, TextDirection::Ltr, TextAlign::Start, width);

    assert_eq!(lines(&wrapped, text), ["The quick ", "brown fox"]);
    assert_eq!(wrapped.line_visible_end(0), "The quick".len());
    assert!(wrapped.line_right(0) <= width);
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
        .filter_map(|run| run.positions().next().map(|(_, x, _)| x))
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
            .flat_map(|run| run.positions().map(|(id, _, _)| id.0).collect::<Vec<_>>())
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
