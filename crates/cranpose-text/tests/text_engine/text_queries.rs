use cranpose_text::{
    CaretDirection, FontFamily, FontFamilyResolver, LayoutInput, ParagraphLengths, ParagraphStyle,
    ShapingSpan, TextAlign, TextLayout, TextLayoutOptions, TextLayouter,
};
use cranpose_ui_graphics::Rect;

use crate::fonts::{ARABIC_VF, ROBOTO, span, typeface};

const TEXT: &str = "The quick brown fox jumps over the lazy dog";

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    spans: &[ShapingSpan<'_>],
    align: TextAlign,
    width: f32,
) -> TextLayout {
    let paragraph = ParagraphStyle {
        text_align: align,
        ..ParagraphStyle::default()
    };
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans,
            paragraph: &paragraph,
            lengths: ParagraphLengths::default(),
            min_width: 0.0,
            max_width: width,
            options: TextLayoutOptions::default(),
        },
        resolver,
    )
}

fn plain(resolver: &FontFamilyResolver, text: &str, align: TextAlign, width: f32) -> TextLayout {
    let family = FontFamily::Default;
    layout(
        resolver,
        text,
        &[span(0..text.len(), &family, 400)],
        align,
        width,
    )
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn justified_lines_fill_the_width_except_the_last() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let laid = plain(&resolver, TEXT, TextAlign::Justify, 150.0);

    let last = laid.line_count() - 1;
    assert!(last > 0);
    for line in 0..last {
        assert!(close(laid.line_left(line), 0.0) && close(laid.line_right(line), 150.0));
        // The last visible character ends at the right edge.
        let end = laid.line_visible_end(line);
        let last_char = TEXT
            .get(..end)
            .and_then(|before| before.char_indices().next_back())
            .map(|(at, _)| at)
            .expect("a visible character");
        let bounds = laid.bounding_box(last_char);
        assert!(
            close(bounds.x + bounds.width, 150.0),
            "line {line}: {bounds:?}"
        );
    }
    assert!(laid.line_right(last) < 150.0);
}

#[test]
fn a_baseline_shift_raises_the_span() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let spans = [
        span(0..2, &family, 400),
        ShapingSpan {
            baseline_shift: 5.0,
            ..span(2..3, &family, 400)
        },
    ];

    let laid = layout(&resolver, "xyz", &spans, TextAlign::Start, f32::INFINITY);

    let baseline = laid.line_baseline(0);
    let heights: Vec<f32> = laid
        .glyph_runs(0)
        .flat_map(|run| run.positions().map(|glyph| glyph.y).collect::<Vec<_>>())
        .collect();
    assert!(close(heights[0], baseline), "{heights:?}");
    assert!(
        close(*heights.last().expect("glyphs"), baseline - 5.0),
        "{heights:?}"
    );
}

#[test]
fn a_character_box_and_a_caret_sit_between_its_caret_positions() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let laid = plain(&resolver, "abc", TextAlign::Start, f32::INFINITY);
    let caret = |offset| laid.horizontal_position(offset, CaretDirection::Primary);

    let bounds = laid.bounding_box(1);
    let cursor = laid.cursor_rect(2);

    assert!(close(bounds.x, caret(1)) && close(bounds.x + bounds.width, caret(2)));
    assert!(close(bounds.y, laid.line_top(0)) && close(bounds.height, laid.line_bottom(0)));
    assert!(close(cursor.x, caret(2)) && close(cursor.width, 0.0));
    assert!(
        close(laid.bounding_box(3).width, 0.0),
        "the end has no character"
    );
}

#[test]
fn a_selection_takes_a_rectangle_per_line_and_per_direction() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let wrapped = plain(&resolver, TEXT, TextAlign::Start, 150.0);
    let mixed_text = "abc مرحبا def";
    let mixed = plain(&resolver, mixed_text, TextAlign::Start, f32::INFINITY);
    let mut rects: Vec<Rect> = Vec::new();

    wrapped.selection_rects(4, wrapped.line_end(0) + 3, &mut rects);
    assert_eq!(rects.len(), 2, "{rects:?}");
    assert!(close(rects[1].x, wrapped.line_left(1)));
    assert!(rects[0].y < rects[1].y);

    rects.clear();
    // From "c" into the Arabic word: the left-to-right part and the
    // right-to-left part are apart on the line.
    mixed.selection_rects(2, "abc مر".len(), &mut rects);
    assert_eq!(rects.len(), 2, "{rects:?}");
}

#[test]
fn the_word_around_an_offset_runs_between_word_boundaries() {
    let text = "Hello, wide world";

    assert_eq!(cranpose_text::word_boundary(text, 2), 0..5);
    assert_eq!(cranpose_text::word_boundary(text, 7), 7..11);
    assert_eq!(cranpose_text::word_boundary(text, 9), 7..11);
    assert_eq!(
        cranpose_text::word_boundary(text, text.len()),
        12..text.len()
    );
}
