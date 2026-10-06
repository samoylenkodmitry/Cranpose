use cranpose_render_common::geometry::BLUR_TAP_PAIRS;

pub const SHADER: &str = cranpose_ui_graphics::framework_shaders::SHAPE_WGSL;

pub const IMAGE_SHADER: &str = cranpose_ui_graphics::framework_shaders::IMAGE_WGSL;

pub const GLYPH_ATLAS_SHADER: &str = cranpose_ui_graphics::framework_shaders::GLYPH_ATLAS_WGSL;

pub const FULLSCREEN_QUAD_VS: &str =
    cranpose_ui_graphics::framework_shaders::FULLSCREEN_QUAD_VS_WGSL;

pub const SDF_ROUNDED_RECT_FN: &str =
    cranpose_ui_graphics::framework_shaders::SDF_ROUNDED_RECT_FN_WGSL;

pub const COMPOSITE_SAMPLE_FN: &str =
    cranpose_ui_graphics::framework_shaders::COMPOSITE_SAMPLE_FN_WGSL;

pub(crate) const RUN_TABLE_DECLARATIONS: [(&str, &str); 3] = [
    (
        "var<uniform> brushes: array<BrushRecord, 256>;",
        "var<storage, read> brushes: array<BrushRecord>;",
    ),
    (
        "var<uniform> gradient_stops: array<GradientStop, 256>;",
        "var<storage, read> gradient_stops: array<GradientStop>;",
    ),
    (
        "var<uniform> placements: array<Placement, 4>;",
        "var<storage, read> placements: array<Placement>;",
    ),
];

pub(crate) fn storage_shape_shader() -> String {
    let mut source = SHADER.to_string();
    for (uniform, storage) in RUN_TABLE_DECLARATIONS {
        assert!(
            source.contains(uniform),
            "shape.wgsl must declare `{uniform}`"
        );
        source = source.replace(uniform, storage);
    }
    source
}

pub(crate) fn filling_shape_shader() -> String {
    let mut source = storage_shape_shader();
    source.push_str(cranpose_ui_graphics::framework_shaders::ARC_TRIG_FILL_WGSL);
    source
}

/// The line of `blur_fs.wgsl` that the kernel's pair taps replace: a `//@`
/// directive, which the embedded text keeps without its indentation.
const BLUR_KERNEL_PAIRS_MARKER: &str = "//@BLUR_KERNEL_PAIRS\n";

/// The blur shader, its kernel's pairs written out one guarded call per
/// entry of the uniform pair table: a loop over the table indexes it
/// dynamically, which Mali's compilers neither unroll nor keep in uniform
/// registers, so each pair cost memory loads besides its fetch.
pub fn blur_shader() -> String {
    use std::fmt::Write;
    let source = cranpose_ui_graphics::framework_shaders::BLUR_FS_WGSL;
    assert!(
        source.contains(BLUR_KERNEL_PAIRS_MARKER),
        "blur_fs.wgsl must mark where the kernel's pairs go"
    );
    let mut pairs = String::new();
    for pair in 0..BLUR_TAP_PAIRS {
        let _ = writeln!(
            pairs,
            "    if (pair_count > {pair}) {{ color = color + kernel_pair(frame, axis, blur.pairs[{pair}], {}.0); }}",
            2 * pair + 1
        );
    }
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        source.replace(BLUR_KERNEL_PAIRS_MARKER, &pairs)
    )
}

pub fn offset_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::OFFSET_FS_WGSL
    )
}

/// The image shader, with the rounded-rect distance its rounded clip takes.
pub fn image_shader() -> String {
    format!("{SDF_ROUNDED_RECT_FN}{IMAGE_SHADER}")
}

pub fn blit_shader() -> String {
    let mut shader = format!(
        "{FULLSCREEN_QUAD_VS}{SDF_ROUNDED_RECT_FN}{}",
        cranpose_ui_graphics::framework_shaders::BLIT_FS_WGSL
    );
    shader.push_str(COMPOSITE_SAMPLE_FN);
    shader.push_str(cranpose_ui_graphics::framework_shaders::BLIT_FS_MAIN_WGSL);
    shader
}

pub fn projective_blit_shader() -> String {
    let mut shader = cranpose_ui_graphics::framework_shaders::PROJECTIVE_BLIT_FS_WGSL.to_string();
    shader.push_str(COMPOSITE_SAMPLE_FN);
    shader.push_str(cranpose_ui_graphics::framework_shaders::PROJECTIVE_BLIT_MAIN_WGSL);
    shader
}

#[cfg(test)]
#[path = "tests/shaders_tests.rs"]
mod tests;
