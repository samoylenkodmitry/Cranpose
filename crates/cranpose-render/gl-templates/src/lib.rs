#![deny(missing_docs)]
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
//!
//! A page whose first screen was recorded also fetches the exact GLSL of
//! the stages that screen compiles ([`first_screen_glsl`]): each specialized
//! by its pipeline's constants, folded and flattened.

mod exact;
mod first_screen;
mod generate;
mod rust_source;
mod written;

pub use first_screen::first_screen_glsl;
pub use generate::{Generated, MacroKind, Template, generate, module_templates};
pub use rust_source::rust_source;
pub use written::{BlockBinding, TextureBinding};
