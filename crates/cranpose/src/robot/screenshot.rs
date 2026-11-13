// The pixels of a frame a robot captures.

/// RGBA screenshot captured from the current render scene.
#[derive(Debug, Clone)]
pub struct RobotScreenshot {
    /// Screenshot width in pixels.
    pub width: u32,
    /// Screenshot height in pixels.
    pub height: u32,
    /// Logical width covered by the screenshot.
    pub logical_width: f32,
    /// Logical height covered by the screenshot.
    pub logical_height: f32,
    /// Packed RGBA8 pixel buffer in row-major order.
    pub pixels: Vec<u8>,
}
