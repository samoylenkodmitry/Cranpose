//! Passes over a naga module that shrink a pipeline's shader stages before a
//! driver compiles them: [`fold`] decides the branches a stage's override
//! constants decide, and [`flatten`] inlines its calls and splits its struct
//! variables. The renderer runs them on each native pipeline's stages, and
//! the web build on the stages a WebGL page's first screen compiles.

mod flatten;
mod fold;

pub use flatten::{Unsupported, flatten};
pub use fold::fold;
