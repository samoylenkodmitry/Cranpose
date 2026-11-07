//! The exact GLSL of the WebGL stages a page's first screen compiles, which
//! a page built with first-screen records fetches beside its wasm
//! (`first_screen_glsl.txt`, written by `cranpose-first-screen-glsl` of
//! `cranpose-gl-templates`, whose `first_screen_glsl` tells the format). A
//! pipeline whose module, entry points and constants match a program of the
//! file compiles the program's GLSL, specialized, folded and flattened at
//! build time, instead of its stages' templates. On a Mate 20 X a shape
//! program's compile and link took 36-38 ms that way against 53-56 ms from
//! its templates, and the gauntlet's first content came about 90 ms sooner.
//!
//! A recording run lists the stages its first screen compiles for the tool
//! (see [`list_first_screen_stages`]).

use std::{
    fmt::Write as _,
    sync::{Mutex, OnceLock, PoisonError},
};

use cranpose_ui_graphics::framework_shaders::SOURCES_KEY;
use wgpu::hal::{GlslBlock, GlslTemplate, GlslTexture};

/// Why [`use_first_screen_glsl`] refused a file.
#[derive(Debug, thiserror::Error)]
pub enum FirstScreenGlslError {
    /// The file is not one `cranpose-first-screen-glsl` wrote.
    #[error("not a first-screen GLSL file")]
    Malformed,
    /// The file was written from other framework shaders than the page's.
    #[error("the file was written from other shaders")]
    OtherShaders,
    /// The page took a file already.
    #[error("the page uses first-screen GLSL already")]
    InUse,
}

const HEADER: &str = "cranpose-first-screen-glsl/1/";

static PROGRAMS: OnceLock<Vec<Program>> = OnceLock::new();

// The stages listed since `list_first_screen_stages`, a line each, or
// `None` when no listing runs.
static LISTED: Mutex<Option<String>> = Mutex::new(None);

// Lists the stages of a pipeline built from a built-in module: its name,
// its entry points and the pipeline's constants.
pub(crate) type List = fn(&str, (&str, &str), &[(&str, f64)]);

// What lists a pipeline's stages, set by `list_first_screen_stages`: a page
// that never lists them carries no listing code.
static LISTING: OnceLock<List> = OnceLock::new();

struct Program {
    // The program's stages as `stages_line` writes them.
    stages_line: &'static str,
    // The vertex stage, then the fragment stage.
    stages: [GlslTemplate; 2],
}

/// Builds each WebGL pipeline whose module, entry points and constants a
/// program of `file` names from that program's GLSL: a page calls it with
/// its `first_screen_glsl.txt` before it builds its first screen's
/// pipelines.
pub fn use_first_screen_glsl(file: Vec<u8>) -> Result<(), FirstScreenGlslError> {
    let key = Fields { rest: &file }
        .text()
        .and_then(|header| header.strip_prefix(HEADER))
        .ok_or(FirstScreenGlslError::Malformed)?;
    if key != SOURCES_KEY.to_string() {
        return Err(FirstScreenGlslError::OtherShaders);
    }
    if PROGRAMS.get().is_some() {
        return Err(FirstScreenGlslError::InUse);
    }
    // The device keeps a module's templates for as long as the page runs,
    // as it keeps the build's own, so the file's text lives as long.
    let mut fields = Fields {
        rest: Vec::leak(file),
    };
    let _header = fields.text();
    let programs = fields
        .programs()
        .filter(|_| fields.rest.is_empty())
        .ok_or(FirstScreenGlslError::Malformed)?;
    PROGRAMS
        .set(programs)
        .map_err(|_| FirstScreenGlslError::InUse)
}

/// Starts listing every WebGL stage a pipeline is built with, for a
/// recording run of the page's first screen: [`first_screen_stages`] takes
/// the list.
pub fn list_first_screen_stages() {
    *LISTED.lock().unwrap_or_else(PoisonError::into_inner) = Some(String::new());
    let _ = LISTING.set(list);
}

/// The WebGL stages listed since [`list_first_screen_stages`], a line each
/// as `cranpose-first-screen-glsl` reads them, and ends the listing.
pub fn first_screen_stages() -> String {
    LISTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .unwrap_or_default()
}

// A pipeline's stages as a line: `MODULE|VERTEX|FRAGMENT|NAME=VALUE,...`.
fn stages_line(
    module: &str,
    (vertex, fragment): (&str, &str),
    constants: &[(&str, f64)],
) -> String {
    let mut line = format!("{module}|{vertex}|{fragment}|");
    for (index, (name, value)) in constants.iter().enumerate() {
        let comma = if index == 0 { "" } else { "," };
        let _ = write!(line, "{comma}{name}={value}");
    }
    line
}

// What lists the stages of a pipeline built from a built-in module while a
// listing runs.
pub(crate) fn listing() -> Option<List> {
    LISTING.get().copied()
}

fn list(module: &str, entries: (&str, &str), constants: &[(&str, f64)]) {
    if let Some(listed) = LISTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        listed.push_str(&stages_line(module, entries, constants));
        listed.push('\n');
    }
}

// The two stages of the program of the built-in `module` at `entries` with
// `constants`, where the page's file has one.
pub(crate) fn program(
    module: &str,
    entries: (&str, &str),
    constants: &[(&str, f64)],
) -> Option<&'static [GlslTemplate]> {
    let programs = PROGRAMS.get()?;
    let line = stages_line(module, entries, constants);
    programs
        .iter()
        .find(|program| program.stages_line == line)
        .map(|program| program.stages.as_slice())
}

// The file's fields, read one after another.
struct Fields<'a> {
    rest: &'a [u8],
}

impl<'a> Fields<'a> {
    fn text(&mut self) -> Option<&'a str> {
        let colon = self.rest.iter().position(|&byte| byte == b':')?;
        let length = decimal(&self.rest[..colon])?;
        let (field, rest) = self.rest[colon + 1..].split_at_checked(length as usize)?;
        self.rest = rest;
        std::str::from_utf8(field).ok()
    }

    fn number(&mut self) -> Option<u32> {
        decimal(self.text()?.as_bytes())
    }

    fn binding(&mut self) -> Option<naga::ResourceBinding> {
        Some(naga::ResourceBinding {
            group: self.number()?,
            binding: self.number()?,
        })
    }
}

impl Fields<'static> {
    fn programs(&mut self) -> Option<Vec<Program>> {
        let mut programs = Vec::new();
        for _ in 0..self.number()? {
            let stages_line = self.text()?;
            let mut names = stages_line.split('|').skip(1);
            let (vertex, fragment) = (names.next()?, names.next()?);
            let stages = [
                self.stage(vertex, naga::ShaderStage::Vertex)?,
                self.stage(fragment, naga::ShaderStage::Fragment)?,
            ];
            programs.push(Program {
                stages_line,
                stages,
            });
        }
        Some(programs)
    }

    // Each list is kept for as long as the page runs, as the file's text.
    fn stage(
        &mut self,
        entry_point: &'static str,
        stage: naga::ShaderStage,
    ) -> Option<GlslTemplate> {
        let glsl = Vec::leak(vec![self.text()?]);
        let mut blocks = Vec::new();
        for _ in 0..self.number()? {
            blocks.push(GlslBlock {
                name: self.text()?,
                storage: self.number()? != 0,
                binding: self.binding()?,
            });
        }
        let mut textures = Vec::new();
        for _ in 0..self.number()? {
            let (name, storage, binding) = (self.text()?, self.number()? != 0, self.binding()?);
            let sampler = if self.number()? != 0 {
                Some(self.binding()?)
            } else {
                None
            };
            textures.push(GlslTexture {
                name,
                storage,
                binding,
                sampler,
            });
        }
        let mut varyings = Vec::new();
        for _ in 0..self.number()? {
            varyings.push((self.text()?, self.number()?));
        }
        Some(GlslTemplate {
            entry_point,
            stage,
            glsl,
            macros: &[],
            blocks: blocks.leak(),
            textures: textures.leak(),
            varyings: varyings.leak(),
            clip_distance_count: self.number()?,
        })
    }
}

// A field's decimal digits as a number: the reader's one number parser.
fn decimal(digits: &[u8]) -> Option<u32> {
    digits.iter().try_fold(0u32, |number, &digit| {
        let digit = digit.checked_sub(b'0').filter(|digit| *digit < 10)?;
        number.checked_mul(10)?.checked_add(u32::from(digit))
    })
}
