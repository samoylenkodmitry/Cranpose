//! Repro: a wrapping paragraph must PAINT the height it MEASURED.
//!
//! A `Text` without `fill_max_width` is placed at its own `metrics.width` — the
//! widest line it wrapped into, which is by construction narrower than the
//! constraint it wrapped under. The paint pass used to re-wrap at that placed
//! width, and because the widest line is exactly the one that fills the limit,
//! measuring it against itself pushed its last word onto a new line. The
//! paragraph then painted one line taller than the box the column had reserved:
//! the extra line was clipped away, and the block below it had already been
//! placed as if that line did not exist.
//!
//! Run it:
//!
//! ```text
//! cargo run -p desktop-app --features robot-app --example text_wrap_repro
//! ```
//!
//! What to look for: the paragraph sits in a boxed frame whose height is what
//! LAYOUT measured, and the red rule underneath is where the next sibling
//! starts. Fixed, every line of the paragraph is inside the frame and clear of
//! the rule. Broken, the paragraph's last line is missing and the text runs
//! into the rule.
//!
//! The strings are mixed Latin/Cyrillic on purpose: this was reported from an
//! app rendering Serbian receipts, and the defect needs the real proportional
//! font backend — with a monospaced stub the two wrap widths agree by accident
//! and nothing shows.

use cranpose_ui::{
    composable,
    text::{SpanStyle, TextUnit},
    Color, Column, ColumnSpec, LinearArrangement, Modifier, Size, Spacer, Text, TextStyle,
};

/// Width the paragraph is measured under. Chosen so the re-wrap grows a line.
const PARAGRAPH_WIDTH: f32 = 245.0;

const VERDICT: &str = "fed back картица scored fp32 износ once paper fed Vision dropped fed \
     widest the strip mask prompt mask threshold Vision on датум instance mask износ Apple";

const ENGINE: &str = "fp32 back ПДВ under box+point Рачун датум fp32 the ПДВ under dropped \
     укупно run box+point tail: tail: rten scored strip Kept Vision картица порез Kept";

fn body_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.10, 0.11, 0.13, 1.0)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn label_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.35, 0.38, 0.45, 1.0)),
            font_size: TextUnit::Sp(11.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// One paragraph inside a frame the size layout gave it, over a rule marking
/// where the following sibling begins.
#[allow(non_snake_case)]
#[composable]
fn FramedParagraph(title: String, body: String) {
    Column(
        Modifier::empty().width(PARAGRAPH_WIDTH),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Text(title.clone(), Modifier::empty(), label_style());
            // No `fill_max_width`: this is the shape that breaks. The node is
            // placed at the width the paragraph measured, not at the constraint.
            Text(
                body.clone(),
                Modifier::empty().background(Color(0.87, 0.93, 1.0, 1.0)),
                body_style(),
            );
            // The following sibling. Layout puts it directly under the
            // paragraph's MEASURED height, so anything the paragraph paints
            // past that height lands on top of this.
            Text(
                "▲ next sibling starts here".to_string(),
                Modifier::empty()
                    .fill_max_width()
                    .background(Color(0.85, 0.20, 0.18, 1.0)),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color::WHITE),
                        font_size: TextUnit::Sp(11.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
pub fn TextWrapReproScreen() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(24.0)
            .background(Color(0.98, 0.98, 0.97, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(20.0)),
        move || {
            Text(
                "Wrapped paragraph paints the height it measured".to_string(),
                Modifier::empty(),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color(0.10, 0.11, 0.13, 1.0)),
                        font_size: TextUnit::Sp(17.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            Text(
                "Every line of each paragraph must sit on the blue fill and clear of the red \
                 rule. A missing last line, or text touching the rule, is the defect."
                    .to_string(),
                Modifier::empty().width(520.0),
                label_style(),
            );
            Spacer(Size {
                width: 0.0,
                height: 4.0,
            });
            FramedParagraph("Verdict".to_string(), VERDICT.to_string());
            FramedParagraph("Engine".to_string(), ENGINE.to_string());
        },
    );
}
