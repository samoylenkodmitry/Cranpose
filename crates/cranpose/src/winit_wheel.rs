use cranpose_app_shell::{Modifiers, WheelScroll};
use cranpose_ui::Point;
use winit::keyboard::ModifiersState;

pub(crate) fn wheel_scroll_from_winit(
    logical_delta: Point,
    modifiers: ModifiersState,
    uptime_millis: u64,
) -> WheelScroll {
    WheelScroll::new(logical_delta, uptime_millis).with_modifiers(Modifiers {
        shift: modifiers.contains(ModifiersState::SHIFT),
        ctrl: modifiers.contains(ModifiersState::CONTROL),
        alt: modifiers.contains(ModifiersState::ALT),
        meta: modifiers.contains(ModifiersState::META),
    })
}

#[cfg(test)]
mod tests {
    use cranpose_app_shell::RotaryScrollEvent;
    use cranpose_platform_desktop_winit::DesktopWinitPlatform;
    use winit::{dpi::PhysicalPosition, event::MouseScrollDelta};

    use super::*;

    const NOTCH_PIXELS: f32 = 40.0;

    fn wheel_for(delta: MouseScrollDelta, scale_factor: f64) -> WheelScroll {
        let platform = DesktopWinitPlatform::new(scale_factor);
        wheel_scroll_from_winit(platform.scroll_delta(delta), ModifiersState::empty(), 0)
    }

    fn rotary_for(delta: MouseScrollDelta, scale_factor: f64) -> RotaryScrollEvent {
        let wheel = wheel_for(delta, scale_factor);
        RotaryScrollEvent::from_wheel_pixels(wheel.delta.y, wheel.delta.x, wheel.uptime_millis)
    }

    #[test]
    fn line_delta_scroll_up_produces_negative_vertical_pixels() {
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
        let event = rotary_for(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 8.0, y: 24.0 }),
            2.0,
        );

        assert_eq!(event.vertical_scroll_pixels, -12.0);
        assert_eq!(event.horizontal_scroll_pixels, -4.0);
    }

    #[test]
    fn both_delta_variants_agree_on_direction() {
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
        let desktop = rotary_for(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        let android = RotaryScrollEvent::from_detents(1.0, NOTCH_PIXELS, NOTCH_PIXELS, 0);

        assert_eq!(
            desktop.vertical_scroll_pixels,
            android.vertical_scroll_pixels
        );
    }

    #[test]
    fn a_wheel_scrolled_up_asks_the_content_to_move_down() {
        let up = wheel_for(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);

        assert_eq!(up.delta.y, NOTCH_PIXELS);
        assert!(!up.is_zoom());
        assert_eq!(up.scroll_delta(), up.delta);
    }

    #[test]
    fn zero_delta_produces_an_empty_event() {
        let event = rotary_for(MouseScrollDelta::LineDelta(0.0, 0.0), 1.0);

        assert!(event.is_empty());
    }

    #[test]
    fn uptime_is_carried_through() {
        let wheel =
            wheel_scroll_from_winit(Point { x: 0.0, y: 1.0 }, ModifiersState::empty(), 9876);

        assert_eq!(wheel.uptime_millis, 9_876);
    }

    #[test]
    fn winit_modifiers_reach_the_shell_policy() {
        let ctrl = wheel_scroll_from_winit(Point { x: 0.0, y: 40.0 }, ModifiersState::CONTROL, 0);
        let alt = wheel_scroll_from_winit(Point { x: 0.0, y: 40.0 }, ModifiersState::ALT, 0);

        assert!(ctrl.is_zoom());
        assert!(!alt.is_zoom());
        assert_eq!(alt.scroll_delta(), Point { x: 40.0, y: 0.0 });
    }
}
