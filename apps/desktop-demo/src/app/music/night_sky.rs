// The sky behind the roll: a night and its stars, for a piece named after
// moonlight.

use cranpose_ui::{composable, Brush, Canvas, Color, Modifier, Point};

const ZENITH: Color = Color::from_rgb_u8(0x03, 0x06, 0x12);
const MIDDLE: Color = Color::from_rgb_u8(0x0A, 0x12, 0x2E);
const HORIZON: Color = Color::from_rgb_u8(0x1B, 0x1F, 0x4D);
const STAR: Color = Color::from_rgb_u8(0xDD, 0xE8, 0xFF);
const STARS: u16 = 110;
// The stars keep to this share of the sky from its top.
const STARS_REACH: f32 = 0.8;
const STAR_ALPHA: f32 = 0.75;

// A share from 0 to 1 that stays the same for one star and one `salt`, and
// differs from star to star.
fn scatter(star: u16, salt: f32) -> f32 {
    ((f32::from(star) * 12.9898 + salt * 78.233).sin() * 43_758.547)
        .fract()
        .abs()
}

// Fills its place with the night. Nothing here changes, so the sky is drawn
// once.
#[composable]
pub(super) fn NightSky(modifier: Modifier) {
    Canvas(modifier, |scope| {
        let size = scope.size();
        scope.draw_rect(Brush::vertical_gradient(
            vec![ZENITH, MIDDLE, HORIZON],
            0.0,
            size.height,
        ));
        for star in 0..STARS {
            let center = Point {
                x: scatter(star, 1.0) * size.width,
                y: scatter(star, 2.0) * size.height * STARS_REACH,
            };
            scope.draw_circle(
                Brush::solid(STAR.with_alpha(STAR_ALPHA * (0.35 + 0.65 * scatter(star, 3.0)))),
                center,
                0.6 + scatter(star, 5.0) * 1.1,
            );
        }
    });
}
