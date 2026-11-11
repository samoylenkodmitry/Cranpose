// A piano keyboard as a composable: a row of `PianoKey`s. It lights the
// keys the notes of the performance hold down, and a press on a key sounds
// it. The keyboard takes the pointer for all of its keys, since a drag goes
// from key to key.

use cranpose_core::{derivedStateOf, rememberMutableStateOf, rememberUpdatedState, MutableState};
use cranpose_foundation::{PointerEvent, PointerEventKind, PointerId};
use cranpose_ui::{
    composable, BoxWithConstraints, BoxWithConstraintsScope, Modifier, PointerInputScope, Rect,
    Size,
};

use super::{
    key_range::{is_black_key, KeyRange},
    performance::{local_performance, Performance},
    piano_key::{KeyLights, KeyboardColors, PianoKey, KEY_COUNT},
    voicing::{local_voicing, Voicing},
};

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
fn key_place(keys: KeyRange, key: u8, size: Size) -> Option<Rect> {
    let (left, width) = keys.key_span(key, size.width)?;
    let height = if is_black_key(key) {
        size.height * keys.black_key_height()
    } else {
        size.height
    };
    Some(Rect {
        x: left,
        y: 0.0,
        width,
        height,
    })
}

// Lays out the keys of `keys`, each a `PianoKey`. A key lights in the color
// of the note of the performance around the call that holds it down. A
// press sounds the key with the voicing around the call, and a drag moves
// the sound from key to key; each finger plays its own key.
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
    // What holds each key, worked out once for all the keys when the clock
    // moves; a key hears of it when its own entry changes.
    let lights = {
        let performance = performance.clone();
        derivedStateOf(move || {
            let mut lights = [None; KEY_COUNT];
            performance.for_each_key_down(|down| {
                // A part transposed past the last key lights none.
                if let Some(light) = lights.get_mut(usize::from(down.key)) {
                    *light = Some(down);
                }
            });
            KeyLights(lights)
        })
    };

    let modifier =
        modifier
            .background(colors.gap)
            .pointer_input(keys, move |scope: PointerInputScope| {
                let hand = performance.clone();
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

    BoxWithConstraints(modifier, move |scope| {
        let size = Size::new(
            scope.constraints().max_width,
            scope.constraints().max_height,
        );
        // The black keys lie over the white ones.
        for black in [false, true] {
            for key in keys.keys().filter(|key| is_black_key(*key) == black) {
                if let Some(place) = key_place(keys, key, size) {
                    PianoKey(key, place, colors, lights);
                }
            }
        }
    });
}
