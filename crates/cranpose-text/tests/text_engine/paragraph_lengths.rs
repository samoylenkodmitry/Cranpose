use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, LineHeightAlignment, LineHeightMode,
    LineHeightStyle, LineHeightTrim, ParagraphLengths, ParagraphStyle, ShapingSpan, TextLayout,
    TextLayoutOptions, TextLayouter, TextUnit,
};

use crate::fonts::{ARABIC_VF, ROBOTO, span, typeface};

const TEXT: &str = "The quick brown fox jumps over the lazy dog";

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    paragraph: &ParagraphStyle,
    lengths: ParagraphLengths,
    width: f32,
    letter_spacing: f32,
) -> TextLayout {
    let family = FontFamily::Default;
    let spans = [ShapingSpan {
        letter_spacing,
        ..span(0..text.len(), &family, 400)
    }];
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph,
            lengths,
            width,
            options: TextLayoutOptions::default(),
        },
        resolver,
    )
}

fn line_height(trim: LineHeightTrim) -> ParagraphStyle {
    ParagraphStyle {
        line_height: TextUnit::Sp(40.0),
        line_height_style: Some(LineHeightStyle {
            alignment: LineHeightAlignment::Proportional,
            trim,
            mode: LineHeightMode::Fixed,
        }),
        ..ParagraphStyle::default()
    }
}

fn tall() -> ParagraphLengths {
    ParagraphLengths {
        line_height: Some(40.0),
        ..ParagraphLengths::default()
    }
}

#[test]
fn every_line_takes_the_asked_line_height() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let laid = layout(
        &resolver,
        TEXT,
        &line_height(LineHeightTrim::None),
        tall(),
        120.0,
        0.0,
    );

    assert!(laid.line_count() > 2);
    for line in 0..laid.line_count() {
        let height = laid.line_bottom(line) - laid.line_top(line);
        assert!((height - 40.0).abs() < 0.01, "line {line}: {height}");
    }
    assert!((laid.height() - 40.0 * line_count(&laid)).abs() < 0.01);
}

#[test]
fn trimming_takes_the_leading_off_the_first_top_and_the_last_bottom() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let full = layout(
        &resolver,
        TEXT,
        &line_height(LineHeightTrim::None),
        tall(),
        120.0,
        0.0,
    );

    let trimmed = layout(
        &resolver,
        TEXT,
        &line_height(LineHeightTrim::Both),
        tall(),
        120.0,
        0.0,
    );

    let last = trimmed.line_count() - 1;
    let height = |laid: &TextLayout, line: usize| laid.line_bottom(line) - laid.line_top(line);
    assert!(height(&trimmed, 0) < 40.0 && height(&trimmed, last) < 40.0);
    assert!((height(&trimmed, 1) - 40.0).abs() < 0.01);
    assert!(trimmed.height() < full.height());
    assert!(trimmed.line_baseline(0) < full.line_baseline(0));
}

fn line_count(laid: &TextLayout) -> f32 {
    f32::from(u16::try_from(laid.line_count()).expect("few lines"))
}

#[test]
fn indents_move_and_narrow_the_first_and_the_other_lines() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let lengths = ParagraphLengths {
        first_line_indent: 30.0,
        rest_line_indent: 10.0,
        ..ParagraphLengths::default()
    };
    let paragraph = ParagraphStyle::default();

    let latin = layout(&resolver, TEXT, &paragraph, lengths, 200.0, 0.0);
    let arabic = layout(
        &resolver,
        "مرحبا بالعالم، هذه جملة اختبار طويلة جدا",
        &paragraph,
        lengths,
        200.0,
        0.0,
    );

    assert!((latin.line_left(0) - 30.0).abs() < 0.01);
    assert!((latin.line_left(1) - 10.0).abs() < 0.01);
    assert!((0..latin.line_count()).all(|line| latin.line_right(line) <= 200.0));
    assert!(
        (arabic.line_right(0) - 170.0).abs() < 0.01,
        "{}",
        arabic.line_right(0)
    );
    assert!(
        (arabic.line_right(1) - 190.0).abs() < 0.01,
        "{}",
        arabic.line_right(1)
    );
}

#[test]
fn letter_spacing_widens_each_character_except_in_joined_scripts() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let paragraph = ParagraphStyle::default();
    let width = |text, spacing| {
        layout(
            &resolver,
            text,
            &paragraph,
            ParagraphLengths::default(),
            f32::INFINITY,
            spacing,
        )
        .width()
    };

    assert!((width("abc", 5.0) - width("abc", 0.0) - 15.0).abs() < 0.01);
    assert!((width("مرحبا", 5.0) - width("مرحبا", 0.0)).abs() < 0.01);
}
