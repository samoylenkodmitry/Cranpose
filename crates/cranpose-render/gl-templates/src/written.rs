//! A naga module written as GLSL ES 3.00 for WebGL, as the GL backend's naga
//! options write it, with the names the backend binds its resources by.

use naga::{Handle, Module, TypeInner, back::glsl, valid};

/// A uniform or storage block, by the name the template gives it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockBinding {
    /// The block's name in the GLSL.
    pub name: String,
    /// Whether the block is a storage block instead of a uniform block.
    pub storage: bool,
    /// The WGSL group and binding of the block.
    pub binding: naga::ResourceBinding,
}

/// A texture, by the name the template gives it, with the sampler GLSL ES
/// combines it with.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureBinding {
    /// The texture's name in the GLSL.
    pub name: String,
    /// Whether the texture is a storage texture instead of a sampled one.
    pub storage: bool,
    /// The WGSL group and binding of the texture.
    pub binding: naga::ResourceBinding,
    /// The WGSL group and binding of the sampler combined with the texture, or `None`
    /// when no sampler is.
    pub sampler: Option<naga::ResourceBinding>,
}

// The GLSL of one entry point and what a backend needs to bind it.
pub(crate) struct Written {
    // The GLSL, starting with its `#version` line.
    pub(crate) glsl: String,
    pub(crate) blocks: Vec<BlockBinding>,
    pub(crate) textures: Vec<TextureBinding>,
    // Each vertex input or fragment output by name, with its location,
    // sorted by name.
    pub(crate) varyings: Vec<(String, u32)>,
    pub(crate) clip_distance_count: u32,
}

// `glsl` without the indentation and the blank lines of its lines, which a
// GLSL compiler skips: the indentation was a tenth of the bytes of the
// templates a page embeds.
pub(crate) fn compact(glsl: &str) -> String {
    let mut out = String::with_capacity(glsl.len());
    for line in glsl.lines().map(str::trim).filter(|line| !line.is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

// `entry_point` of `module` as GLSL ES 3.00 for WebGL, its `main`, with the
// blocks, textures and varyings it names. Errors on immediates, which the GL
// backend binds only for code it wrote itself.
pub(crate) fn write(
    module: &Module,
    info: &valid::ModuleInfo,
    entry_point: &str,
    naga_stage: naga::ShaderStage,
) -> Result<Written, String> {
    let (glsl, reflection) = write_glsl(module, info, entry_point, naga_stage)?;
    if !reflection.immediates_items.is_empty() {
        return Err(format!("{entry_point}: immediates are not templated"));
    }
    let (blocks, textures) = bindings(module, &reflection);
    let mut varyings: Vec<(String, u32)> = reflection
        .varying
        .iter()
        .map(|(name, location)| (name.clone(), location.location))
        .collect();
    varyings.sort();
    Ok(Written {
        glsl,
        blocks,
        textures,
        varyings,
        clip_distance_count: reflection.clip_distance_count,
    })
}

// `module` as GLSL ES 3.00 for WebGL with `entry_point` its `main`, as the
// GL backend's naga options write it, and what the writer tells of it.
fn write_glsl(
    module: &Module,
    info: &valid::ModuleInfo,
    entry_point: &str,
    naga_stage: naga::ShaderStage,
) -> Result<(String, glsl::ReflectionInfo), String> {
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: true,
    };
    let pipeline_options = glsl::PipelineOptions {
        shader_stage: naga_stage,
        entry_point: entry_point.to_string(),
        multiview: None,
    };
    let unchecked = naga::proc::BoundsCheckPolicy::Unchecked;
    let policies = naga::proc::BoundsCheckPolicies {
        index: unchecked,
        buffer: unchecked,
        image_load: unchecked,
        binding_array: unchecked,
    };
    let mut written = String::new();
    let reflection = glsl::Writer::new(
        &mut written,
        module,
        info,
        &options,
        &pipeline_options,
        policies,
    )
    .and_then(|mut writer| writer.write())
    .map_err(|error| format!("{entry_point}: {error}"))?;
    Ok((written, reflection))
}

// The blocks and textures the written GLSL names, in a stable order.
fn bindings(
    module: &Module,
    reflection: &glsl::ReflectionInfo,
) -> (Vec<BlockBinding>, Vec<TextureBinding>) {
    let binding = |handle: Handle<naga::GlobalVariable>| module.global_variables[handle].binding;
    let mut blocks: Vec<BlockBinding> = reflection
        .uniforms
        .iter()
        .filter_map(|(handle, name)| {
            Some(BlockBinding {
                name: name.clone(),
                storage: matches!(
                    module.global_variables[*handle].space,
                    naga::AddressSpace::Storage { .. }
                ),
                binding: binding(*handle)?,
            })
        })
        .collect();
    blocks.sort();
    let mut textures: Vec<TextureBinding> = reflection
        .texture_mapping
        .iter()
        .filter_map(|(name, mapping)| {
            Some(TextureBinding {
                name: name.clone(),
                storage: matches!(
                    module.types[module.global_variables[mapping.texture].ty].inner,
                    TypeInner::Image {
                        class: naga::ImageClass::Storage { .. },
                        ..
                    }
                ),
                binding: binding(mapping.texture)?,
                sampler: mapping.sampler.and_then(binding),
            })
        })
        .collect();
    textures.sort();
    (blocks, textures)
}
