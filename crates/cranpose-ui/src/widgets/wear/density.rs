//! Integral layout, the way Compose does it.
//!
//! Cranpose's layout is continuous: [`Constraints`](cranpose_ui_layout::Constraints)
//! is four `f32`, a child is placed at an `f32` offset, and nothing in
//! `cranpose-ui-layout` rounds. Compose's is integral — `Dp.roundToPx()` turns
//! every padding, minimum size and spacing into an `Int` before anything is
//! measured, and `Placeable.placeAt` takes an `IntOffset`.
//!
//! That difference is not cosmetic. Half a pixel of drift moves a line of
//! text's antialiasing onto the next row, and it puts a soft edge everywhere
//! the Compose build draws a crisp one. Every Wear widget in this module
//! therefore rounds its own sizes and offsets through [`WearDensity`] rather
//! than trusting the ambient float arithmetic.
//!
//! One length unit runs through all of it: a Cranpose layout point is a dp.
//! `WearDensity` converts to and from device pixels and does the rounding on
//! the pixel side, which is the only side where rounding means anything.

use crate::render_state::{current_density, current_font_scale};
use crate::round_scaling_list::round_to_px;

/// The device pixel grid a Wear widget measures against.
///
/// A measure policy has no scope parameter and so cannot be handed a density;
/// [`WearDensity::current`] reads the ambient one the host installed. Tests and
/// goldens construct one directly so a measurement does not depend on the
/// machine it runs on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearDensity {
    density: f32,
    font_scale: f32,
}

impl WearDensity {
    /// The grid the running app is on.
    pub fn current() -> Self {
        Self::new(current_density(), current_font_scale())
    }

    /// A grid stated outright, for a test or a golden.
    pub fn new(density: f32, font_scale: f32) -> Self {
        Self {
            density: if density.is_finite() && density > 0.0 {
                density
            } else {
                1.0
            },
            font_scale: if font_scale.is_finite() && font_scale > 0.0 {
                font_scale
            } else {
                1.0
            },
        }
    }

    /// Device pixels per layout point.
    pub fn density(self) -> f32 {
        self.density
    }

    /// The user's text size setting.
    pub fn font_scale(self) -> f32 {
        self.font_scale
    }

    /// `Dp.roundToPx()`, expressed back in points: a dp length snapped to the
    /// whole device pixel Compose would give it.
    pub fn dp(self, value: f32) -> f32 {
        round_to_px(value, self.density)
    }

    /// A text size in scale-independent pixels, snapped the same way.
    ///
    /// Anything that must not move when the user changes their text size is
    /// measured with [`WearDensity::dp`] instead — that is the whole difference
    /// between the two.
    pub fn sp(self, value: f32) -> f32 {
        self.dp(value * self.font_scale)
    }

    /// A length snapped to the nearest whole device pixel, exact halves up.
    pub fn round(self, value: f32) -> f32 {
        round_to_px(value, self.density)
    }

    /// A length snapped down to a whole device pixel.
    pub fn floor(self, value: f32) -> f32 {
        if value.is_finite() {
            (value * self.density).floor() / self.density
        } else {
            value
        }
    }

    /// A length snapped up to a whole device pixel.
    ///
    /// This is what a line box does with its own height, and what a container
    /// does with a content size it must not clip.
    pub fn ceil(self, value: f32) -> f32 {
        if value.is_finite() {
            (value * self.density).ceil() / self.density
        } else {
            value
        }
    }

    /// Points to device pixels.
    pub fn to_px(self, points: f32) -> f32 {
        points * self.density
    }

    /// Device pixels to points.
    pub fn to_points(self, pixels: f32) -> f32 {
        pixels / self.density
    }

    /// Centres `content` in `available` the way Compose does: on a whole pixel.
    ///
    /// `Arrangement.Center` measures every child in whole pixels and rounds the
    /// leading gap before placing any of them, so a column whose content is an
    /// odd number of pixels tall starts on a pixel boundary rather than across
    /// one. Halving in floats instead is the single easiest way to soften every
    /// edge in a row.
    pub fn centre(self, available: f32, content: f32) -> f32 {
        self.round((available - content) * 0.5)
    }
}

impl Default for WearDensity {
    fn default() -> Self {
        Self::new(1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dp_length_lands_on_a_whole_device_pixel() {
        let density = WearDensity::new(2.0, 1.0);
        assert_eq!(density.dp(13.3), 13.5);
        assert_eq!(density.to_px(density.dp(13.3)), 27.0);
        // An exact half goes up, as Kotlin's `roundToInt` does.
        assert_eq!(density.dp(0.25), 0.5);
    }

    #[test]
    fn a_text_size_moves_with_the_font_scale_and_a_dp_length_does_not() {
        let density = WearDensity::new(2.0, 1.24);
        assert_eq!(density.dp(15.0), 15.0);
        // 15sp at 1.24 is 18.6dp = 37.2px, which is 37px.
        assert_eq!(density.to_px(density.sp(15.0)), 37.0);
    }

    #[test]
    fn centring_puts_the_leading_gap_on_a_pixel_boundary() {
        let density = WearDensity::new(2.0, 1.0);
        // 104px of room, 93px of content: 5.5px above, which rounds up to a
        // whole 6px rather than landing across a pixel boundary.
        let leading = density.centre(52.0, 46.5);
        assert_eq!(density.to_px(leading), 6.0);
    }

    #[test]
    fn floor_and_ceil_move_whole_pixels_only() {
        let density = WearDensity::new(2.0, 1.0);
        assert_eq!(density.floor(13.3), 13.0);
        assert_eq!(density.ceil(13.3), 13.5);
        assert_eq!(density.ceil(13.5), 13.5);
    }

    #[test]
    fn a_nonsense_grid_falls_back_to_one_rather_than_dividing_by_zero() {
        let density = WearDensity::new(0.0, f32::NAN);
        assert_eq!(density.density(), 1.0);
        assert_eq!(density.font_scale(), 1.0);
        assert_eq!(density.dp(3.7), 4.0);
    }
}
