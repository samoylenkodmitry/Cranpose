/// CPU-readable RGBA frame captured from the renderer output.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Width of the frame in device pixels.
    pub width: u32,
    /// Height of the frame in device pixels.
    pub height: u32,
    /// The pixels as 8-bit RGBA, row by row from the top left, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    /// The renderer's account of the frame it drew for the capture, such as
    /// the placeholders it drew for pipelines that still compile.
    pub stats: Option<crate::RenderStatsSnapshot>,
}
