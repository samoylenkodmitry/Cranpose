// Elevation shadows, composited layers and stored runs between stacked
// opaque cards draw where they fall without closing the arena chunk the
// cards fill, so the cards' opaque interiors still go down in one draw ahead
// of the paint, and the frame draws the same pixels as without the pre-pass.

use cranpose_ui_graphics::{Brush, CompositingStrategy, GraphicsLayer, Rect};
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
    // A drawing of more records than a run joins the arena with: a stored
    // run, drawn from buffers it keeps between frames.
    Dots,
    // A stored run of opaque tiles, which lays interiors of its own.
    Tiles,
    Nothing,
}

// Cards stacked like replies in a thread, each inset from the last and
// raised over it by a shadow, or carrying a badge drawn through a surface
// of its own and composited, or a field of dots or of tiles, or plain.
#[composable]
fn StackedCards(overlay: Overlay) {
    FramePage(FRAME, FRAME, Color(0.08, 0.09, 0.15, 1.0), move || {
        for level in 0..LEVELS {
            let inset = 6.0 * level as f32 + 0.25;
            let side = FRAME as f32 - 2.0 * inset;
            let card = rect_modifier([inset, inset, side, side]);
            let card = match overlay {
                Overlay::Shadow => card.shadow(4.0),
                Overlay::Composite | Overlay::Dots | Overlay::Tiles | Overlay::Nothing => card,
            };
            Box(
                card.background(PALETTE[level % 3]),
                BoxSpec::default(),
                move || {
                    if matches!(overlay, Overlay::Dots | Overlay::Tiles) {
                        let side = if overlay == Overlay::Tiles { 33.0 } else { 1.0 };
                        Box(
                            Modifier::empty()
                                .size_points(30.0, 6.0)
                                .draw_behind(move |scope| {
                                    for dot in 0..80 {
                                        let rect = Rect {
                                            x: (dot % 20) as f32 * 1.5,
                                            y: (dot / 20) as f32 * 1.5,
                                            width: side,
                                            height: side,
                                        };
                                        scope.draw_rect_at(rect, Brush::solid(Color::WHITE));
                                    }
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                    }
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
fn DottedCards() {
    StackedCards(Overlay::Dots);
}

#[composable]
fn TiledCards() {
    StackedCards(Overlay::Tiles);
}

#[composable]
fn PlainCards() {
    StackedCards(Overlay::Nothing);
}

// `page` lays its interiors in `draws` draws, the cards' in one, draws the
// same pixels without the pre-pass, and draws its overlays.
fn assert_interior_draws(page: fn(), draws: u32) {
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
        with_stats.shape_interior_draws, draws,
        "the page and every card lay their interiors down in one draw"
    );
    assert_eq!(
        without_stats.shape_interior_draws, 0,
        "turned off, no pass lays interiors down"
    );
    let differing = support::differing_pixels(FRAME, &with.pixels, &without.pixels);
    assert!(
        differing.is_empty(),
        "the pre-pass changed pixels: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn shadows_between_stacked_cards_keep_their_interiors_in_one_draw() {
    assert_interior_draws(ShadowedCards, 1);
}

#[test]
fn composites_between_stacked_cards_keep_their_interiors_in_one_draw() {
    assert_interior_draws(BadgedCards, 1);
}

#[test]
fn stored_runs_between_stacked_cards_keep_their_interiors_in_one_draw() {
    assert_interior_draws(DottedCards, 1);
}

// Each stored run lays its tiles' interiors from its own tables, and the
// cards' interiors around them stay one draw.
#[test]
fn stored_runs_with_interiors_of_their_own_draw_the_same_pixels() {
    assert_interior_draws(
        TiledCards,
        1 + u32::try_from(LEVELS).expect("a dozen levels"),
    );
}
