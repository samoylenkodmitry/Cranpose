//! Rotary input events (Wear OS crown / rotating bezel).
//!
//! This mirrors Jetpack Compose for Wear OS's
//! `androidx.compose.ui.input.rotary` package: a [`RotaryScrollEvent`] carries
//! a scroll amount **already converted to pixels**, and handlers return `true`
//! to consume the event and stop it propagating.
//!
//! # Sign convention
//!
//! Android reports the crown/bezel delta on `MotionEvent.AXIS_SCROLL` in
//! *detents*, where a **positive** value means the user scrolled *up / away
//! from themselves*. Compose negates that value before scaling it to pixels,
//! so a positive `AXIS_SCROLL` becomes a **negative**
//! [`RotaryScrollEvent::vertical_scroll_pixels`].
//!
//! Evidence — `AndroidComposeView.android.kt`, `handleRotaryEvent`
//! (androidx-main):
//!
//! ```text
//! private fun handleRotaryEvent(event: MotionEvent): Boolean {
//!     val config = android.view.ViewConfiguration.get(context)
//!     val axisValue = -event.getAxisValue(AXIS_SCROLL)
//!     val rotaryEvent =
//!         RotaryScrollEvent(
//!             verticalScrollPixels = axisValue * getScaledVerticalScrollFactor(config, context),
//!             horizontalScrollPixels =
//!                 axisValue * getScaledHorizontalScrollFactor(config, context),
//!             uptimeMillis = event.eventTime,
//!             inputDeviceId = event.deviceId,
//!         )
//!     ...
//! }
//! ```
//!
//! Note that Compose feeds the *same* (negated) `AXIS_SCROLL` value into both
//! the vertical and the horizontal field, scaled by the respective scroll
//! factor — it does **not** read `AXIS_HSCROLL`. A rotary encoder has one
//! degree of freedom; the two fields exist so a horizontally scrolling
//! container can consume the same gesture. [`RotaryScrollEvent::from_detents`]
//! reproduces this exactly.
//!
//! The resulting pixel value is a *scroll amount in content space*: it can be
//! handed straight to a scroll container's "scroll by N pixels" API with the
//! same meaning a wheel/touch scroll delta would have.

/// Android's default vertical scroll factor expressed in density-independent
/// pixels.
///
/// Android derives the real factor from the current theme's
/// `listPreferredItemHeight` via
/// `ViewConfiguration.getScaledVerticalScrollFactor()`, which needs a JVM
/// `Context`. Cranpose's Android backend has no JNI dependency in its input
/// path, so it defaults to this value scaled by the display density and lets
/// the host override it with the exact platform number (see
/// `AppShell::set_rotary_scroll_factor`).
pub const DEFAULT_ROTARY_SCROLL_FACTOR_DP: f32 = 64.0;

/// A rotary scroll event produced by a Wear OS crown or rotating bezel.
///
/// Field-for-field equivalent to Compose's
/// `androidx.compose.ui.input.rotary.RotaryScrollEvent`. The scroll amounts are
/// in **pixels**, not detents: the platform layer has already multiplied the
/// raw axis value by the system scroll factor.
///
/// This type is `Copy` and contains no heap data, so dispatching one allocates
/// nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotaryScrollEvent {
    /// How far to scroll (in pixels) in a container that scrolls vertically.
    ///
    /// Negative when the user turned the crown "up"/away (positive
    /// `AXIS_SCROLL` on Android); see the [module docs](self) for the
    /// derivation.
    pub vertical_scroll_pixels: f32,
    /// How far to scroll (in pixels) in a container that scrolls horizontally.
    pub horizontal_scroll_pixels: f32,
    /// Time in milliseconds at which the event occurred. The zero point is
    /// platform-dependent (Android's uptime clock, the process start elsewhere),
    /// so only differences between events are meaningful.
    pub uptime_millis: u64,
}

impl RotaryScrollEvent {
    /// Creates a rotary event from scroll amounts that are already in pixels.
    pub const fn new(
        vertical_scroll_pixels: f32,
        horizontal_scroll_pixels: f32,
        uptime_millis: u64,
    ) -> Self {
        Self {
            vertical_scroll_pixels,
            horizontal_scroll_pixels,
            uptime_millis,
        }
    }

    /// Builds a rotary event from a raw Android `AXIS_SCROLL` value in detents.
    ///
    /// `detents` is the value straight out of
    /// `AMotionEvent_getAxisValue(event, AMOTION_EVENT_AXIS_SCROLL, 0)`; the
    /// scroll factors are `ViewConfiguration.getScaledVerticalScrollFactor()`
    /// and `getScaledHorizontalScrollFactor()`.
    ///
    /// The detent value is negated exactly once, matching Compose (see the
    /// [module docs](self)), and the *same* negated value feeds both axes.
    pub fn from_detents(
        detents: f32,
        vertical_scroll_factor: f32,
        horizontal_scroll_factor: f32,
        uptime_millis: u64,
    ) -> Self {
        let axis_value = -detents;
        Self {
            vertical_scroll_pixels: axis_value * vertical_scroll_factor,
            horizontal_scroll_pixels: axis_value * horizontal_scroll_factor,
            uptime_millis,
        }
    }

    /// Builds a rotary event from a mouse-wheel / trackpad sample already in
    /// logical pixels.
    ///
    /// The final target for rotary input is a Wear OS crown, but the machines
    /// it is developed on have wheels, so every desktop-class host offers its
    /// wheel to the rotary handlers first. `vertical` and `horizontal` are in
    /// the shell's wheel convention — positive when the content should move
    /// down and right, the direction a wheel turned *up* produces — which is
    /// the same physical direction as a positive Android detent. Both are
    /// therefore negated exactly once, like [`from_detents`](Self::from_detents),
    /// so a wheel turned up and a crown turned up land on the same negative
    /// [`vertical_scroll_pixels`](Self::vertical_scroll_pixels).
    ///
    /// Unlike the detent path, a wheel really does have two axes, so each one
    /// is carried through on its own rather than fanned out from a single
    /// value.
    pub fn from_wheel_pixels(vertical: f32, horizontal: f32, uptime_millis: u64) -> Self {
        Self {
            vertical_scroll_pixels: -vertical,
            horizontal_scroll_pixels: -horizontal,
            uptime_millis,
        }
    }

    /// Returns true when neither axis carries a usable scroll amount.
    ///
    /// Non-finite values (NaN/inf from a misbehaving driver) count as empty so
    /// they are dropped at the ingress instead of poisoning scroll offsets.
    pub fn is_empty(&self) -> bool {
        let vertical_dead =
            !self.vertical_scroll_pixels.is_finite() || self.vertical_scroll_pixels == 0.0;
        let horizontal_dead =
            !self.horizontal_scroll_pixels.is_finite() || self.horizontal_scroll_pixels == 0.0;
        vertical_dead && horizontal_dead
    }
}

/// Turns continuous rotary travel into discrete steps while retaining sub-step
/// travel between events. The input and step size use the same caller-chosen
/// unit, typically platform-resolved pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotaryStepAccumulator {
    pixels_per_step: f32,
    pending_pixels: f32,
}

impl RotaryStepAccumulator {
    /// Creates an accumulator for the given amount of travel per step.
    pub fn new(pixels_per_step: f32) -> Self {
        assert!(
            pixels_per_step.is_finite() && pixels_per_step > 0.0,
            "rotary step size must be finite and positive"
        );
        Self {
            pixels_per_step,
            pending_pixels: 0.0,
        }
    }

    /// Adds a rotary delta and returns all whole steps crossed by this event.
    pub fn accept(&mut self, pixels: f32) -> i32 {
        if !pixels.is_finite() || pixels == 0.0 {
            return 0;
        }
        self.pending_pixels += pixels;
        let steps = (self.pending_pixels / self.pixels_per_step).trunc() as i32;
        self.pending_pixels -= steps as f32 * self.pixels_per_step;
        steps
    }

    /// Drops any partial step retained from previous events.
    pub fn reset(&mut self) {
        self.pending_pixels = 0.0;
    }
}

/// Converts a raw Android `AXIS_SCROLL` detent value into pixels using
/// Compose's sign convention: positive detents (crown turned up/away) produce a
/// **negative** pixel amount.
pub fn rotary_scroll_pixels_from_detents(detents: f32, scroll_factor: f32) -> f32 {
    -detents * scroll_factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_pixel_amounts_verbatim() {
        let event = RotaryScrollEvent::new(-12.0, 3.5, 4_200);

        assert_eq!(event.vertical_scroll_pixels, -12.0);
        assert_eq!(event.horizontal_scroll_pixels, 3.5);
        assert_eq!(event.uptime_millis, 4_200);
    }

    #[test]
    fn step_accumulator_retains_partial_travel_and_emits_every_crossed_step() {
        let mut steps = RotaryStepAccumulator::new(10.0);
        assert_eq!(steps.accept(6.0), 0);
        assert_eq!(steps.accept(6.0), 1);
        assert_eq!(steps.accept(29.0), 3);
        assert_eq!(steps.accept(-12.0), -1);
    }

    #[test]
    fn step_accumulator_reset_drops_partial_travel() {
        let mut steps = RotaryStepAccumulator::new(10.0);
        assert_eq!(steps.accept(9.0), 0);
        steps.reset();
        assert_eq!(steps.accept(1.0), 0);
    }

    #[test]
    fn positive_detents_become_negative_pixels() {
        assert_eq!(rotary_scroll_pixels_from_detents(1.0, 64.0), -64.0);
        assert_eq!(rotary_scroll_pixels_from_detents(-1.0, 64.0), 64.0);
        assert_eq!(rotary_scroll_pixels_from_detents(0.0, 64.0), 0.0);
    }

    #[test]
    fn from_detents_matches_compose_and_feeds_both_axes() {
        let event = RotaryScrollEvent::from_detents(2.0, 64.0, 48.0, 9);

        assert_eq!(event.vertical_scroll_pixels, -128.0);
        assert_eq!(event.horizontal_scroll_pixels, -96.0);
        assert_eq!(event.uptime_millis, 9);
    }

    #[test]
    fn from_detents_is_sign_symmetric() {
        let up = RotaryScrollEvent::from_detents(1.5, 64.0, 64.0, 0);
        let down = RotaryScrollEvent::from_detents(-1.5, 64.0, 64.0, 0);

        assert_eq!(up.vertical_scroll_pixels, -down.vertical_scroll_pixels);
        assert!(up.vertical_scroll_pixels < 0.0);
        assert!(down.vertical_scroll_pixels > 0.0);
    }

    #[test]
    fn a_wheel_turned_up_lands_where_a_crown_turned_up_does() {
        let wheel = RotaryScrollEvent::from_wheel_pixels(64.0, 0.0, 0);
        let crown = RotaryScrollEvent::from_detents(1.0, 64.0, 64.0, 0);

        assert_eq!(
            wheel.vertical_scroll_pixels, crown.vertical_scroll_pixels,
            "a positive wheel delta and a positive detent are the same physical turn"
        );
        assert!(wheel.vertical_scroll_pixels < 0.0);
    }

    #[test]
    fn a_wheel_carries_each_axis_on_its_own() {
        let event = RotaryScrollEvent::from_wheel_pixels(12.0, -5.0, 77);

        assert_eq!(event.vertical_scroll_pixels, -12.0);
        assert_eq!(event.horizontal_scroll_pixels, 5.0);
        assert_eq!(event.uptime_millis, 77);
        assert!(RotaryScrollEvent::from_wheel_pixels(0.0, 0.0, 1).is_empty());
    }

    #[test]
    fn is_empty_rejects_zero_and_non_finite_amounts() {
        assert!(RotaryScrollEvent::default().is_empty());
        assert!(RotaryScrollEvent::new(0.0, 0.0, 1).is_empty());
        assert!(RotaryScrollEvent::new(f32::NAN, f32::INFINITY, 1).is_empty());

        assert!(!RotaryScrollEvent::new(-1.0, 0.0, 1).is_empty());
        assert!(!RotaryScrollEvent::new(0.0, 2.0, 1).is_empty());
    }

    #[test]
    fn event_is_copy_and_allocation_free() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<RotaryScrollEvent>();
        assert_eq!(
            std::mem::size_of::<RotaryScrollEvent>(),
            std::mem::size_of::<f32>() * 2 + std::mem::size_of::<u64>()
        );
    }
}
