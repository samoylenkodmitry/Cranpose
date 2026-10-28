use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, ParagraphIntrinsics, ParagraphLengths,
    ParagraphStyle, TextLayout, TextLayoutOptions, TextLayouter,
};

use crate::fonts::{ROBOTO, lay_out_with, span, typeface};

const TEXT: &str = "Some words, one of them extraordinarily long.\nA second paragraph here";

fn lengths(first: f32, rest: f32) -> ParagraphLengths {
    ParagraphLengths {
        first_line_indent: first,
        rest_line_indent: rest,
        ..ParagraphLengths::default()
    }
}

fn intrinsics(
    resolver: &FontFamilyResolver,
    text: &str,
    lengths: ParagraphLengths,
) -> ParagraphIntrinsics {
    let family = FontFamily::Default;
    let spans = [span(0..text.len(), &family, 400)];
    TextLayouter::default().intrinsics(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph: &ParagraphStyle::default(),
            lengths,
            min_width: 0.0,
            max_width: 1.0,
            options: TextLayoutOptions::default(),
        },
        resolver,
    )
}

fn layout(resolver: &FontFamilyResolver, lengths: ParagraphLengths, width: f32) -> TextLayout {
    lay_out_with(
        resolver,
        TEXT,
        (&ParagraphStyle::default(), lengths),
        width,
        TextLayoutOptions::default(),
    )
}

// Whether a line of `laid` ends inside a word of `TEXT`.
fn breaks_a_word(laid: &TextLayout) -> bool {
    (0..laid.line_count()).any(|line| {
        let end = laid.line_end(line);
        TEXT.get(..end)
            .and_then(|before| before.chars().next_back())
            .is_some_and(|last| !last.is_whitespace())
            && TEXT
                .get(end..)
                .and_then(|after| after.chars().next())
                .is_some_and(|next| !next.is_whitespace())
    })
}

#[test]
fn no_word_breaks_at_the_min_intrinsic_width_and_one_does_below_it() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for (first, rest) in [(0.0, 0.0), (24.0, 0.0), (0.0, 24.0)] {
        let widths = intrinsics(&resolver, TEXT, lengths(first, rest));

        let at = layout(&resolver, lengths(first, rest), widths.min_intrinsic_width);
        let below = layout(
            &resolver,
            lengths(first, rest),
            widths.min_intrinsic_width - 0.5,
        );

        assert!(!breaks_a_word(&at), "{first} {rest}");
        assert!(!at.did_overflow_width(), "{first} {rest}");
        assert!(breaks_a_word(&below), "{first} {rest}");
    }
}

#[test]
fn no_paragraph_wraps_at_the_max_intrinsic_width_and_one_does_below_it() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for first in [0.0, 24.0] {
        let widths = intrinsics(&resolver, TEXT, lengths(first, 8.0));

        let at = layout(&resolver, lengths(first, 8.0), widths.max_intrinsic_width);
        let below = layout(
            &resolver,
            lengths(first, 8.0),
            widths.max_intrinsic_width - 0.5,
        );
        let unbounded = layout(&resolver, lengths(first, 8.0), f32::INFINITY);

        assert_eq!(at.line_count(), 2, "{first}");
        assert_eq!(below.line_count(), 3, "{first}");
        assert_eq!(
            unbounded.width(),
            widths.max_intrinsic_width.ceil(),
            "{first}"
        );
    }
}

#[test]
fn an_empty_text_is_as_wide_as_its_first_line_indent() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let widths = intrinsics(&resolver, "", lengths(12.0, 4.0));

    assert_eq!(
        widths,
        ParagraphIntrinsics {
            min_intrinsic_width: 12.0,
            max_intrinsic_width: 12.0,
        }
    );
}
