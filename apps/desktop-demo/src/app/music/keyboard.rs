// A piano keyboard as a composable: it lights the keys the parts of the
// performance hold down, and a press on a key sounds it.

use cranpose_animation::Lerp;
use cranpose_core::{rememberMutableStateOf, rememberUpdatedState, MutableState};
use cranpose_foundation::{PointerEvent, PointerEventKind, PointerId};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Brush, Canvas, Color, CornerRadii, DrawScope,
    Modifier, PointerInputScope, Rect, Size,
};

use super::{
    key_range::{is_black_key, KeyRange},
    performance::{local_performance, Performance},
    voicing::{local_voicing, Voicing},
};

const KEY_GAP: f32 = 1.0;
const KEY_CORNER: f32 = 3.0;
const KEY_COUNT: usize = 128;
// How far a lit key is mixed toward white at its top.
const LIGHT_TOP: f32 = 0.55;

// The colors of a keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyboardColors {
    // A white key at its top and at its lower edge.
    pub white: Color,
    pub white_edge: Color,
    // A black key at its lower end and at its top.
    pub black: Color,
    pub black_edge: Color,
    // A key under a finger.
    pub hand: Color,
    pub gap: Color,
}

// The key under each finger that is down.
type Fingers = MutableState<Vec<(PointerId, u8)>>;

// Moves the sound of one finger with `event`: a press strikes the key under
// it, a drag carries the sound to the next key, a lift ends it. Returns
// whether the event changed a key.
fn follow_finger(
    event: &PointerEvent,
    size: Size,
    keys: KeyRange,
    fingers: Fingers,
    hand: &Performance,
    voicing: Voicing,
) -> bool {
    let before = fingers.with(|fingers| {
        fingers
            .iter()
            .find(|(finger, _)| *finger == event.id)
            .map(|(_, key)| *key)
    });
    let under = || keys.key_at(event.position.x, event.position.y / size.height, size.width);
    let over = match event.kind {
        PointerEventKind::Down => under(),
        PointerEventKind::Move if before.is_some() => under(),
        PointerEventKind::Up | PointerEventKind::Cancel => None,
        _ => return false,
    };
    if over == before {
        return false;
    }
    if let Some(key) = before {
        hand.key_up(key);
    }
    if let Some(key) = over {
        hand.key_down(key, voicing);
    }
    fingers.update(|fingers| {
        fingers.retain(|(finger, _)| *finger != event.id);
        fingers.extend(over.map(|key| (event.id, key)));
    });
    true
}

// The place of `key` on a keyboard of `size`, or none outside the range.
fn key_rect(keys: KeyRange, key: u8, size: Size) -> Option<Rect> {
    let (left, width) = keys.key_span(key, size.width)?;
    let height = if is_black_key(key) {
        size.height * keys.black_key_height()
    } else {
        size.height
    };
    Some(Rect {
        x: left + KEY_GAP * 0.5,
        y: 0.0,
        width: (width - KEY_GAP).max(1.0),
        height,
    })
}

const KEY_CORNERS: CornerRadii = CornerRadii {
    top_left: 0.0,
    top_right: 0.0,
    bottom_right: KEY_CORNER,
    bottom_left: KEY_CORNER,
};

// The keys at rest. Nothing here reads the clock, so the layer is drawn once.
fn draw_keys(scope: &mut dyn DrawScope, keys: KeyRange, colors: KeyboardColors) {
    let size = scope.size();
    scope.draw_rect(Brush::solid(colors.gap));
    // The black keys lie over the white ones.
    for black in [false, true] {
        let (top, bottom) = if black {
            (colors.black_edge, colors.black)
        } else {
            (colors.white, colors.white_edge)
        };
        for key in keys.keys().filter(|key| is_black_key(*key) == black) {
            let Some(rect) = key_rect(keys, key, size) else {
                continue;
            };
            scope.draw_round_rect_at(
                rect,
                Brush::vertical_gradient(vec![top, bottom], 0.0, rect.height),
                KEY_CORNERS,
            );
        }
    }
}

// A key that is down: the key in the color of what holds it, lighter where
// it meets the roll.
fn draw_light(scope: &mut dyn DrawScope, keys: KeyRange, key: u8, color: Color) {
    let size = scope.size();
    let Some(rect) = key_rect(keys, key, size) else {
        return;
    };
    let top = color.lerp(&Color::WHITE, LIGHT_TOP);
    scope.draw_round_rect_at(
        rect,
        Brush::vertical_gradient(vec![top, color], 0.0, rect.height),
        KEY_CORNERS,
    );
}

// Draws the keys of `keys`. A key lights in the color of the part of the
// performance around the call that holds it down. A press sounds the key
// with the voicing around the call, and a drag moves the sound from key to
// key; each finger plays its own key.
#[composable]
pub fn Keyboard(keys: KeyRange, colors: KeyboardColors, modifier: Modifier) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let voicing = local_voicing().current();
    let fingers: Fingers = rememberMutableStateOf(Vec::new);
    // The handler outlives a change of voicing, and strikes with the one
    // of the moment.
    let struck_with = rememberUpdatedState(voicing);

    let hand = performance.clone();
    let modifier =
        modifier
            .clip_to_bounds()
            .pointer_input(keys, move |scope: PointerInputScope| {
                let hand = hand.clone();
                async move {
                    scope
                        .await_pointer_event_scope(|events| async move {
                            loop {
                                let event = events.await_pointer_event().await;
                                let size = events.size();
                                let voicing = struck_with.get();
                                if follow_finger(&event, size, keys, fingers, &hand, voicing) {
                                    event.consume();
                                }
                            }
                        })
                        .await;
                }
            });

    CranposeBox(modifier, BoxSpec::default(), move || {
        Canvas(Modifier::empty().fill_max_size(), move |scope| {
            draw_keys(scope, keys, colors);
        });
        let performance = performance.clone();
        Canvas(Modifier::empty().fill_max_size(), move |scope| {
            // A black key over a lit white one keeps its own look, so the
            // white keys light first and the black keys between them are
            // drawn again.
            let mut lit = [None; KEY_COUNT];
            performance.for_each_key_down(|held| lit[usize::from(held.key)] = Some(held.color));
            let by_hand = performance.hand_keys();
            for key in keys.keys().filter(|key| by_hand.contains(*key)) {
                lit[usize::from(key)] = Some(colors.hand);
            }
            if lit.iter().all(Option::is_none) {
                return;
            }
            let size = scope.size();
            for key in keys.keys().filter(|key| !is_black_key(*key)) {
                if let Some(color) = lit[usize::from(key)] {
                    draw_light(scope, keys, key, color);
                }
            }
            for key in keys.keys().filter(|key| is_black_key(*key)) {
                let Some(rect) = key_rect(keys, key, size) else {
                    continue;
                };
                match lit[usize::from(key)] {
                    Some(color) => draw_light(scope, keys, key, color),
                    None => scope.draw_round_rect_at(
                        rect,
                        Brush::vertical_gradient(
                            vec![colors.black_edge, colors.black],
                            0.0,
                            rect.height,
                        ),
                        KEY_CORNERS,
                    ),
                }
            }
        });
    });
}
