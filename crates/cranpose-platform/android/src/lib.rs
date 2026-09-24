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
#[path = "tests/android_tests.rs"]
mod tests;
