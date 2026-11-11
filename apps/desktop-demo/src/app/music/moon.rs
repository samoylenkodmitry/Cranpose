// The moon over the roll as a composable. Its halo grows and brightens with
// the count of the keys that are down, on a slow spring.

use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::derivedStateOf;
use cranpose_ui::{composable, Brush, Canvas, Color, Modifier, Point};

use super::performance::local_performance;

const MOON: Color = Color::from_rgb_u8(0xDD, 0xE8, 0xFF);
// Where the moon stands, as shares of the width and height of its place.
const PLACE: (f32, f32) = (0.86, 0.62);
const RADIUS: f32 = 30.0;
// The halo with no key down, and what it gains with `FULL_KEYS` keys down.
const HALO_RADIUS: f32 = 260.0;
const HALO_ALPHA: f32 = 0.22;
const HALO_GROWTH: f32 = 0.25;
const HALO_GAIN: f32 = 0.3;
const FULL_KEYS: u8 = 6;
// A spring with no swing that comes to rest in about half a second.
const GLOW_DAMPING: f32 = 1.0;
const GLOW_STIFFNESS: f32 = 40.0;

// The middle of a circle of `radius` for its gradient, which is placed from
// the corner of the square around the circle.
fn middle_of(radius: f32) -> Point {
    Point {
        x: radius,
        y: radius,
    }
}

// Draws a moon in its place. In a performance its halo follows the keys that
// are down, those of the notes and those under a hand.
#[composable]
pub(super) fn Moon(modifier: Modifier) {
    let performance = local_performance().current();
    // The moon composes again when the count of the keys changes, and not
    // with each move of the clock.
    let keys_down = derivedStateOf(move || {
        let Some(performance) = &performance else {
            return 0;
        };
        let mut keys = performance.hand_keys().len();
        performance.for_each_key_down(|_| keys += 1);
        u8::try_from(keys).map_or(FULL_KEYS, |keys| keys.min(FULL_KEYS))
    })
    .get();
    let glow = animateFloatAsState(
        f32::from(keys_down) / f32::from(FULL_KEYS),
        spring(GLOW_DAMPING, GLOW_STIFFNESS),
        "Moon glow",
    );

    Canvas(modifier, move |scope| {
        let size = scope.size();
        let glow = glow.get();
        let moon = Point {
            x: size.width * PLACE.0,
            y: size.height * PLACE.1,
        };
        let halo = HALO_RADIUS * (1.0 + HALO_GROWTH * glow);
        scope.draw_circle(
            Brush::radial_gradient(
                vec![
                    MOON.with_alpha(HALO_ALPHA + HALO_GAIN * glow),
                    MOON.with_alpha(0.0),
                ],
                middle_of(halo),
                halo,
            ),
            moon,
            halo,
        );
        scope.draw_circle(
            Brush::radial_gradient(vec![Color::WHITE, MOON], middle_of(RADIUS), RADIUS),
            moon,
            RADIUS,
        );
    });
}
