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
    let module =
        naga::front::wgsl::parse_str(source).map_err(|err| format!("WGSL parse error: {err}"))?;
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
    validate_glsl_portability_with_constants(
        source,
        entry_point,
        shader_stage,
        &naga::back::PipelineConstants::default(),
    )
}

fn validate_glsl_portability_with_constants(
    source: &str,
    entry_point: &str,
    shader_stage: ShaderStage,
    constants: &naga::back::PipelineConstants,
) -> Result<(), String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|err| format!("WGSL parse error: {err}"))?;
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
        constants,
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
    for tile_mode in 0..4 {
        for block in [2, 4] {
            let constants = naga::back::PipelineConstants::from_iter([
                ("BLUR_TILE_MODE".into(), tile_mode as f64),
                ("BLUR_BLOCK".into(), block as f64),
            ]);
            for entry in ["blur_fs", "blur_downsample_fs", "blur_mean_fs"] {
                validate_glsl_portability_with_constants(
                    &shader,
                    entry,
                    ShaderStage::Fragment,
                    &constants,
                )
                .unwrap_or_else(|error| {
                    panic!("mode={tile_mode}, block={block}, {entry}: {error}")
                });
            }
        }
    }
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
fn shape_vertices_receive_every_record_field_from_its_column() {
    let module = naga::front::wgsl::parse_str(&storage_shape_shader()).unwrap();
    for entry_name in ["vs_record", "vs_record_solid", "vs_record_gradient_fill"] {
        let entry = module
            .entry_points
            .iter()
            .find(|entry| entry.name == entry_name)
            .unwrap();
        let argument = entry
            .function
            .arguments
            .iter()
            .find(|argument| argument.name.as_deref() == Some("record"))
            .expect("native vertices must receive the record through instance attributes");
        let naga::TypeInner::Struct { members, span } = &module.types[argument.ty].inner else {
            panic!("the instance input must be a shape record");
        };
        let layouts = crate::record_columns::record_vertex_layouts();
        assert_eq!(members.len(), 9);
        assert_eq!(
            layouts
                .iter()
                .map(|layout| layout.array_stride)
                .sum::<u64>(),
            u64::from(*span)
        );
        assert!(
            layouts
                .iter()
                .all(|layout| layout.step_mode == wgpu::VertexStepMode::Instance)
        );
        for (index, member) in members.iter().enumerate() {
            assert!(
                matches!(member.binding, Some(naga::Binding::Location { location, .. }) if location == index as u32)
            );
            let (layout, attribute) = layouts
                .iter()
                .find_map(|layout| {
                    layout
                        .attributes
                        .iter()
                        .find(|attribute| attribute.shader_location == index as u32)
                        .map(|attribute| (layout, attribute))
                })
                .expect("the field must have an instance attribute");
            assert!(attribute.offset + attribute.format.size() <= layout.array_stride);
            let format = match member.name.as_deref().unwrap() {
                "flags" | "brush" | "placement" => wgpu::VertexFormat::Uint32,
                "stroke_width" => wgpu::VertexFormat::Float32,
                _ => wgpu::VertexFormat::Float32x4,
            };
            assert_eq!(attribute.format, format);
        }
    }
}

#[test]
fn shape_shader_validates_for_webgl() {
    for entry in ["vs_record", "vs_record_solid", "vs_record_gradient_fill"] {
        if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Vertex) {
            panic!("shape.wgsl `{entry}` must lower to GLSL ES 300: {err}");
        }
    }
    for entry in ["fs_main", "fs_solid", "fs_gradient_fill"] {
        if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Fragment) {
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
    for entry_point in ["fs_main", "fs_solid", "fs_gradient_fill"] {
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
        7,
        "a solid batch carries the coverage vectors and nothing of the brush"
    );
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_gradient_fill").len(),
        11,
        "a gradient fill batch carries no colour, stroke or arc vectors"
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
        "fn vs_record_gradient_fill(",
        "fn fs_gradient_fill(",
        "override BRUSH_KIND_FIXED: i32",
        "override TIER_ARENA: bool",
    ] {
        assert!(
            super::SHADER.contains(needle),
            "shape.wgsl must declare `{needle}`"
        );
    }
}

#[test]
fn the_uniform_chunk_sizes_in_the_shader_match_the_run_store() {
    use crate::run_store::{BRUSH_CHUNK, PLACEMENT_CHUNK, STOP_CHUNK};
    for (uniform, _) in RUN_TABLE_DECLARATIONS {
        assert!(super::SHADER.contains(uniform), "missing `{uniform}`");
    }
    assert!(super::SHADER.contains(&format!("array<BrushRecord, {BRUSH_CHUNK}>")));
    assert!(super::SHADER.contains(&format!("array<GradientStop, {STOP_CHUNK}>")));
    assert!(super::SHADER.contains(&format!("array<Placement, {PLACEMENT_CHUNK}>")));
}
