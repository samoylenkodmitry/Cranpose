//! Writes GLSL templates of WGSL shaders for WebGL2, in a build script.
//!
//! On WebGL every pipeline's shaders reach the browser as GLSL. Cranpose's
//! renderer writes its shaders in WGSL with pipeline constants, and naga
//! translating them on the page cost a first visit on a Mate 20 X about
//! 0.3 s of the page's only thread before anything showed: parsing the shape
//! shader, applying each pipeline's constants and writing its GLSL.
//!
//! A template is that GLSL written once, at build time, for one entry point:
//! each pipeline constant is a macro, and each branch and function the
//! constants can rule out is under a preprocessor `#if`. For a pipeline, the
//! GL backend of Cranpose's `wgpu` works out the `#if`s from its constants
//! and hands the browser the code they keep, as naga's pruning would
//! (`wgpu::hal::GlslTemplate`).

mod generate;

pub use generate::{
    BlockBinding, Generated, MacroKind, Template, TextureBinding, generate, module_templates,
    rust_source,
};
