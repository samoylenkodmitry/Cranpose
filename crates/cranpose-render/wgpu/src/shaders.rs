//! WGSL shaders for 2D rendering with GPU acceleration.

pub const SHADER: &str = cranpose_ui_graphics::framework_shaders::SHAPE_WGSL;

pub const IMAGE_SHADER: &str = cranpose_ui_graphics::framework_shaders::IMAGE_WGSL;

pub const GLYPH_ATLAS_SHADER: &str = cranpose_ui_graphics::framework_shaders::GLYPH_ATLAS_WGSL;

// ═══════════════════════════════════════════════════════════════════════════
// Shared WGSL snippets for post-process effects
// ═══════════════════════════════════════════════════════════════════════════

/// Fullscreen quad vertex shader preamble shared by all post-process effects.
///
/// Declares `VertexOutput` and `fullscreen_vs` — a vertex shader that generates
/// a full-screen triangle pair from vertex ID (no vertex buffer needed).
/// Output UV covers [0,1]×[0,1].
pub const FULLSCREEN_QUAD_VS: &str =
    cranpose_ui_graphics::framework_shaders::FULLSCREEN_QUAD_VS_WGSL;

/// SDF rounded-rectangle function shared by the main shape shader and blit shader.
pub const SDF_ROUNDED_RECT_FN: &str =
    cranpose_ui_graphics::framework_shaders::SDF_ROUNDED_RECT_FN_WGSL;

pub const COMPOSITE_SAMPLE_FN: &str =
    cranpose_ui_graphics::framework_shaders::COMPOSITE_SAMPLE_FN_WGSL;

// ═══════════════════════════════════════════════════════════════════════════
// Composed post-process shaders
// ═══════════════════════════════════════════════════════════════════════════

/// Two-pass separable Gaussian blur post-process shader.
///
/// Uniforms (via push-style uniform buffer):
/// - direction: vec2<f32> — (1,0) for horizontal, (0,1) for vertical
/// - radius: vec2<f32> — blur radius in pixels (x,y)
/// - texture_size: vec2<f32> — input texture dimensions in pixels
/// - tile_mode: f32 — 0.0 = Clamp, 1.0 = Repeated, 2.0 = Mirror, 3.0 = Decal
pub fn blur_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::BLUR_FS_WGSL
    )
}

/// Fused vertical blur plus rounded-alpha-mask shader.
///
/// The horizontal blur has already been written to the input texture. This
/// pass performs the vertical blur and applies the same rounded mask semantics
/// as `rounded_alpha_mask_effect`, then composites directly into the caller's
/// destination render target.
pub fn blur_rounded_mask_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::BLUR_ROUNDED_MASK_FS_WGSL
    )
}

/// Offset post-process shader.
///
/// Translates the source texture by the provided pixel offset. Pixels shifted
/// outside the source texture become transparent.
pub fn offset_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::OFFSET_FS_WGSL
    )
}

/// Simple fullscreen blit shader for compositing offscreen targets to the surface.
///
/// Renders the entire offscreen texture as a fullscreen quad with premultiplied alpha blending.
/// Transparent regions contribute nothing, so only the effect-processed content
/// is composited onto the existing surface.
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
mod tests {
    use naga::back::glsl;
    use naga::ShaderStage;

    fn validate_wgsl_module(source: &str) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(source)
            .map_err(|err| format!("WGSL parse error: {err}"))?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .map_err(|err| format!("WGSL validation error: {err}"))?;
        Ok(())
    }

    fn validate_glsl_portability(
        source: &str,
        entry_point: &str,
        shader_stage: ShaderStage,
    ) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(source)
            .map_err(|err| format!("WGSL parse error: {err}"))?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        let module_info = validator
            .validate(&module)
            .map_err(|err| format!("WGSL validation error: {err}"))?;
        let mut glsl_source = String::new();
        let options = glsl::Options {
            version: glsl::Version::new_gles(300),
            writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
            ..Default::default()
        };
        let pipeline_options = glsl::PipelineOptions {
            shader_stage,
            entry_point: entry_point.to_string(),
            multiview: None,
        };
        let mut writer = glsl::Writer::new(
            &mut glsl_source,
            &module,
            &module_info,
            &options,
            &pipeline_options,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .map_err(|err| format!("GL/WebGL portability validation failed: {err}"))?;
        writer
            .write()
            .map(|_| ())
            .map_err(|err| format!("GL/WebGL portability emission failed: {err}"))
    }

    #[test]
    fn blur_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::blur_shader()).is_ok());
    }

    #[test]
    fn blur_shader_validates_for_webgl() {
        let shader = super::blur_shader();
        assert!(validate_glsl_portability(&shader, "fullscreen_vs", ShaderStage::Vertex).is_ok());
        assert!(validate_glsl_portability(&shader, "blur_fs", ShaderStage::Fragment).is_ok());
    }

    #[test]
    fn blur_rounded_mask_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::blur_rounded_mask_shader()).is_ok());
    }

    #[test]
    fn blur_rounded_mask_shader_validates_for_webgl() {
        let shader = super::blur_rounded_mask_shader();
        assert!(validate_glsl_portability(&shader, "fullscreen_vs", ShaderStage::Vertex).is_ok());
        assert!(
            validate_glsl_portability(&shader, "blur_rounded_mask_fs", ShaderStage::Fragment)
                .is_ok()
        );
    }

    #[test]
    fn offset_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::offset_shader()).is_ok());
    }
}
