#![deny(unsafe_code)]

use cranpose_ui_graphics::Point;

/// Platform abstraction for Android.
///
/// This type manages platform-specific conversions (e.g., density, pointer coordinates)
/// and provides a bridge between Android's event system and Compose's logical coordinate space.
#[derive(Debug, Clone)]
pub struct AndroidPlatform {
    scale_factor: f64,
    input_surface_offset_x_px: f64,
    input_surface_offset_y_px: f64,
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            input_surface_offset_x_px: 0.0,
            input_surface_offset_y_px: 0.0,
        }
    }
}

impl AndroidPlatform {
    /// Creates a new Android platform with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the platform's scale factor.
    ///
    /// This should be called when the device density changes.
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
    }

    /// Updates the native-surface offset for Android input coordinates.
    ///
    /// Android `MotionEvent` positions are relative to the activity content
    /// area, while the renderer draws into the full `ANativeWindow`. Freeform
    /// and desktop window modes can inflate that native surface around the
    /// content area for shadows/decor, so input must be shifted into native
    /// surface coordinates before density conversion.
    pub fn set_input_surface_offset_px(&mut self, x_px: f64, y_px: f64) {
        self.input_surface_offset_x_px = x_px;
        self.input_surface_offset_y_px = y_px;
    }

    /// Returns the configured native-surface input offset in physical pixels.
    pub fn input_surface_offset_px(&self) -> (f64, f64) {
        (
            self.input_surface_offset_x_px,
            self.input_surface_offset_y_px,
        )
    }

    /// Converts a physical Android pointer position into logical coordinates.
    ///
    /// Android provides pointer positions in content-relative physical pixels;
    /// this method first shifts them into the native surface coordinate space,
    /// then scales them back to logical pixels using the platform's current
    /// scale factor.
    pub fn pointer_position(&self, physical_x: f64, physical_y: f64) -> Point {
        let scale = self.scale_factor;
        Point {
            x: ((physical_x + self.input_surface_offset_x_px) / scale) as f32,
            y: ((physical_y + self.input_surface_offset_y_px) / scale) as f32,
        }
    }

    /// Returns the current scale factor (density).
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor as f32
    }
}

#[cfg(test)]
mod tests {
    use super::AndroidPlatform;

    #[test]
    fn default_platform_uses_mdpi_and_zero_input_offset() {
        let platform = AndroidPlatform::new();

        assert_eq!(platform.scale_factor(), 1.0);
        assert_eq!(platform.input_surface_offset_px(), (0.0, 0.0));
        assert_eq!(platform.pointer_position(12.0, 34.0).x, 12.0);
        assert_eq!(platform.pointer_position(12.0, 34.0).y, 34.0);
    }

    #[test]
    fn pointer_position_applies_density() {
        let mut platform = AndroidPlatform::new();
        platform.set_scale_factor(2.0);

        let logical = platform.pointer_position(80.0, 120.0);

        assert_eq!(platform.scale_factor(), 2.0);
        assert_eq!(logical.x, 40.0);
        assert_eq!(logical.y, 60.0);
    }

    #[test]
    fn pointer_position_applies_surface_offset_before_density() {
        let mut platform = AndroidPlatform::new();
        platform.set_scale_factor(1.5);
        platform.set_input_surface_offset_px(39.0, 21.0);

        let logical = platform.pointer_position(111.0, 129.0);

        assert_eq!(platform.input_surface_offset_px(), (39.0, 21.0));
        assert_eq!(logical.x, 100.0);
        assert_eq!(logical.y, 100.0);
    }
}
