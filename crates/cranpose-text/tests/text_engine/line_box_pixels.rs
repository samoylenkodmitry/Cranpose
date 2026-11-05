// Line boxes in whole device pixels, as AOSP's `StaticLayout` lays out the
// lines Compose's `LineHeightStyleSpan` sizes: the font's ascent and descent
// rounded, the line height `ceil`ed, the leading split in whole pixels. Every
// expected pixel below is what that span gives Roboto on an Android phone.

use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, LineHeightAlignment, LineHeightMode,
    LineHeightStyle, LineHeightTrim, ParagraphLengths, ParagraphStyle, ShapingSpan, TextLayout,
    TextLayoutOptions, TextLayouter, TextOverflow, TextUnit,
};

use crate::fonts::{ROBOTO, span, typeface};

// The body of a Gauntlet card, longer than its four lines.
const BODY: &str = "Lorem nostrud irure aute tempor lorem sit aliqua velit ullamco aliquip \
                    veniam ullamco sit ullamco cillum magna exercitation ad consequat aliqua \
                    lorem nisi tempor";

// A Roboto paragraph's sizes in layout pixels and the grid they lie on.
#[derive(Clone, Copy)]
struct Sizes {
    font: f32,
    line_height: f32,
    grid: f32,
    width: f32,
}

fn layout(
    text: &str,
    sizes: Sizes,
    line_height_style: LineHeightStyle,
    options: TextLayoutOptions,
) -> TextLayout {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let spans = [ShapingSpan {
        size: sizes.font,
        ..span(0..text.len(), &family, 400)
    }];
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph: &ParagraphStyle {
                line_height: TextUnit::Sp(sizes.line_height),
                line_height_style: Some(line_height_style),
                ..ParagraphStyle::default()
            },
            lengths: ParagraphLengths {
                line_height: Some(sizes.line_height),
                grid: sizes.grid,
                ..ParagraphLengths::default()
            },
            min_width: 0.0,
            max_width: sizes.width,
            options,
        },
        &resolver,
    )
}

// `lines` lines of one letter each.
fn letters(lines: usize) -> String {
    vec!["a"; lines].join("\n")
}

fn style(
    alignment: LineHeightAlignment,
    trim: LineHeightTrim,
    mode: LineHeightMode,
) -> LineHeightStyle {
    LineHeightStyle {
        alignment,
        trim,
        mode,
    }
}

// Each line's baseline and bottom, in device pixels.
fn pixel_lines(laid: &TextLayout, grid: f32) -> Vec<(f32, f32)> {
    (0..laid.line_count())
        .map(|line| {
            (
                laid.line_baseline(line) * grid,
                laid.line_bottom(line) * grid,
            )
        })
        .collect()
}

fn assert_pixel_lines(laid: &TextLayout, grid: f32, expected: &[(f32, f32)], case: &str) {
    let actual = pixel_lines(laid, grid);
    let close = actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (a.0 - e.0).abs() < 1e-3 && (a.1 - e.1).abs() < 1e-3);
    assert!(
        close,
        "{case}: baselines and bottoms {actual:?} px, Android lays them at {expected:?} px"
    );
    // Whatever sizes the text rounds its height up to whole pixels.
    let height = (laid.height() * grid).ceil();
    let bottom = expected.last().map_or(0.0, |line| line.1);
    assert!(
        (height - bottom).abs() < 1e-3,
        "{case}: {height} px tall, Android makes it {bottom} px"
    );
}

#[test]
fn a_paragraph_is_as_tall_as_on_android_at_any_density() {
    let compose_default = LineHeightStyle::default();
    // (density, font, line height in em, lines, Android's height in pixels)
    for (grid, font, em, lines, height) in [
        (3.0_f32, 6.0_f32, 1.4_f32, 4_usize, 99.0_f32),
        (3.0, 6.0, 1.2, 4, 87.0),
        (1.5, 8.0, 1.2, 1, 14.0),
        (1.5, 6.5, 1.4, 2, 25.0),
        (1.75, 7.0, 1.2, 2, 29.0),
        (2.625, 5.0, 1.2, 2, 31.0),
        (2.625, 5.5, 1.2, 4, 71.0),
        (2.75, 7.0, 1.4, 4, 104.0),
    ] {
        let sizes = Sizes {
            font,
            line_height: font * em,
            grid,
            width: 400.0,
        };
        let laid = layout(
            &letters(lines),
            sizes,
            compose_default,
            TextLayoutOptions::default(),
        );
        let actual = (laid.height() * grid).ceil();
        assert!(
            (actual - height).abs() < 1e-3,
            "{lines} lines of {font} at {em} em on {grid}: {actual} px, Android makes it {height} px"
        );
        for (baseline, bottom) in pixel_lines(&laid, grid) {
            assert!(
                (baseline - baseline.round()).abs() < 1e-3
                    && (bottom - bottom.round()).abs() < 1e-3,
                "{lines} lines of {font} at {em} em on {grid}: a line at {baseline}, {bottom} px"
            );
        }
    }
}

// A Mate 20 X lays the body of a Gauntlet card in four 26 px lines, the
// last one ellipsized and 99 px down (#2052).
#[test]
fn an_ellipsized_card_body_stacks_like_on_android() {
    let sizes = Sizes {
        font: 6.0,
        line_height: 8.4,
        grid: 3.0,
        width: 314.0 / 3.0,
    };
    let options = TextLayoutOptions {
        max_lines: 4,
        overflow: TextOverflow::Ellipsis,
        ..TextLayoutOptions::default()
    };
    let laid = layout(BODY, sizes, LineHeightStyle::default(), options);
    assert!(laid.did_overflow_height());
    assert_pixel_lines(
        &laid,
        3.0,
        &[(17.0, 22.0), (43.0, 48.0), (69.0, 74.0), (95.0, 99.0)],
        "card body",
    );
}

// 18 px Roboto: an ascent of 17 px and a descent of 4 px.
fn eighteen(line_height: f32) -> Sizes {
    Sizes {
        font: 18.0,
        line_height,
        grid: 1.0,
        width: 400.0,
    }
}

#[test]
fn the_leading_splits_into_whole_pixels_as_on_android() {
    use LineHeightAlignment::{Bottom, Center, Proportional, Top};
    use LineHeightMode::Fixed;
    use LineHeightTrim::Both;
    let untrimmed = LineHeightTrim::None;
    for (case, line_height, line_style, expected) in [
        (
            "5 px of leading, proportional",
            26.0,
            style(Proportional, untrimmed, Fixed),
            [(21.0, 26.0), (47.0, 52.0), (73.0, 78.0)],
        ),
        (
            "3 px short, proportional",
            18.0,
            style(Proportional, untrimmed, Fixed),
            [(16.0, 18.0), (34.0, 36.0), (52.0, 54.0)],
        ),
        (
            "3 px short, top",
            18.0,
            style(Top, untrimmed, Fixed),
            [(14.0, 18.0), (32.0, 36.0), (50.0, 54.0)],
        ),
        (
            "3 px short, centre",
            18.0,
            style(Center, untrimmed, Fixed),
            [(15.0, 18.0), (33.0, 36.0), (51.0, 54.0)],
        ),
        (
            "3 px short, bottom",
            18.0,
            style(Bottom, untrimmed, Fixed),
            [(17.0, 18.0), (35.0, 36.0), (53.0, 54.0)],
        ),
        (
            "3 px short, both edges trimmed",
            18.0,
            style(Proportional, Both, Fixed),
            [(17.0, 19.0), (35.0, 37.0), (53.0, 57.0)],
        ),
    ] {
        let laid = layout(
            &letters(3),
            eighteen(line_height),
            line_style,
            TextLayoutOptions::default(),
        );
        assert_pixel_lines(&laid, 1.0, &expected, case);
    }
}

#[test]
fn a_tight_line_height_adds_nothing_at_a_trimmed_edge() {
    use LineHeightAlignment::Proportional;
    use LineHeightMode::Tight;
    use LineHeightTrim::Both;
    let untrimmed = LineHeightTrim::None;
    for (case, text, line_height, trim, expected) in [
        (
            "3 px short, both edges trimmed",
            letters(3),
            18.0,
            Both,
            vec![(16.0, 18.0), (34.0, 36.0), (52.0, 54.0)],
        ),
        (
            "one line 3 px short",
            letters(1),
            18.0,
            Both,
            vec![(16.0, 18.0)],
        ),
        (
            "3 px short, no edge trimmed",
            letters(3),
            18.0,
            untrimmed,
            vec![(17.0, 19.0), (35.0, 37.0), (53.0, 57.0)],
        ),
        (
            "5 px of leading",
            letters(3),
            26.0,
            Both,
            vec![(17.0, 22.0), (43.0, 48.0), (69.0, 73.0)],
        ),
    ] {
        let laid = layout(
            &text,
            eighteen(line_height),
            style(Proportional, trim, Tight),
            TextLayoutOptions::default(),
        );
        assert_pixel_lines(&laid, 1.0, &expected, case);
    }
}
