pub(crate) struct ParsedShader {
    module: naga::Module,
    info: naga::valid::ModuleInfo,
}

impl ParsedShader {
    pub(crate) fn new(source: &str) -> Result<Self, String> {
        let module = naga::front::wgsl::parse_str(source)
            .map_err(|err| format!("WGSL parse error: {err}"))?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .map_err(|err| format!("WGSL validation error: {err}"))?;
        Ok(Self { module, info })
    }

    pub(crate) fn lower_to_gles(
        &self,
        entry_point: &str,
        shader_stage: naga::ShaderStage,
        constants: &naga::back::PipelineConstants,
    ) -> Result<String, String> {
        use naga::back::glsl;
        let (module, info) = naga::back::pipeline_constants::process_overrides(
            &self.module,
            &self.info,
            Some((shader_stage, entry_point)),
            constants,
        )
        .map_err(|err| format!("override resolution failed: {err}"))?;
        let (module, info) = wgpu_hal::auxil::prune::prune_decided_branches(module, info)?;
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
        let mut source = String::new();
        glsl::Writer::new(
            &mut source,
            &module,
            &info,
            &options,
            &pipeline_options,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .map_err(|err| format!("GL/WebGL portability validation failed: {err}"))?
        .write()
        .map_err(|err| format!("GL/WebGL portability emission failed: {err}"))?;
        Ok(source)
    }
}
