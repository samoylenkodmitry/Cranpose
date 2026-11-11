// A piano keyboard as a composable: it lights the keys the parts of the
// performance hold down, and a press on a key sounds it.

use cranpose_core::{rememberMutableStateOf, rememberUpdatedState, MutableState};
use cranpose_foundation::{PointerEvent, PointerEventKind, PointerId};
use cranpose_ui::{
    composable, Brush, Canvas, Color, CornerRadii, DrawScope, Modifier, PointerInputScope, Rect,
    Size,
};

use super::{
    key_range::{is_black_key, KeyRange},
    key_set::KeySet,
    performance::{local_performance, Performance},
    voicing::{local_voicing, Voicing},
};

const KEY_GAP: f32 = 1.0;
const KEY_CORNER: f32 = 2.0;

// The colors of a keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyboardColors {
    pub white: Color,
    pub black: Color,
    pub lit: Color,
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

fn draw_keys(scope: &mut dyn DrawScope, keys: KeyRange, colors: KeyboardColors, down: KeySet) {
    let size = scope.size();
    scope.draw_rect(Brush::solid(colors.gap));
    let black_height = size.height * keys.black_key_height();
    // The black keys lie over the white ones.
    for black in [false, true] {
        for key in keys.keys().filter(|key| is_black_key(*key) == black) {
            let Some((left, width)) = keys.key_span(key, size.width) else {
                continue;
            };
            let color = match (down.contains(key), black) {
                (true, _) => colors.lit,
                (false, true) => colors.black,
                (false, false) => colors.white,
            };
            scope.draw_round_rect_at(
                Rect {
                    x: left + KEY_GAP * 0.5,
                    y: 0.0,
                    width: (width - KEY_GAP).max(1.0),
                    height: if black { black_height } else { size.height },
                },
                Brush::solid(color),
                CornerRadii {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_right: KEY_CORNER,
                    bottom_left: KEY_CORNER,
                },
            );
        }
    }
}

// Draws the keys of `keys`. A key lights while a part of the performance
// around the call holds it down. A press sounds the key with the voicing
// around the call, and a drag moves the sound from key to key.
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

    Canvas(modifier, move |scope| {
        let mut down = performance.keys_down();
        fingers.with(|fingers| {
            for (_, key) in fingers {
                let sounded = i16::from(*key) + i16::from(voicing.transpose);
                if let Ok(sounded) = u8::try_from(sounded) {
                    down.insert(sounded);
                }
            }
        });
        draw_keys(scope, keys, colors, down);
    });
}
