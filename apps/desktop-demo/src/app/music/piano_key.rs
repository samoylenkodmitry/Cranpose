// One key of a piano keyboard as a composable. It draws itself, lights in
// the color of the note that holds it down, and dips under the note and
// springs back. Where the note lands on it, light blooms at its upper edge,
// and each strike throws sparks up into the roll.

use cranpose_animation::{spring, tween, updateTransition, Animatable, Easing, Lerp};
use cranpose_core::{derivedStateOf, remember, with_current_composer, LaunchedEffect, State};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Brush, Color, CornerRadii, DrawScope, Modifier, Point,
    Rect,
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
// Where a drop lands on its key, light blooms at the key's upper edge. The
// bloom stays while the key is held, at this share of what it is at the
// strike, and a strike takes this long to settle.
const BLOOM_REST: f32 = 0.6;
const STRIKE_MILLIS: u64 = 900;
// The white middle of the bloom, and the haze of the key's light around it.
const CORE_RADIUS: f32 = 15.0;
const HAZE_RADIUS: f32 = 36.0;
const HAZE_ALPHA: f32 = 0.5;
// The beam the bloom sends up into the water over the key.
const BEAM_HEIGHT: f32 = 110.0;
const BEAM_ALPHA: f32 = 0.42;
// The flare along the edge of the keys: how far it reaches to each side at
// rest, what it gains at the strike, and its height.
const FLARE_REACH: f32 = 40.0;
const FLARE_GAIN: f32 = 34.0;
const FLARE_HEIGHT: f32 = 3.0;
// A strike throws this many sparks up from the key: how far they fly, how
// far they fall back by the end, and their radius.
const SPARKS: u8 = 12;
const SPARK_REACH: f32 = 60.0;
const SPARK_FALL: f32 = 22.0;
const SPARK_RADIUS: f32 = 1.3;
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
    // How far the last strike has settled, from 0 to 1.
    settled: f32,
    // What tells the splash of this key from that of another.
    seed: f32,
}

// A share from 0 to 1 that stays the same for one key and one drop of
// its splash, and differs between them.
fn scatter(seed: f32, salt: f32) -> f32 {
    ((seed * 12.9898 + salt * 78.233).sin() * 43_758.547)
        .fract()
        .abs()
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

    // Draws the bloom over a key that is down by the share `down`, and the
    // sparks of a strike that has not settled.
    fn draw_strike(self, scope: &mut dyn DrawScope, down: f32) {
        let Self {
            bottom: color,
            settled,
            ..
        } = self;
        let size = scope.size();
        let fresh = 1.0 - settled;
        let strength = (BLOOM_REST + (1.0 - BLOOM_REST) * fresh) * down;
        let hot = color.lerp(&Color::WHITE, 0.55);
        let center = Point {
            x: size.width * 0.5,
            y: 0.0,
        };
        // A gradient is placed from the corner of the shape it paints.
        let around = |radius: f32| Point {
            x: radius,
            y: radius,
        };
        let haze = HAZE_RADIUS * (0.8 + 0.5 * fresh);
        scope.draw_circle(
            Brush::radial_gradient(
                vec![
                    color.with_alpha(HAZE_ALPHA * strength),
                    color.with_alpha(0.0),
                ],
                around(haze),
                haze,
            ),
            center,
            haze,
        );
        let beam = BEAM_HEIGHT * (0.6 + 0.4 * fresh);
        scope.draw_rect_at(
            Rect {
                x: -size.width * 0.1,
                y: -beam,
                width: size.width * 1.2,
                height: beam,
            },
            Brush::vertical_gradient(
                vec![
                    color.with_alpha(0.0),
                    color.with_alpha(BEAM_ALPHA * strength),
                ],
                0.0,
                beam,
            ),
        );
        draw_flare(
            scope,
            center,
            FLARE_REACH + FLARE_GAIN * fresh,
            hot,
            strength,
        );
        let core = CORE_RADIUS * (0.8 + 0.7 * fresh);
        scope.draw_circle(
            Brush::radial_gradient(
                vec![
                    Color::WHITE.with_alpha(strength.min(1.0)),
                    hot.with_alpha(0.6 * strength),
                    color.with_alpha(0.0),
                ],
                around(core),
                core,
            ),
            center,
            core,
        );
        if fresh > 0.0 {
            self.draw_sparks(scope, center, hot.with_alpha(fresh * down));
        }
    }

    // Draws the sparks of a strike that has not settled: they fly up and out
    // from `center`, fast at first, and fall back a little.
    fn draw_sparks(self, scope: &mut dyn DrawScope, center: Point, light: Color) {
        let flown = 1.0 - (1.0 - self.settled) * (1.0 - self.settled);
        for spark in 0..SPARKS {
            let salt = f32::from(spark);
            // From 80 degrees to the left of straight up to 80 to the right.
            let angle = (scatter(self.seed, salt) - 0.5) * 2.8;
            let reach = SPARK_REACH * (0.25 + 0.75 * scatter(self.seed, salt + 9.0)) * flown;
            scope.draw_circle(
                Brush::solid(light),
                Point {
                    x: center.x + angle.sin() * reach,
                    y: center.y - angle.cos() * reach + SPARK_FALL * self.settled * self.settled,
                },
                SPARK_RADIUS * (1.0 - 0.4 * self.settled),
            );
        }
    }
}

// Draws the flare of a landing along the edge of the keys, `reach` to each
// side of `center`: a wide soft one in the light `hot`, and a thin white one
// over it.
fn draw_flare(scope: &mut dyn DrawScope, center: Point, reach: f32, hot: Color, strength: f32) {
    for (reach, height, light) in [
        (reach, FLARE_HEIGHT * 4.0, hot.with_alpha(0.45 * strength)),
        (
            reach * 0.7,
            FLARE_HEIGHT,
            Color::WHITE.with_alpha(0.95 * strength),
        ),
    ] {
        // A gradient is placed from the corner of the shape it paints.
        scope.draw_rect_at(
            Rect {
                x: center.x - reach,
                y: -height * 0.5,
                width: reach * 2.0,
                height,
            },
            Brush::horizontal_gradient(
                vec![light.with_alpha(0.0), light, light.with_alpha(0.0)],
                0.0,
                reach * 2.0,
            ),
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
// it down. It dips as it goes down and springs back, light blooms where a
// note lands on it, each strike throws sparks up from it, and its light
// fades when it goes up.
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

    // Each strike starts the bloom again at the key, also the strike of a
    // note that follows another on the same key with no rest between them.
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let strike_light = remember(move || Animatable::new(1.0, runtime)).with(Animatable::clone);
    let struck = light.map(|light| light.strike);
    {
        let mut strike_light = strike_light.clone();
        LaunchedEffect(struck, async move |_| {
            if struck.is_some() {
                strike_light.snapTo(0.0);
                strike_light.animateTo(1.0, tween(STRIKE_MILLIS, Easing::EaseOut));
            }
        });
    }
    let strike = strike_light.state();

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
                    settled: strike.get(),
                    seed: f32::from(key),
                }
                .draw(scope);
            }),
        BoxSpec::default(),
        || {},
    );
}
