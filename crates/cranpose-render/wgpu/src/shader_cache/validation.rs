use naga::ShaderStage;

// The framework's runtime shaders, each with whether its color ignores the
// pixel's place on the target. On the web a runtime shader of one of these
// sources takes no validation on the page: its tests validate it
// (`framework_runtime_shaders_validate_as_listed`), and the page's naga took
// ~100 ms of a Galaxy Tab S7+'s first glass on WebGPU, where the browser
// parses the WGSL again itself.
#[cfg(any(target_arch = "wasm32", test))]
pub(super) const FRAMEWORK_RUNTIME_SHADERS: [(&str, bool); 6] = [
    (cranpose_ui_graphics::LIQUID_GLASS_WGSL, false),
    (cranpose_ui_graphics::GRADIENT_BLUR_WGSL, false),
    (cranpose_ui_graphics::GRADIENT_CUT_MASK_WGSL, true),
    (cranpose_ui_graphics::GRADIENT_FADE_DST_OUT_WGSL, true),
    (cranpose_ui_graphics::ROUNDED_ALPHA_MASK_WGSL, true),
    (crate::pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER, true),
];

/// The parsed `source` and whether its fragment stage ignores the pixel
/// position, once it holds the entry points a runtime shader needs and
/// validates for `backend`.
pub(super) fn validate_runtime_shader_source(
    source: &str,
    backend: wgpu::Backend,
) -> Result<(naga::Module, bool), String> {
    let (module, module_info, fragment) =
        validate_shader_source(source, backend, ("fullscreen_vs", "effect_fs"))?;
    let position_independent = fragment_position_independent(&module, &module_info, fragment);
    Ok((module, position_independent))
}

/// The parsed `source`, what validation learned of it and the index of its
/// fragment entry point, once it holds the `vertex` and `fragment` entry
/// points and validates for `backend`.
pub(super) fn validate_shader_source(
    source: &str,
    backend: wgpu::Backend,
    (vertex, fragment): (&str, &str),
) -> Result<(naga::Module, naga::valid::ModuleInfo, usize), String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|err| format!("WGSL parse error: {err}"))?;
    let entry = |stage: ShaderStage, name: &str| {
        module
            .entry_points
            .iter()
            .position(|ep| ep.stage == stage && ep.name == name)
    };
    if entry(ShaderStage::Vertex, vertex).is_none() {
        return Err(format!("missing required vertex entry point `{vertex}`"));
    }
    let Some(fragment_index) = entry(ShaderStage::Fragment, fragment) else {
        return Err(format!(
            "missing required fragment entry point `{fragment}`"
        ));
    };

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let module_info = validator
        .validate(&module)
        .map_err(|err| format!("WGSL validation error: {err}"))?;

    if backend == wgpu::Backend::Gl {
        validate_glsl_portability(&module, &module_info, vertex, ShaderStage::Vertex)?;
        validate_glsl_portability(&module, &module_info, fragment, ShaderStage::Fragment)?;
    }
    Ok((module, module_info, fragment_index))
}

fn is_position(binding: &Option<naga::Binding>) -> bool {
    matches!(
        binding,
        Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. }))
    )
}

fn fragment_position_independent(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    fragment: usize,
) -> bool {
    let function = &module.entry_points[fragment].function;
    let info = info.get_entry_point(fragment);
    function.expressions.iter().all(|(handle, expression)| {
        let naga::Expression::FunctionArgument(index) = expression else {
            return true;
        };
        let argument = &function.arguments[*index as usize];
        if is_position(&argument.binding) {
            return info[handle].ref_count == 0;
        }
        let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner else {
            return true;
        };
        if !members.iter().any(|member| is_position(&member.binding)) {
            return true;
        }
        let safe_uses = function
            .expressions
            .iter()
            .filter(|(_, expression)| {
                matches!(expression,
                naga::Expression::AccessIndex { base, index }
                    if *base == handle && !is_position(&members[*index as usize].binding))
            })
            .count();
        safe_uses == info[handle].ref_count
    })
}

#[cfg(any(test, feature = "backend-gles", target_arch = "wasm32"))]
fn validate_glsl_portability(
    module: &naga::Module,
    module_info: &naga::valid::ModuleInfo,
    entry_point: &str,
    shader_stage: ShaderStage,
) -> Result<(), String> {
    use naga::back::glsl;

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
        module,
        module_info,
        Some((shader_stage, entry_point)),
        &naga::back::PipelineConstants::default(),
    )
    .map_err(|err| format!("override resolution failed for `{entry_point}`: {err}"))?;
    let mut writer = glsl::Writer::new(
        &mut glsl_source,
        &module,
        &module_info,
        &options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|err| format!("GL/WebGL portability validation failed for `{entry_point}`: {err}"))?;

    writer
        .write()
        .map(|_| ())
        .map_err(|err| format!("GL/WebGL portability emission failed for `{entry_point}`: {err}"))
}

#[cfg(not(any(test, feature = "backend-gles", target_arch = "wasm32")))]
fn validate_glsl_portability(
    _module: &naga::Module,
    _module_info: &naga::valid::ModuleInfo,
    entry_point: &str,
    _shader_stage: ShaderStage,
) -> Result<(), String> {
    Err(format!(
        "GL backend active for `{entry_point}` but GL support is not compiled in \
         (enable `backend-gles`, or `renderer-wgpu-gles` on the `cranpose` crate)"
    ))
}
