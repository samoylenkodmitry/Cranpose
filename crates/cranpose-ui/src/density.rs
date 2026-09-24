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
//! the Compose build draws a crisp one. A widget that must draw a crisp edge
//! therefore rounds its own sizes and offsets through [`Density`] rather than
//! trusting the ambient float arithmetic. The Wear widgets do this throughout;
//! the rest of the library does not, which is where a soft edge still comes
//! from.
//!
//! One length unit runs through all of it: a Cranpose layout point is a dp.
//! `Density` converts to and from device pixels and does the rounding on
//! the pixel side, which is the only side where rounding means anything.

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};

use crate::{
    font_scale::FontScaleCurve,
    render_state::{current_density, current_font_scale_curve},
    round_scaling_list::round_to_px,
};

/// The device pixel grid a layout measures against: how many device pixels
/// a layout point is worth, and how the platform converts a text size.
///
/// This is Compose's `Density`, and [`local_density`] is its `LocalDensity`.
///
/// Read it in a composable through [`density`], which subscribes to the
/// composition local rather than to process-wide state, so a subtree given its
/// own density recomposes only what read it. Tests and goldens construct one
/// directly so a measurement does not depend on the machine it runs on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density {
    density: f32,
    font_scale: FontScaleCurve,
}

impl Density {
    /// The grid the host installed on this shell.
    ///
    /// This is the composition local's default, not the way a composable should
    /// read the density -- use [`density`] for that, so a subtree provided its
    /// own grid is honoured.
    pub fn from_host() -> Self {
        if crate::render_state::has_current_app_context() {
            Self::with_curve(current_density(), current_font_scale_curve())
        } else {
            Self::default()
        }
    }

    /// A grid stated outright, for a test or a golden. The setting is taken as
    /// a plain multiplier, which is what it is wherever the platform reports no
    /// conversion of its own.
    pub fn new(density: f32, font_scale: f32) -> Self {
        let font_scale = if font_scale.is_finite() && font_scale > 0.0 {
            font_scale
        } else {
            1.0
        };
        Self::with_curve(density, FontScaleCurve::linear(font_scale))
    }

    /// A grid whose text size follows the platform's own `Sp` conversion rather
    /// than a multiplier. See [`crate::font_scale`].
    pub fn with_curve(density: f32, font_scale: FontScaleCurve) -> Self {
        Self {
            density: if density.is_finite() && density > 0.0 {
                density
            } else {
                1.0
            },
            font_scale,
        }
    }

    /// Device pixels per layout point.
    pub fn density(self) -> f32 {
        self.density
    }

    /// The user's text size setting.
    pub fn font_scale(self) -> f32 {
        self.font_scale.scale()
    }

    /// The conversion the platform performs for a size in `Sp`.
    pub fn font_scale_curve(self) -> FontScaleCurve {
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
    /// measured with [`Density::dp`] instead — that is the whole difference
    /// between the two.
    pub fn sp(self, value: f32) -> f32 {
        self.dp(self.font_scale.sp_to_dp(value))
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

impl Default for Density {
    fn default() -> Self {
        Self::new(1.0, 1.0)
    }
}

/// The production [`cranpose_ui_layout::MeasureScope`]: a grid captured at
/// composition time, carried through measurement.
///
/// `MeasurePolicy::measure` runs with this as its scope, the way Compose's
/// `MeasurePolicy.measure` runs with `MeasureScope` as its receiver. The grid
/// is the [`LayoutNode`](crate::widgets::nodes::LayoutNode)'s own
/// [`Density`] -- read at composition time by the `Layout` composable from
/// [`density`], not the host default -- so a subtree given a different grid
/// through [`ProvideDensity`] is measured on that grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DensityMeasureScope {
    density: Density,
}

impl DensityMeasureScope {
    /// Wraps a captured [`Density`] as a measurement scope.
    pub fn new(density: Density) -> Self {
        Self { density }
    }
}

impl cranpose_ui_layout::MeasureScope for DensityMeasureScope {
    fn density(&self) -> f32 {
        self.density.density()
    }

    fn font_scale(&self) -> f32 {
        self.density.font_scale()
    }
}

/// The [`CompositionLocal`] carrying the device pixel grid.
///
/// Compose's `LocalDensity`. Its default is whatever grid the host installed on
/// this shell, so an application that never provides one still measures against
/// the real screen; providing one scopes a different grid to a subtree, which
/// is what a preview, a scaled container or a density-specific golden needs.
pub fn local_density() -> CompositionLocal<Density> {
    thread_local! {
        static LOCAL: std::cell::RefCell<Option<CompositionLocal<Density>>> =
            const { std::cell::RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(Density::from_host))
            .clone()
    })
}

/// The device pixel grid in force here.
pub fn density() -> Density {
    local_density().current()
}

/// Runs `content` on `density`.
///
/// Compose's `CompositionLocalProvider(LocalDensity provides ...)`. A preview, a
/// scaled container or a golden that must not depend on the machine it runs on
/// states its grid here instead of moving the whole shell onto it.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideDensity(density: Density, content: impl FnOnce()) {
    CompositionLocalProvider(vec![local_density().provides(density)], content);
}

#[cfg(test)]
#[path = "tests/density_tests.rs"]
mod tests;
