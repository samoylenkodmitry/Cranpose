// Each draw of a frame's passes counts the one reason it starts a draw of
// its own, so the reasons of a frame add up to its draws. A label that lies
// over its card and under the card's dot breaks the shapes' draws by kind.

use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const SIZE: [u32; 2] = [200, 56];
const CARDS: u32 = 4;

#[composable]
fn Cards() {
    FramePage(SIZE[0], SIZE[1], Color(0.1, 0.1, 0.2, 1.0), || {
        for index in 0..CARDS {
            let x = 8.0 + index as f32 * 48.0;
            Box(
                rect_modifier([x, 8.0, 40.0, 40.0]).background(Color(0.9, 0.9, 0.8, 1.0)),
                BoxSpec::default(),
                || {
                    Text(
                        "card",
                        rect_modifier([4.0, 4.0, 32.0, 12.0]),
                        TextStyle::from_span_style(SpanStyle {
                            color: Some(Color(0.05, 0.05, 0.1, 1.0)),
                            font_size: TextUnit::Sp(10.0),
                            ..Default::default()
                        }),
                    );
                    Box(
                        rect_modifier([4.0, 10.0, 8.0, 8.0]).background(Color(1.0, 0.0, 0.0, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        }
    });
}

#[test]
fn the_reasons_the_draws_of_a_frame_start_add_up_to_its_draws() {
    let Some((_, stats)) =
        support::warm_app_frame_toggled(Cards, SIZE[0], SIZE[1], Some("CRANPOSE_GPU_STATS"))
    else {
        return;
    };
    let breaks = stats.batch_breaks;
    let reasons = [
        breaks.first,
        breaks.kind,
        breaks.pipeline,
        breaks.bind_group,
        breaks.scissor,
        breaks.uniform,
        breaks.buffers,
        breaks.unchanged,
        breaks.untracked,
    ];
    assert_eq!(
        reasons.iter().sum::<u32>(),
        stats.draw_calls,
        "every draw counts one reason: {breaks:?}"
    );
    assert!(
        breaks.first >= 1,
        "a pass starts with a first draw: {breaks:?}"
    );
    assert!(
        breaks.kind >= 1,
        "the labels between the cards and their dots break the shapes: {breaks:?}"
    );
}
