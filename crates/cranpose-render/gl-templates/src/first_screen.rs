//! The exact GLSL of the WebGL stages a page's first screen compiles, which
//! the page fetches beside its wasm as it starts, so the browser compiles
//! and links each first screen program from the stage's own GLSL instead of
//! its template (see [`crate::exact`]).
//!
//! The file is UTF-8 text made of fields, each its length in bytes in
//! decimal, a colon, then its bytes. Its first field is
//! `cranpose-first-screen-glsl/1/` and the key of the framework shaders it
//! was written from (`cranpose_ui_graphics::framework_shaders::SOURCES_KEY`),
//! its second the number of programs. Each program then has its line as
//! listed, then its vertex stage and its fragment stage, each as: the GLSL;
//! the number of blocks, then each block's name, `1` for storage or `0`,
//! group and binding; the number of textures, then each texture's name,
//! storage, group, binding, `1` and the sampler's group and binding or `0`
//! for no sampler; the number of varyings, then each one's name and
//! location; and the number of clip distances.

use std::collections::{BTreeMap, BTreeSet};

use naga::{Module, back::PipelineConstants, valid};

use crate::Generated;

/// The file of the exact GLSL of each stage listed in `stages`, one line a
/// program: `MODULE|VERTEX|FRAGMENT|NAME=VALUE,...`, the name of a module of
/// `modules`, its entry points and the pipeline's constants. `sources_key` is
/// the key of the shaders `modules` holds: a page whose shaders differ
/// refuses the file.
pub fn first_screen_glsl(
    modules: &[(&str, &str)],
    sources_key: u64,
    stages: &str,
) -> Result<String, String> {
    let listed: BTreeSet<&str> = stages
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut parsed: BTreeMap<&str, (Module, valid::ModuleInfo)> = BTreeMap::new();
    let mut file = Fields::default();
    file.text(&format!("cranpose-first-screen-glsl/1/{sources_key}"));
    file.number(listed.len());
    for line in listed {
        let [name, vertex, fragment, constants] = line.split('|').collect::<Vec<_>>()[..] else {
            return Err(format!("not a stage line: {line}"));
        };
        if !parsed.contains_key(name) {
            let (_, source) = modules
                .iter()
                .find(|(module, _)| *module == name)
                .ok_or_else(|| format!("no shader module {name}"))?;
            let module = naga::front::wgsl::parse_str(source)
                .map_err(|error| error.emit_to_string(source))?;
            let info = crate::generate::validate(&module)?;
            parsed.insert(name, (module, info));
        }
        let (module, info) = &parsed[name];
        let values = pipeline_constants(constants)?;
        file.text(line);
        for (entry, stage) in [
            (vertex, naga::ShaderStage::Vertex),
            (fragment, naga::ShaderStage::Fragment),
        ] {
            file.stage(&crate::exact::exact((module, info), entry, stage, &values)?);
        }
    }
    Ok(file.0)
}

// The constants a stage line lists, `NAME=VALUE` apart by commas.
fn pipeline_constants(listed: &str) -> Result<PipelineConstants, String> {
    listed
        .split(',')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("not a constant: {pair}"))?;
            let value = value
                .parse::<f64>()
                .map_err(|error| format!("{pair}: {error}"))?;
            Ok((name.to_string(), value))
        })
        .collect()
}

// The file's text, written a field at a time.
#[derive(Default)]
struct Fields(String);

impl Fields {
    fn text(&mut self, text: &str) {
        self.0.push_str(&text.len().to_string());
        self.0.push(':');
        self.0.push_str(text);
    }

    fn number(&mut self, number: impl ToString + Copy) {
        self.text(&number.to_string());
    }

    fn binding(&mut self, binding: naga::ResourceBinding) {
        self.number(binding.group);
        self.number(binding.binding);
    }

    fn stage(&mut self, stage: &Generated) {
        self.text(&stage.glsl);
        self.number(stage.blocks.len());
        for block in &stage.blocks {
            self.text(&block.name);
            self.number(u8::from(block.storage));
            self.binding(block.binding);
        }
        self.number(stage.textures.len());
        for texture in &stage.textures {
            self.text(&texture.name);
            self.number(u8::from(texture.storage));
            self.binding(texture.binding);
            self.number(u8::from(texture.sampler.is_some()));
            if let Some(sampler) = texture.sampler {
                self.binding(sampler);
            }
        }
        self.number(stage.varyings.len());
        for (name, location) in &stage.varyings {
            self.text(name);
            self.number(*location);
        }
        self.number(stage.clip_distance_count);
    }
}
