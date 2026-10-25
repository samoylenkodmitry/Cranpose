use std::{collections::BTreeMap, fmt::Write as _};

use crate::Template;

/// The Rust source of a static for each of `modules`, named as given, that
/// holds the module's templates as `wgpu::hal::GlslTemplate`s, for a crate
/// that depends on `wgpu`. It imports `wgpu`'s `hal` and `naga` once and names
/// every type through them. Each piece of GLSL the templates share is written
/// once.
pub fn rust_source(modules: &[(&str, Vec<Template>)]) -> String {
    let mut pool: BTreeMap<&str, usize> = BTreeMap::new();
    let mut pieces_out = String::new();
    let mut statics = String::new();
    for (name, templates) in modules {
        let _ = write!(
            statics,
            "pub(crate) static {name}: &[hal::GlslTemplate] = &["
        );
        for template in templates {
            let generated = &template.generated;
            let _ = write!(
                statics,
                "hal::GlslTemplate {{ entry_point: {:?}, stage: naga::ShaderStage::{:?}, glsl: &[",
                template.entry_point, template.stage
            );
            for piece in pieces(&generated.glsl) {
                let next = pool.len();
                let index = *pool.entry(piece).or_insert_with(|| {
                    let _ = writeln!(pieces_out, "const P{next}: &str = {piece:?};");
                    next
                });
                let _ = write!(statics, "P{index},");
            }
            statics.push_str("], macros: &[");
            for (name, kind) in &generated.macros {
                let _ = write!(
                    statics,
                    "hal::GlslMacro {{ constant: {name:?}, name: \"CP_{name}\", kind: hal::GlslMacroKind::{kind:?} }},"
                );
            }
            statics.push_str("], blocks: &[");
            for block in &generated.blocks {
                let _ = write!(
                    statics,
                    "hal::GlslBlock {{ name: {:?}, storage: {}, binding: {} }},",
                    block.name,
                    block.storage,
                    resource_binding(block.binding)
                );
            }
            statics.push_str("], textures: &[");
            for texture in &generated.textures {
                let sampler = match texture.sampler {
                    Some(sampler) => format!("Some({})", resource_binding(sampler)),
                    None => "None".to_string(),
                };
                let _ = write!(
                    statics,
                    "hal::GlslTexture {{ name: {:?}, storage: {}, binding: {}, sampler: {sampler} }},",
                    texture.name,
                    texture.storage,
                    resource_binding(texture.binding)
                );
            }
            statics.push_str("], varyings: &[");
            for (name, location) in &generated.varyings {
                let _ = write!(statics, "({name:?}, {location}),");
            }
            let _ = write!(
                statics,
                "], clip_distance_count: {} }},",
                generated.clip_distance_count
            );
        }
        statics.push_str("];\n");
    }
    format!("use wgpu::{{hal, naga}};\n{pieces_out}{statics}")
}

/// `glsl` cut where templates are likely to share what follows: after the
/// `#version` line, each top-level declaration, each function and each
/// `#if` and `#endif`.
fn pieces(glsl: &str) -> impl Iterator<Item = &str> {
    let mut start = 0;
    let mut end = 0;
    let mut first = true;
    glsl.split_inclusive('\n').filter_map(move |line| {
        end += line.len();
        let text = line.trim_end();
        let top_level = !text.starts_with([' ', '\t', '#']);
        let cut = std::mem::take(&mut first)
            || text.starts_with('}')
            || text.starts_with("#endif")
            || text.starts_with("#if ")
            || (top_level && text.ends_with(';'))
            || end == glsl.len();
        let piece = cut.then(|| &glsl[start..end]);
        if cut {
            start = end;
        }
        piece
    })
}

fn resource_binding(binding: naga::ResourceBinding) -> String {
    format!(
        "naga::ResourceBinding {{ group: {}, binding: {} }}",
        binding.group, binding.binding
    )
}
