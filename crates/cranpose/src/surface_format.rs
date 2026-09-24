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
#[path = "tests/surface_format_tests.rs"]
mod tests;
