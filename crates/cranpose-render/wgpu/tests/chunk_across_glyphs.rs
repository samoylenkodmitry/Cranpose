//! Text between stacked opaque cards draws where it falls without closing
//! the arena chunk the cards fill, so the cards' opaque interiors still go
//! down in one draw ahead of the paint, and the frame draws the same pixels
//! as without the pre-pass.

use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME: u32 = 200;
const LEVELS: usize = 12;
const NO_INTERIORS_FIRST: &str = "CRANPOSE_NO_INTERIORS_FIRST";
const PALETTE: [Color; 3] = [
    Color(0.9, 0.3, 0.24, 1.0),
    Color(0.24, 0.67, 0.35, 1.0),
    Color(0.27, 0.43, 0.9, 1.0),
];

/// Cards stacked like replies in a thread, each inset from the last and
/// labelled at its top, where the next card covers the label.
#[composable]
fn StackedLabelledCards() {
    FramePage(FRAME, FRAME, Color(0.08, 0.09, 0.15, 1.0), || {
        for level in 0..LEVELS {
            let inset = 6.0 * level as f32 + 0.25;
            let side = FRAME as f32 - 2.0 * inset;
            Box(
                rect_modifier([inset, inset, side, side]).background(PALETTE[level % 3]),
                BoxSpec::default(),
                move || {
                    Text(
                        format!("level {level}"),
                        Modifier::empty(),
                        TextStyle::from_span_style(SpanStyle {
                            color: Some(Color::WHITE),
                            font_size: TextUnit::Sp(12.0),
                            ..Default::default()
                        }),
                    );
                },
            );
        }
    });
}

#[test]
fn text_between_stacked_cards_keeps_their_interiors_in_one_draw() {
    let Some((with, with_stats)) = support::warm_app_frame(StackedLabelledCards, FRAME, FRAME)
    else {
        return;
    };
    cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, Some("1"));
    let without = support::warm_app_frame(StackedLabelledCards, FRAME, FRAME);
    cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, None);
    let Some((without, without_stats)) = without else {
        return;
    };
    assert_eq!(
        with_stats.shape_interior_draws, 1,
        "the page and every card lay their interiors down in one draw"
    );
    assert_eq!(without_stats.shape_interior_draws, 0);
    let differing = support::differing_pixels(FRAME, &with.pixels, &without.pixels);
    assert!(
        differing.is_empty(),
        "the pre-pass changed pixels: {}",
        support::describe_differing(&differing)
    );
}
