use compose_ui_graphics::Point;

/// Platform abstraction for Android.
///
/// This type manages platform-specific conversions (e.g., density, pointer coordinates)
/// and provides a bridge between Android's event system and Compose's logical coordinate space.
#[derive(Debug, Clone)]
pub struct AndroidPlatform {
    scale_factor: f64,
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self { scale_factor: 1.0 }
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

    /// Converts a physical pointer position into logical coordinates.
    ///
    /// Android provides pointer positions in physical pixels; this method
    /// scales them back to logical pixels using the platform's current scale factor.
    pub fn pointer_position(&self, physical_x: f64, physical_y: f64) -> Point {
        let scale = self.scale_factor;
        Point {
            x: (physical_x / scale) as f32,
            y: (physical_y / scale) as f32,
        }
    }

    /// Returns the current scale factor (density).
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor as f32
    }
}
