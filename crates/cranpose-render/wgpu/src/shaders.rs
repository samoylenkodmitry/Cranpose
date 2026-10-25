use std::borrow::Cow;

use cranpose_ui_graphics::framework_shaders as wgsl;

use crate::shared_shader::ShaderCode;

pub(crate) const SHADER: &str = wgsl::SHAPE_WGSL;

/// A built-in module's code, with its GLSL templates where the build script
/// wrote them.
macro_rules! built_in {
    ($($name:ident = $wgsl:ident;)+) => {$(
        #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
        pub(crate) const $name: ShaderCode =
            ShaderCode::built_in(|| Cow::Borrowed(wgsl::$wgsl), crate::gl_templates::$name);
        #[cfg(not(all(target_arch = "wasm32", feature = "gl-templates")))]
        pub(crate) const $name: ShaderCode = ShaderCode::wgsl(|| Cow::Borrowed(wgsl::$wgsl));
    )+};
}

built_in! {
    SHAPE = SHAPE_WGSL;
    IMAGE = IMAGE_WGSL;
    GLYPH_ATLAS = GLYPH_ATLAS_WGSL;
    RRECT_SHADOW = RRECT_SHADOW_WGSL;
    BLUR = BLUR_WGSL;
    OFFSET = OFFSET_WGSL;
    BLIT = BLIT_WGSL;
    PROJECTIVE_BLIT = PROJECTIVE_BLIT_WGSL;
    OUTPUT_CONVERSION = OUTPUT_CONVERSION_WGSL;
    MESH_SURFACE = MESH_SURFACE_WGSL;
}

pub(crate) const RUN_TABLE_DECLARATIONS: [(&str, &str); 3] = [
    (
        "var<uniform> brush_words: array<vec4<u32>, 768>;",
        "var<storage, read> brush_words: array<vec4<u32>>;",
    ),
    (
        "var<uniform> gradient_stop_words: array<vec4<f32>, 512>;",
        "var<storage, read> gradient_stop_words: array<vec4<f32>>;",
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

#[cfg(test)]
#[path = "tests/shaders_tests.rs"]
mod tests;
