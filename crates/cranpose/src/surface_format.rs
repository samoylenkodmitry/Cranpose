//! Display surface format policy shared by every platform shell.
//!
//! The framework's color pipeline is sRGB-native: `Color` values, gradient
//! stops, text colors and runtime-shader uniforms are all authored and
//! blended in sRGB space, and the fragment shaders write them through
//! unconverted. The swapchain must therefore be a NON-sRGB view — an
//! `*UnormSrgb` surface would treat those values as linear and gamma-encode
//! them a second time, washing out every color on screen (a solid
//! `(52, 199, 89)` fill would display as `(125, 229, 160)`).

/// Picks the swapchain format for a display surface: the first non-sRGB
/// 8-bit color format, falling back to the first reported format.
pub(crate) fn select_display_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .or_else(|| formats.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_non_srgb_over_srgb() {
        let formats = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        assert_eq!(
            select_display_surface_format(&formats),
            Some(wgpu::TextureFormat::Bgra8Unorm),
            "sRGB views double-encode the framework's sRGB-space colors"
        );
    }

    #[test]
    fn falls_back_to_first_format_when_only_srgb_is_offered() {
        let formats = [wgpu::TextureFormat::Bgra8UnormSrgb];
        assert_eq!(
            select_display_surface_format(&formats),
            Some(wgpu::TextureFormat::Bgra8UnormSrgb)
        );
    }

    #[test]
    fn empty_capabilities_yield_none() {
        assert_eq!(select_display_surface_format(&[]), None);
    }
}
