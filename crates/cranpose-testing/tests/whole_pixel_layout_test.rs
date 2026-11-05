//! Layout resolves to whole device pixels the way Compose's does: `dp` rounds
//! to pixels per value, weights give their leftover pixels to the leading
//! children, a fraction of the space rounds, and a text block's height rounds
//! up. The same screen then wraps and stacks to the same pixels as in Compose.

use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode};
use cranpose_ui::{
    Box, BoxSpec, LinearArrangement, Modifier, ParagraphStyle, Row, RowSpec, Size, SpanStyle, Text,
    TextStyle, text::TextUnit,
};

fn tagged(modifier: Modifier, label: &'static str) -> Modifier {
    modifier.semantics(move |config| {
        config.content_description = Some(label.to_string());
    })
}

fn placed(density: f32, window: Size, content: impl FnMut() + 'static) -> PlacedSemanticsNode {
    let mut rule = ComposeTestRule::with_density(density);
    rule.set_content(content).expect("the content composes");
    rule.placed_semantics(window)
        .expect("the content lays out")
        .expect("the content places something")
}

/// A node's left edge and width, in device pixels.
fn pixels(root: &PlacedSemanticsNode, label: &str, density: f32) -> (f32, f32) {
    let node = root
        .flatten()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("{label} is placed"));
    (
        node.layout_bounds.x * density,
        node.layout_bounds.width * density,
    )
}

fn assert_pixels(actual: (f32, f32), expected: (f32, f32), label: &str) {
    assert!(
        (actual.0 - expected.0).abs() < 0.01 && (actual.1 - expected.1).abs() < 0.01,
        "{label}: left edge and width {actual:?} px, Compose places it at {expected:?} px"
    );
}

/// The gauntlet's card row on a Mate 20 X (#1215): three weighted cards in a
/// 1063 px row, 8.0466 dp apart at density 3. Compose rounds each gap to 24 px
/// and gives the 1015 px left over as 339, 338 and 338.
#[test]
fn weighted_children_share_whole_pixels_and_the_first_takes_the_spare() {
    let density = 3.0;
    let root = placed(density, Size::new(1063.0 / density, 400.0), || {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0466)),
            || {
                for label in ["first", "second", "third"] {
                    Box(
                        tagged(Modifier::empty().weight(1.0).height(40.0), label),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    });
    assert_pixels(pixels(&root, "first", density), (0.0, 339.0), "first");
    assert_pixels(pixels(&root, "second", density), (363.0, 338.0), "second");
    assert_pixels(pixels(&root, "third", density), (725.0, 338.0), "third");
}

/// Compose sizes `fillMaxWidth(fraction)` to `(maxWidth * fraction).roundToInt()`.
#[test]
fn a_fraction_of_the_width_rounds_to_a_whole_pixel() {
    let density = 2.625;
    let root = placed(density, Size::new(1080.0 / density, 400.0), || {
        Box(
            tagged(
                Modifier::empty()
                    .fill_max_width_fraction(0.3333)
                    .height(40.0),
                "third",
            ),
            BoxSpec::default(),
            || {},
        );
    });
    assert_pixels(pixels(&root, "third", density), (0.0, 360.0), "third");
}

/// A text block is as tall as its paragraph, rounded up to the next pixel, so
/// the rows below it start on a whole pixel at any density.
#[test]
fn a_text_block_height_rounds_up_to_a_whole_pixel() {
    for density in [2.625, 3.0, 1.5] {
        let root = placed(density, Size::new(400.0, 400.0), || {
            Text(
                "Commodo esse aliquip amet",
                tagged(Modifier::empty(), "text"),
                TextStyle::default(),
            );
        });
        let node = root
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("text"))
            .expect("the text is placed");
        let height = node.layout_bounds.height * density;
        assert!(
            (height - height.round()).abs() < 0.01,
            "at density {density} the text is {height} px tall, not a whole pixel"
        );
    }
}

// A text block is exactly as tall as Android lays its lines out: whole-pixel
// line boxes stacked without the float error of adding them in `dp`, which
// made the Gauntlet's card bodies a pixel taller on a Mate 20 X (#2052).
// Each expected height is AOSP's for Cranpose Test Mono at that density.
#[test]
fn a_text_block_is_as_tall_as_android_lays_its_lines_out() {
    // (density, font size in sp, line height in em, lines, height in pixels)
    for (density, size, em, lines, expected) in [
        (2.625_f32, 10.5_f32, 1.5_f32, 1_usize, 28.0_f32),
        (3.0, 10.5, 1.25, 1, 31.0),
        (3.0, 10.0, 1.25, 3, 106.0),
        (1.5, 12.0, 1.4, 2, 44.0),
        (2.75, 11.5, 1.25, 2, 71.0),
        // Heights a `dp` round trip at these densities lands a hair above.
        (2.625, 8.5, 1.4, 1, 22.0),
        (2.75, 10.5, 1.25, 1, 29.0),
        (1.75, 8.5, 1.4, 3, 57.0),
    ] {
        let root = placed(density, Size::new(400.0, 400.0), move || {
            Text(
                vec!["a"; lines].join("\n"),
                tagged(Modifier::empty(), "text"),
                TextStyle {
                    span_style: SpanStyle {
                        font_size: TextUnit::Sp(size),
                        ..SpanStyle::default()
                    },
                    paragraph_style: ParagraphStyle {
                        line_height: TextUnit::Em(em),
                        ..ParagraphStyle::default()
                    },
                },
            );
        });
        let node = root
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("text"))
            .expect("the text is placed");
        let height = node.layout_bounds.height * density;
        assert!(
            (height - expected).abs() < 0.01,
            "{lines} lines of {size} sp at {em} em on {density}: {height} px, Android lays them out {expected} px tall"
        );
    }
}
