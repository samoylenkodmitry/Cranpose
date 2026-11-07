//! One pipeline's stage as the GLSL it compiles: specialized by the
//! pipeline's constants, folded and flattened as the renderer does a native
//! pipeline's stages (`cranpose_shader_passes`), with no macros. The
//! browser compiles and links such a stage about a third faster than its
//! template with the same constants defined.

use naga::{Module, back::PipelineConstants, valid};

use crate::Generated;

// The `naga_stage` entry point `entry_point` of the validated `module` with
// `constants` applied, folded and flattened, as GLSL for WebGL2. A stage the
// flatten pass refuses stays folded.
pub(crate) fn exact(
    (module, info): (&Module, &valid::ModuleInfo),
    entry_point: &str,
    naga_stage: naga::ShaderStage,
    constants: &PipelineConstants,
) -> Result<Generated, String> {
    let folded = || -> Result<Module, String> {
        let (specialized, _) = naga::back::pipeline_constants::process_overrides(
            module,
            info,
            Some((naga_stage, entry_point)),
            constants,
        )
        .map_err(|error| format!("{entry_point}: {error:?}"))?;
        let mut folded = specialized.into_owned();
        cranpose_shader_passes::fold(&mut folded);
        Ok(folded)
    };
    let mut stage = folded()?;
    if cranpose_shader_passes::flatten(&mut stage).is_err() {
        // The pass left the stage part flattened.
        stage = folded()?;
    }
    naga::compact::compact(&mut stage, naga::compact::KeepUnused::No);
    let info = crate::generate::validate(&stage)?;
    let written = crate::written::write(&stage, &info, entry_point, naga_stage)?;
    Ok(Generated {
        glsl: written.glsl,
        macros: Vec::new(),
        blocks: written.blocks,
        textures: written.textures,
        varyings: written.varyings,
        clip_distance_count: written.clip_distance_count,
    })
}
