// One key of a piano keyboard as a composable. It draws itself, lights in
// the color of the note that holds it down, and glows up into the roll over
// it: bright as it goes down, fading while it is held.

use cranpose_animation::Lerp;
use cranpose_core::{derivedStateOf, State};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Brush, Color, CornerRadii, Modifier, Point, Rect,
};

use super::{
    held_notes::KeyDown, key_range::is_black_key, performance::local_performance,
    song::note_seconds,
};

pub(super) const KEY_COUNT: usize = 128;
const GAP: f32 = 1.0;
const CORNER: f32 = 3.0;
// How far a lit key is mixed toward white at its top.
const LIGHT_TOP: f32 = 0.55;
// The glow over a key that is down: its height and strength at the strike,
// the share of both it keeps while the key is held, and how fast it falls
// there, per second.
const GLOW_HEIGHT: f32 = 150.0;
const GLOW_ALPHA: f32 = 0.6;
const GLOW_REST: f32 = 0.3;
const GLOW_FADE: f32 = 3.0;
const SPARK_RADIUS: f32 = 14.0;
const SPARK_ALPHA: f32 = 0.9;
const CORNERS: CornerRadii = CornerRadii {
    top_left: 0.0,
    top_right: 0.0,
    bottom_right: CORNER,
    bottom_left: CORNER,
};
const NAMES: [&str; 12] = [
    "C", "C sharp", "D", "D sharp", "E", "F", "F sharp", "G", "G sharp", "A", "A sharp", "B",
];

// The colors of the keys of a keyboard.
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

// What holds each key of a keyboard down, by the key's MIDI number.
#[derive(Clone, Copy, PartialEq)]
pub struct KeyLights(pub [Option<KeyDown>; KEY_COUNT]);

// The name of a key as a screen reader says it: its note and its octave.
fn key_name(key: u8) -> String {
    let octave = i16::from(key / 12) - 1;
    format!("{} {octave}", NAMES[usize::from(key % 12)])
}

// Draws the key `key` in `place`, a rectangle of its keyboard. The key
// lights while `lights` or a hand of the performance around the call holds
// it down.
#[composable]
pub fn PianoKey(key: u8, place: Rect, colors: KeyboardColors, lights: State<KeyLights>) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let black = is_black_key(key);
    // The key redraws when what holds it changes, and not with every key.
    let light = derivedStateOf(move || {
        lights.with(|lights| lights.0.get(usize::from(key)).copied().flatten())
    });

    CranposeBox(
        Modifier::empty()
            .offset(place.x + GAP * 0.5, place.y)
            .size_points((place.width - GAP).max(1.0), place.height)
            .content_description(key_name(key))
            .draw_behind(move |scope| {
                let size = scope.size();
                let held = light.get();
                let by_hand = performance.hand_keys().contains(key);
                let lit = held
                    .map(|held| held.color)
                    .or_else(|| by_hand.then_some(colors.hand));
                let (top, bottom) = match (lit, black) {
                    (Some(color), _) => (color.lerp(&Color::WHITE, LIGHT_TOP), color),
                    (None, true) => (colors.black_edge, colors.black),
                    (None, false) => (colors.white, colors.white_edge),
                };
                scope.draw_round_rect(
                    Brush::vertical_gradient(vec![top, bottom], 0.0, size.height),
                    CORNERS,
                );
                let Some(color) = lit else {
                    return;
                };
                // A key a note holds reads the clock here, so its glow fades
                // frame by frame; no other key does.
                let fresh = held.map_or(0.0, |held| {
                    let down_for = note_seconds(performance.seconds()) - held.since;
                    (-down_for.max(0.0) * GLOW_FADE).exp()
                });
                let strength = GLOW_REST + (1.0 - GLOW_REST) * fresh;
                let height = GLOW_HEIGHT * strength;
                // The glow stands on the key's upper edge, over the roll. A
                // gradient is placed from the corner of the shape it paints.
                scope.draw_rect_at(
                    Rect {
                        x: -size.width * 0.5,
                        y: -height,
                        width: size.width * 2.0,
                        height,
                    },
                    Brush::vertical_gradient(
                        vec![
                            color.with_alpha(0.0),
                            color.with_alpha(GLOW_ALPHA * strength),
                        ],
                        0.0,
                        height,
                    ),
                );
                let center = Point {
                    x: size.width * 0.5,
                    y: 0.0,
                };
                let radius = SPARK_RADIUS * (2.0 - fresh);
                scope.draw_circle(
                    Brush::radial_gradient(
                        vec![
                            Color::WHITE.with_alpha(SPARK_ALPHA * fresh),
                            color.with_alpha(0.0),
                        ],
                        Point {
                            x: radius,
                            y: radius,
                        },
                        radius,
                    ),
                    center,
                    radius,
                );
            }),
        BoxSpec::default(),
        || {},
    );
}
