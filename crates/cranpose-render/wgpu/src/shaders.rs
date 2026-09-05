pub const SHADER: &str = cranpose_ui_graphics::framework_shaders::SHAPE_WGSL;

pub const IMAGE_SHADER: &str = cranpose_ui_graphics::framework_shaders::IMAGE_WGSL;

pub const GLYPH_ATLAS_SHADER: &str = cranpose_ui_graphics::framework_shaders::GLYPH_ATLAS_WGSL;

pub const FULLSCREEN_QUAD_VS: &str =
    cranpose_ui_graphics::framework_shaders::FULLSCREEN_QUAD_VS_WGSL;

pub const SDF_ROUNDED_RECT_FN: &str =
    cranpose_ui_graphics::framework_shaders::SDF_ROUNDED_RECT_FN_WGSL;

pub const COMPOSITE_SAMPLE_FN: &str =
    cranpose_ui_graphics::framework_shaders::COMPOSITE_SAMPLE_FN_WGSL;

/// The four run-table declarations of `shape.wgsl` in their uniform form,
/// each paired with the storage form the native pipelines rewrite it to.
pub(crate) const RUN_TABLE_DECLARATIONS: [(&str, &str); 4] = [
    (
        "var<uniform> records: array<ShapeRecord, 128>;",
        "var<storage, read> records: array<ShapeRecord>;",
    ),
    (
        "var<uniform> brushes: array<BrushRecord, 256>;",
        "var<storage, read> brushes: array<BrushRecord>;",
    ),
    (
        "var<uniform> gradient_stops: array<GradientStop, 256>;",
        "var<storage, read> gradient_stops: array<GradientStop>;",
    ),
    (
        "var<uniform> placements: array<Placement, 64>;",
        "var<storage, read> placements: array<Placement>;",
    ),
];

/// `shape.wgsl` with its run tables as unbounded storage arrays.
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

pub fn blur_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        cranpose_ui_graphics::framework_shaders::BLUR_FS_WGSL
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
        let (module, module_info) = naga::back::pipeline_constants::process_overrides(
            &module,
            &module_info,
            Some((shader_stage, entry_point)),
            &naga::back::PipelineConstants::default(),
        )
        .map_err(|err| format!("override resolution failed: {err}"))?;
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
        assert!(
            validate_glsl_portability(&shader, "blur_downsample_fs", ShaderStage::Fragment).is_ok()
        );
    }

    #[test]
    fn offset_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::offset_shader()).is_ok());
    }

    #[test]
    fn shape_shader_validates_for_webgpu_in_both_table_forms() {
        if let Err(err) = validate_wgsl_module(super::SHADER) {
            panic!("shape.wgsl must validate for WebGPU: {err}");
        }
        if let Err(err) = validate_wgsl_module(&storage_shape_shader()) {
            panic!("shape.wgsl with storage tables must validate for WebGPU: {err}");
        }
    }

    #[test]
    fn shape_shader_validates_for_webgl() {
        for entry in ["vs_record", "vs_record_solid"] {
            if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Vertex) {
                panic!("shape.wgsl `{entry}` must lower to GLSL ES 300: {err}");
            }
        }
        for entry in ["fs_main", "fs_solid"] {
            if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Fragment)
            {
                panic!("shape.wgsl `{entry}` must lower to GLSL ES 300: {err}");
            }
        }
    }

    const GLES_VARYING_VECTOR_FLOOR: u32 = 15;

    fn fragment_input_locations(source: &str, entry_point: &str) -> Vec<u32> {
        let module = naga::front::wgsl::parse_str(source).expect("shader must parse");
        let entry = module
            .entry_points
            .iter()
            .find(|entry| entry.name == entry_point)
            .unwrap_or_else(|| panic!("{entry_point} missing"));
        let mut locations = Vec::new();
        for argument in &entry.function.arguments {
            match &module.types[argument.ty].inner {
                naga::TypeInner::Struct { members, .. } => {
                    for member in members {
                        if let Some(naga::Binding::Location { location, .. }) = member.binding {
                            locations.push(location);
                        }
                    }
                }
                _ => {
                    if let Some(naga::Binding::Location { location, .. }) = argument.binding {
                        locations.push(location);
                    }
                }
            }
        }
        locations.sort_unstable();
        locations
    }

    #[test]
    fn shape_fragment_inputs_fit_the_gles_varying_floor() {
        for entry_point in ["fs_main", "fs_solid"] {
            let locations = fragment_input_locations(super::SHADER, entry_point);
            let highest = locations.last().copied().expect("fragment inputs");
            assert!(
                highest < GLES_VARYING_VECTOR_FLOOR,
                "{entry_point} reads location {highest}; GLSL ES 3.0 only guarantees \
                 {GLES_VARYING_VECTOR_FLOOR} varying vectors, so every location must \
                 stay below it: {locations:?}"
            );
            let mut deduplicated = locations.clone();
            deduplicated.dedup();
            assert_eq!(deduplicated, locations, "{entry_point} reuses a location");
        }
        assert_eq!(
            fragment_input_locations(super::SHADER, "fs_solid").len(),
            8,
            "a solid batch carries the coverage vectors and nothing of the brush"
        );
    }

    #[test]
    fn shape_shader_declares_the_record_layout_the_recorder_writes() {
        for needle in [
            "struct ShapeRecord {",
            "struct BrushRecord {",
            "struct GradientStop {",
            "struct Placement {",
            "fn sdf_arc_band(",
            "fn sdf_stroked_rounded_rect(",
            "fn vs_record(",
            "fn vs_record_solid(",
            "fn band_position(",
            "fn fs_solid(",
            "override TIER_ARENA: bool",
            "override BAND_SEGMENTS: u32",
        ] {
            assert!(
                super::SHADER.contains(needle),
                "shape.wgsl must declare `{needle}`"
            );
        }
    }

    #[test]
    fn the_uniform_chunk_sizes_in_the_shader_match_the_run_store() {
        use crate::run_store::{BRUSH_CHUNK, PLACEMENT_CHUNK, RECORD_CHUNK, STOP_CHUNK};
        for (uniform, _) in RUN_TABLE_DECLARATIONS {
            assert!(super::SHADER.contains(uniform), "missing `{uniform}`");
        }
        assert!(super::SHADER.contains(&format!("array<ShapeRecord, {RECORD_CHUNK}>")));
        assert!(super::SHADER.contains(&format!("array<BrushRecord, {BRUSH_CHUNK}>")));
        assert!(super::SHADER.contains(&format!("array<GradientStop, {STOP_CHUNK}>")));
        assert!(super::SHADER.contains(&format!("array<Placement, {PLACEMENT_CHUNK}>")));
    }
}
