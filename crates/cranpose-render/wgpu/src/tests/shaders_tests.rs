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
fn blur_shader_writes_one_guarded_call_per_kernel_pair() {
    let shader = super::blur_shader();
    assert!(
        !shader.contains(super::BLUR_KERNEL_PAIRS_MARKER),
        "the marker must be replaced"
    );
    for pair in 0..cranpose_render_common::geometry::BLUR_TAP_PAIRS {
        let call = format!(
            "if (pair_count > {pair}) {{ color = color + kernel_pair(frame, axis, blur.pairs[{pair}], {}.0); }}",
            2 * pair + 1
        );
        assert_eq!(shader.matches(&call).count(), 1, "pair {pair}");
    }
    assert_eq!(
        shader
            .matches("kernel_pair(frame, axis, blur.pairs[")
            .count(),
        cranpose_render_common::geometry::BLUR_TAP_PAIRS
    );
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
fn shape_shader_validates_for_webgpu_in_every_table_form() {
    if let Err(err) = validate_wgsl_module(super::SHADER) {
        panic!("shape.wgsl must validate for WebGPU: {err}");
    }
    if let Err(err) = validate_wgsl_module(&storage_shape_shader()) {
        panic!("shape.wgsl with storage tables must validate for WebGPU: {err}");
    }
    if let Err(err) = validate_wgsl_module(&super::filling_shape_shader()) {
        panic!("shape.wgsl with storage tables and the arc trig fill must validate: {err}");
    }
}

#[test]
fn shape_vertices_receive_every_record_field_from_its_column() {
    let module = naga::front::wgsl::parse_str(&storage_shape_shader()).unwrap();
    for entry_name in [
        "vs_record",
        "vs_record_solid",
        "vs_record_dithered_fill",
        "vs_record_gradient_fill",
    ] {
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
    for entry in [
        "vs_record",
        "vs_record_solid",
        "vs_record_dithered_fill",
        "vs_record_gradient_fill",
    ] {
        if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Vertex) {
            panic!("shape.wgsl `{entry}` must lower to GLSL ES 300: {err}");
        }
    }
    for entry in [
        "fs_main",
        "fs_solid",
        "fs_dithered_fill",
        "fs_gradient_fill",
    ] {
        if let Err(err) = validate_glsl_portability(super::SHADER, entry, ShaderStage::Fragment) {
            panic!("shape.wgsl `{entry}` must lower to GLSL ES 300: {err}");
        }
    }
}

#[test]
fn dithering_solid_batches_validate_for_webgl() {
    let constants = naga::back::PipelineConstants::from_iter([
        ("SHAPE_SOLID".into(), 1.0),
        ("SHAPE_DITHER".into(), 1.0),
        ("SHAPE_KIND_FIXED".into(), 0.0),
    ]);
    for (entry, stage) in [
        ("vs_record_dithered_fill", ShaderStage::Vertex),
        ("fs_dithered_fill", ShaderStage::Fragment),
        ("vs_record_solid", ShaderStage::Vertex),
        ("fs_solid", ShaderStage::Fragment),
    ] {
        validate_glsl_portability_with_constants(super::SHADER, entry, stage, &constants)
            .unwrap_or_else(|error| panic!("{entry}: {error}"));
    }
}

const GLES_VARYING_VECTOR_FLOOR: usize = 15;

/// A fragment entry's input locations, in order, and whether it takes
/// `@builtin(position)`.
fn fragment_inputs(source: &str, entry_point: &str) -> (Vec<u32>, bool) {
    let module = naga::front::wgsl::parse_str(source).expect("shader must parse");
    let entry = module
        .entry_points
        .iter()
        .find(|entry| entry.name == entry_point)
        .unwrap_or_else(|| panic!("{entry_point} missing"));
    let mut locations = Vec::new();
    let mut position = false;
    let mut note = |binding: &Option<naga::Binding>| match binding {
        Some(naga::Binding::Location { location, .. }) => locations.push(*location),
        Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. })) => position = true,
        _ => {}
    };
    for argument in &entry.function.arguments {
        match &module.types[argument.ty].inner {
            naga::TypeInner::Struct { members, .. } => {
                for member in members {
                    note(&member.binding);
                }
            }
            _ => note(&argument.binding),
        }
    }
    locations.sort_unstable();
    (locations, position)
}

fn fragment_input_locations(source: &str, entry_point: &str) -> Vec<u32> {
    fragment_inputs(source, entry_point).0
}

#[test]
fn shape_fragment_inputs_fit_the_gles_varying_floor() {
    for entry_point in [
        "fs_main",
        "fs_solid",
        "fs_clipped_fill",
        "fs_plain_fill",
        "fs_dithered_fill",
        "fs_gradient_fill",
    ] {
        let (locations, position) = fragment_inputs(super::SHADER, entry_point);
        // WebGL counts the fragment position a stage reads against the same
        // vectors (GLSL ES 1.00 appendix A.7), and ANGLE links no more.
        let vectors = locations.len() + usize::from(position);
        assert!(
            vectors <= GLES_VARYING_VECTOR_FLOOR,
            "{entry_point} takes {vectors} varying vectors (locations {locations:?}, \
             position {position}); WebGL 2 only guarantees {GLES_VARYING_VECTOR_FLOOR}"
        );
        let mut deduplicated = locations.clone();
        deduplicated.dedup();
        assert_eq!(deduplicated, locations, "{entry_point} reuses a location");
    }
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_solid").len(),
        7,
        "a solid batch carries the coverage vectors, a fill's interior inside its arc vector, \
         and nothing of the brush"
    );
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_clipped_fill").len(),
        4,
        "a clipped fill drawn flat carries its colour, rect, radii and clip"
    );
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_plain_fill").len(),
        3,
        "an unclipped fill drawn flat carries its colour, rect and radii"
    );
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_dithered_fill").len(),
        4,
        "a flat fill batch holding a gradient its vertices shade adds only the dither position"
    );
    assert_eq!(
        fragment_input_locations(super::SHADER, "fs_gradient_fill").len(),
        11,
        "a gradient fill batch derives its interior and carries no colour, stroke or arc vectors"
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
        "override SHAPE_TIER: u32",
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

#[test]
fn shapes_glyphs_and_images_share_one_depth_step() {
    let step = "const DEPTH_STEP: f32 = 1.0 / 1048576.0;";
    for (name, source) in [
        ("shape", SHADER),
        ("glyph", GLYPH_ATLAS_SHADER),
        ("image", IMAGE_SHADER),
    ] {
        assert!(
            source.contains(step),
            "the {name} shader must order its draws on the shapes' depth step"
        );
    }
}
