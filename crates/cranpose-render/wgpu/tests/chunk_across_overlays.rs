// Elevation shadows and composited layers between stacked opaque cards draw
// where they fall without closing the arena chunk the cards fill, so the
// cards' opaque interiors still go down in one draw ahead of the paint, and
// the frame draws the same pixels as without the pre-pass.

use cranpose_ui_graphics::{CompositingStrategy, GraphicsLayer};
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

// What each card draws besides its fill.
#[derive(Clone, Copy, PartialEq)]
enum Overlay {
    Shadow,
    Composite,
    Nothing,
}

// Cards stacked like replies in a thread, each inset from the last and
// raised over it by a shadow, or carrying a badge drawn through a surface
// of its own and composited, or plain.
#[composable]
fn StackedCards(overlay: Overlay) {
    FramePage(FRAME, FRAME, Color(0.08, 0.09, 0.15, 1.0), move || {
        for level in 0..LEVELS {
            let inset = 6.0 * level as f32 + 0.25;
            let side = FRAME as f32 - 2.0 * inset;
            let card = rect_modifier([inset, inset, side, side]);
            let card = match overlay {
                Overlay::Shadow => card.shadow(4.0),
                Overlay::Composite | Overlay::Nothing => card,
            };
            Box(
                card.background(PALETTE[level % 3]),
                BoxSpec::default(),
                move || {
                    if overlay == Overlay::Composite {
                        Box(
                            Modifier::empty()
                                .size_points(4.0, 4.0)
                                .graphics_layer_value(GraphicsLayer {
                                    alpha: 0.6,
                                    compositing_strategy: CompositingStrategy::Offscreen,
                                    ..Default::default()
                                })
                                .background(Color::WHITE),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        }
    });
}

#[composable]
fn ShadowedCards() {
    StackedCards(Overlay::Shadow);
}

#[composable]
fn BadgedCards() {
    StackedCards(Overlay::Composite);
}

#[composable]
fn PlainCards() {
    StackedCards(Overlay::Nothing);
}

// `page` keeps its cards' interiors in one draw, draws the same pixels
// without the pre-pass, and draws its overlays.
fn assert_one_interior_draw(page: fn()) {
    let Some((with, with_stats)) = support::warm_app_frame(page, FRAME, FRAME) else {
        return;
    };
    let without = support::warm_app_frame_toggled(page, FRAME, FRAME, Some(NO_INTERIORS_FIRST));
    let Some((without, without_stats)) = without else {
        return;
    };
    let Some((plain, _)) = support::warm_app_frame(PlainCards, FRAME, FRAME) else {
        return;
    };
    assert!(
        !support::differing_pixels(FRAME, &with.pixels, &plain.pixels).is_empty(),
        "the cards draw their overlays"
    );
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

#[test]
fn shadows_between_stacked_cards_keep_their_interiors_in_one_draw() {
    assert_one_interior_draw(ShadowedCards);
}

#[test]
fn composites_between_stacked_cards_keep_their_interiors_in_one_draw() {
    assert_one_interior_draw(BadgedCards);
}
