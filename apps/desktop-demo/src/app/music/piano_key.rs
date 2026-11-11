// One key of a piano keyboard as a composable. It draws itself, lights in
// the color of the note that holds it down, dips under the note and springs
// back, and sends a glow and a ring up into the roll over it at each strike.

use cranpose_animation::{spring, tween, updateTransition, Animatable, Easing, Lerp};
use cranpose_core::{derivedStateOf, remember, with_current_composer, LaunchedEffect, State};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Brush, Color, CornerRadii, DrawScope, Modifier, Point,
    Rect, Stroke,
};

use super::{held_notes::KeyDown, key_range::is_black_key, performance::local_performance};

pub(super) const KEY_COUNT: usize = 128;
const GAP: f32 = 1.0;
const CORNER: f32 = 3.0;
// How far a lit key is mixed toward white at its top.
const LIGHT_TOP: f32 = 0.55;
// How far a key goes down, and the spring it goes down and comes back with:
// it swings a little past its place both ways.
const DIP: f32 = 3.0;
const PRESS_DAMPING: f32 = 0.5;
const PRESS_STIFFNESS: f32 = 900.0;
// The time the light of a key takes to come and to go.
const LIGHT_MILLIS: u64 = 140;
// The glow over a key that is down: its height and strength at the strike,
// and the share of both it keeps while the key is held.
const GLOW_HEIGHT: f32 = 150.0;
const GLOW_ALPHA: f32 = 0.6;
const GLOW_REST: f32 = 0.3;
// A strike sends a ring up from the key for this long. The glow and the
// spark fall to their rest in the same time.
const STRIKE_MILLIS: u64 = 700;
const RING_RADIUS: std::ops::Range<f32> = 5.0..44.0;
const RING_WIDTH: f32 = 2.0;
const RING_ALPHA: f32 = 0.7;
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

// What a key draws at one moment.
#[derive(Clone, Copy)]
struct KeyPicture {
    // The colors of the key at its top and at its lower end: its own, those
    // of its light, or between the two while the light comes or goes.
    top: Color,
    bottom: Color,
    // How far the key is down: 0 up, 1 down, a little past both in a swing.
    press: f32,
    // How far the ring of the last strike has gone, from 0 to 1.
    ring: f32,
}

impl KeyPicture {
    fn draw(self, scope: &mut dyn DrawScope) {
        let size = scope.size();
        scope.draw_round_rect_at(
            Rect {
                x: 0.0,
                y: DIP * self.press,
                width: size.width,
                height: size.height,
            },
            Brush::vertical_gradient(vec![self.top, self.bottom], 0.0, size.height),
            CORNERS,
        );
        let down = self.press.clamp(0.0, 1.0);
        if down > 0.0 {
            self.draw_strike(scope, down);
        }
    }

    // Draws what stands over a key that is down by the share `down`: its
    // glow, and the ring and the spark of a strike that has not ended.
    fn draw_strike(self, scope: &mut dyn DrawScope, down: f32) {
        let Self {
            bottom: color,
            ring,
            ..
        } = self;
        let size = scope.size();
        let fresh = 1.0 - ring;
        let strength = GLOW_REST + (1.0 - GLOW_REST) * fresh;
        let height = GLOW_HEIGHT * strength;
        // The glow stands on the key's upper edge, over the roll. A gradient
        // is placed from the corner of the shape it paints.
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
                    color.with_alpha(GLOW_ALPHA * strength * down),
                ],
                0.0,
                height,
            ),
        );
        if fresh <= 0.0 {
            return;
        }
        let center = Point {
            x: size.width * 0.5,
            y: 0.0,
        };
        scope.draw_circle_stroked(
            Brush::solid(
                Color::WHITE
                    .lerp(&color, ring)
                    .with_alpha(RING_ALPHA * fresh * down),
            ),
            center,
            RING_RADIUS.start + (RING_RADIUS.end - RING_RADIUS.start) * ring,
            Stroke::new(RING_WIDTH),
        );
        let radius = SPARK_RADIUS * (1.0 + ring);
        scope.draw_circle(
            Brush::radial_gradient(
                vec![
                    Color::WHITE.with_alpha(SPARK_ALPHA * fresh * down),
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
    }
}

// What lights a key: the color of its light, and the strike that put the
// key down, which is the start of a note or nothing for a hand.
#[derive(Clone, Copy, PartialEq)]
struct Light {
    color: Color,
    strike: Option<u32>,
}

// Draws the key `key` in `place`, a rectangle of its keyboard. The key
// lights while `lights` or a hand of the performance around the call holds
// it down. It dips as it goes down and springs back, each strike sends a
// ring up from it, and its light fades when it goes up.
#[composable]
pub fn PianoKey(key: u8, place: Rect, colors: KeyboardColors, lights: State<KeyLights>) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    // The key composes again when what holds it changes, and not with every
    // key.
    let light = derivedStateOf(move || {
        lights
            .with(|lights| lights.0.get(usize::from(key)).copied().flatten())
            .map(|held| Light {
                color: held.color,
                strike: Some(held.since.to_bits()),
            })
            .or_else(|| {
                performance.hand_keys().contains(key).then_some(Light {
                    color: colors.hand,
                    strike: None,
                })
            })
    })
    .get();
    let (top, bottom) = if is_black_key(key) {
        (colors.black_edge, colors.black)
    } else {
        (colors.white, colors.white_edge)
    };

    // The key goes down and takes its light as one transition, and comes
    // back from where it stands when the light goes.
    let transition = updateTransition(light, "PianoKey");
    let press = transition.animateFloat(
        f32::from(u8::from(light.is_some())),
        spring(PRESS_DAMPING, PRESS_STIFFNESS),
        "press",
    );
    let fade = tween(LIGHT_MILLIS, Easing::EaseOut);
    let top = transition.animateColor(
        light.map_or(top, |light| light.color.lerp(&Color::WHITE, LIGHT_TOP)),
        fade,
        "top",
    );
    let bottom = transition.animateColor(light.map_or(bottom, |light| light.color), fade, "bottom");

    // Each strike starts the ring again at the key, also the strike of a
    // note that follows another on the same key with no rest between them.
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let ring = remember(move || Animatable::new(1.0, runtime)).with(Animatable::clone);
    let strike = light.map(|light| light.strike);
    {
        let mut ring = ring.clone();
        LaunchedEffect(strike, async move |_| {
            if strike.is_some() {
                ring.snapTo(0.0);
                ring.animateTo(1.0, tween(STRIKE_MILLIS, Easing::EaseOut));
            }
        });
    }
    let ring = ring.state();

    CranposeBox(
        Modifier::empty()
            .offset(place.x + GAP * 0.5, place.y)
            .size_points((place.width - GAP).max(1.0), place.height)
            .content_description(key_name(key))
            .draw_behind(move |scope| {
                KeyPicture {
                    top: top.get(),
                    bottom: bottom.get(),
                    press: press.get(),
                    ring: ring.get(),
                }
                .draw(scope);
            }),
        BoxSpec::default(),
        || {},
    );
}
