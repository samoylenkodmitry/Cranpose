pub const SHADER: &str = cranpose_ui_graphics::framework_shaders::SHAPE_WGSL;

pub const SOLID_TRIM_APPENDIX: &str =
    cranpose_ui_graphics::framework_shaders::SHAPE_SOLID_TRIM_WGSL;

pub const IMAGE_SHADER: &str = cranpose_ui_graphics::framework_shaders::IMAGE_WGSL;

pub const GLYPH_ATLAS_SHADER: &str = cranpose_ui_graphics::framework_shaders::GLYPH_ATLAS_WGSL;

pub const FULLSCREEN_QUAD_VS: &str =
    cranpose_ui_graphics::framework_shaders::FULLSCREEN_QUAD_VS_WGSL;

pub const SDF_ROUNDED_RECT_FN: &str =
    cranpose_ui_graphics::framework_shaders::SDF_ROUNDED_RECT_FN_WGSL;

pub const COMPOSITE_SAMPLE_FN: &str =
    cranpose_ui_graphics::framework_shaders::COMPOSITE_SAMPLE_FN_WGSL;

pub fn blur_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::BLUR_FS_WGSL
    )
}

pub fn blur_rounded_mask_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::BLUR_ROUNDED_MASK_FS_WGSL
    )
}

pub fn offset_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::OFFSET_FS_WGSL
    )
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
mod tests {
    use super::*;

    #[test]
    fn the_blit_shaders_are_complete_wgsl_with_a_fragment_entry_point() {
        for (name, source) in [
            ("blit", blit_shader()),
            ("projective blit", projective_blit_shader()),
        ] {
            assert!(
                source.contains("@fragment"),
                "the {name} shader has no fragment entry point"
            );
            assert!(
                source.contains("fn "),
                "the {name} shader declares no functions at all"
            );
        }

        assert_ne!(blit_shader(), projective_blit_shader());
    }

    use naga::{ShaderStage, back::glsl};

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

    #[test]
    fn shape_shader_validates_for_webgpu() {
        if let Err(err) = validate_wgsl_module(super::SHADER) {
            panic!("shape.wgsl must validate for WebGPU: {err}");
        }
    }

    #[test]
    fn shape_shader_validates_for_webgl() {
        if let Err(err) = validate_glsl_portability(super::SHADER, "vs_main", ShaderStage::Vertex) {
            panic!("shape.wgsl vertex stage must lower to GLSL ES 300: {err}");
        }
        if let Err(err) = validate_glsl_portability(super::SHADER, "fs_main", ShaderStage::Fragment)
        {
            panic!("shape.wgsl fragment stage must lower to GLSL ES 300: {err}");
        }
    }

    #[test]
    fn trimmed_shape_shader_validates_for_webgpu_and_lowers_to_glsl() {
        let source = format!("{}\n{}", super::SHADER, super::SOLID_TRIM_APPENDIX);
        if let Err(err) = validate_wgsl_module(&source) {
            panic!("shape.wgsl + shape_solid_trim.wgsl must validate for WebGPU: {err}");
        }
        if let Err(err) = validate_glsl_portability(&source, "vs_solid", ShaderStage::Vertex) {
            panic!("trimmed solid vertex stage must lower to GLSL ES 300: {err}");
        }
        if let Err(err) = validate_glsl_portability(&source, "fs_solid_trim", ShaderStage::Fragment)
        {
            panic!("trimmed solid fragment stage must lower to GLSL ES 300: {err}");
        }
    }

    #[test]
    fn shape_shader_declares_the_stroke_and_arc_parameters() {
        for needle in [
            "stroke_params: vec4<f32>",
            "arc_params: vec4<f32>",
            "fn sdf_arc_band(",
            "fn sdf_stroked_rounded_rect(",
        ] {
            assert!(
                super::SHADER.contains(needle),
                "shape.wgsl must declare `{needle}`"
            );
        }
    }

    #[test]
    fn resized_shape_shader_still_validates_for_both_targets() {
        let resized = super::SHADER
            .replace("array<ShapeData, 102>", "array<ShapeData, 409>")
            .replace("array<GradientStop, 256>", "array<GradientStop, 1024>");
        assert!(
            resized.contains("array<ShapeData, 409>"),
            "array length literal drifted from `shape_shader_source`"
        );
        assert!(validate_wgsl_module(&resized).is_ok());
        assert!(validate_glsl_portability(&resized, "fs_main", ShaderStage::Fragment).is_ok());
    }
}
