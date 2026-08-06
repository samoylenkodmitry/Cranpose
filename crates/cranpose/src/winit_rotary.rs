//! Desktop mouse-wheel to rotary-input translation.
//!
//! The final deployment target for rotary input is a Wear OS watch, but the
//! development platform is a desktop. Mapping winit's `MouseWheel` onto
//! [`RotaryScrollEvent`] lets the whole rotary stack be built and eyeballed
//! without a watch, and keeps a single code path under test on the host.
//!
//! # Sign convention
//!
//! winit reports a **positive** vertical wheel delta when the wheel is scrolled
//! *up / away from the user* — the same physical direction that produces a
//! **positive** Android `AXIS_SCROLL` detent for a crown. Compose negates the
//! Android value on its way into `RotaryScrollEvent` (see
//! [`cranpose_app_shell::RotaryScrollEvent`]), so this translation negates the
//! winit delta too. A wheel scrolled up and a crown turned up therefore produce
//! the same **negative** `vertical_scroll_pixels`.

use cranpose_app_shell::RotaryScrollEvent;
use cranpose_ui::Point;

/// Converts an already-logical wheel delta into a [`RotaryScrollEvent`].
///
/// `logical_delta` is the output of
/// `DesktopWinitPlatform::scroll_delta`, which normalizes both
/// `MouseScrollDelta::LineDelta` and `MouseScrollDelta::PixelDelta` into
/// logical pixels while preserving winit's sign. `uptime_millis` is a
/// monotonic timestamp; only differences between events are meaningful.
pub(crate) fn rotary_event_from_wheel_delta(
    logical_delta: Point,
    uptime_millis: u64,
) -> RotaryScrollEvent {
    RotaryScrollEvent::new(-logical_delta.y, -logical_delta.x, uptime_millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_platform_desktop_winit::DesktopWinitPlatform;
    use winit::dpi::PhysicalPosition;
    use winit::event::MouseScrollDelta;

    /// One wheel notch as normalized by `DesktopWinitPlatform`.
    const NOTCH_PIXELS: f32 = 40.0;

    fn rotary_for(delta: MouseScrollDelta, scale_factor: f64) -> RotaryScrollEvent {
        let platform = DesktopWinitPlatform::new(scale_factor);
        rotary_event_from_wheel_delta(platform.scroll_delta(delta), 0)
    }

    #[test]
    fn line_delta_scroll_up_produces_negative_vertical_pixels() {
        // Wheel up == crown turned away == negative vertical_scroll_pixels,
        // matching Compose's `val axisValue = -event.getAxisValue(AXIS_SCROLL)`.
        let event = rotary_for(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);

        assert_eq!(event.vertical_scroll_pixels, -NOTCH_PIXELS);
        assert_eq!(event.horizontal_scroll_pixels, 0.0);
    }

    #[test]
    fn line_delta_scroll_down_produces_positive_vertical_pixels() {
        let event = rotary_for(MouseScrollDelta::LineDelta(0.0, -1.0), 1.0);

        assert_eq!(event.vertical_scroll_pixels, NOTCH_PIXELS);
    }

    #[test]
    fn line_delta_maps_horizontal_axis_too() {
        let event = rotary_for(MouseScrollDelta::LineDelta(2.0, 0.0), 1.0);

        assert_eq!(event.horizontal_scroll_pixels, -2.0 * NOTCH_PIXELS);
        assert_eq!(event.vertical_scroll_pixels, 0.0);
    }

    #[test]
    fn pixel_delta_is_converted_and_negated() {
        let event = rotary_for(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 0.0, y: 12.0 }),
            1.0,
        );

        assert_eq!(event.vertical_scroll_pixels, -12.0);
    }

    #[test]
    fn pixel_delta_respects_hidpi_scale_factor() {
        // 24 physical px at 2x == 12 logical px, then negated.
        let event = rotary_for(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 8.0, y: 24.0 }),
            2.0,
        );

        assert_eq!(event.vertical_scroll_pixels, -12.0);
        assert_eq!(event.horizontal_scroll_pixels, -4.0);
    }

    #[test]
    fn both_delta_variants_agree_on_direction() {
        // A scroll "up" must have the same sign whichever variant the platform
        // reports, or trackpads and wheels would scroll opposite ways.
        let line = rotary_for(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        let pixel = rotary_for(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 0.0, y: 40.0 }),
            1.0,
        );

        assert_eq!(line.vertical_scroll_pixels, pixel.vertical_scroll_pixels);
        assert!(line.vertical_scroll_pixels < 0.0);
    }

    #[test]
    fn desktop_sign_matches_the_android_detent_path() {
        // The two ingresses must agree: a crown turned "up" reports a positive
        // detent, a wheel scrolled "up" reports a positive winit delta, and
        // both must land on a negative vertical_scroll_pixels.
        let desktop = rotary_for(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        let android = RotaryScrollEvent::from_detents(1.0, NOTCH_PIXELS, NOTCH_PIXELS, 0);

        assert_eq!(
            desktop.vertical_scroll_pixels,
            android.vertical_scroll_pixels
        );
    }

    #[test]
    fn zero_delta_produces_an_empty_event() {
        let event = rotary_for(MouseScrollDelta::LineDelta(0.0, 0.0), 1.0);

        assert!(event.is_empty());
    }

    #[test]
    fn uptime_is_carried_through() {
        let event = rotary_event_from_wheel_delta(Point { x: 0.0, y: 1.0 }, 9_876);

        assert_eq!(event.uptime_millis, 9_876);
    }
}
