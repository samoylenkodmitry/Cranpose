pub(crate) fn select_display_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .or_else(|| formats.first().copied())
}

pub(crate) fn display_surface_view_format(
    surface_format: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    surface_format.remove_srgb_suffix()
}

pub(crate) fn display_surface_view_formats(
    surface_format: wgpu::TextureFormat,
) -> Vec<wgpu::TextureFormat> {
    let view_format = display_surface_view_format(surface_format);
    (view_format != surface_format)
        .then_some(view_format)
        .into_iter()
        .collect()
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

    #[test]
    fn srgb_surface_uses_linear_view_for_native_srgb_values() {
        assert_eq!(
            display_surface_view_format(wgpu::TextureFormat::Bgra8UnormSrgb),
            wgpu::TextureFormat::Bgra8Unorm
        );
        assert_eq!(
            display_surface_view_formats(wgpu::TextureFormat::Bgra8UnormSrgb),
            vec![wgpu::TextureFormat::Bgra8Unorm]
        );
        assert!(display_surface_view_formats(wgpu::TextureFormat::Bgra8Unorm).is_empty());
    }
}
