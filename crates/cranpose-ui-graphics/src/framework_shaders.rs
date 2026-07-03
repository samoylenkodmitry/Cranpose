//! WGSL sources for the framework's rendering pipelines.
//!
//! They live in this pure data crate so every renderer backend — the wgpu
//! renderer at runtime, and the slim Vulkan backend's build script — reads
//! the identical text, packaged with a crate they already depend on.

/// Batched shape shader. The uniform array lengths in the source are the
/// wasm/downlevel defaults; native pipelines rewrite them per device class.
pub const SHAPE_WGSL: &str = include_str!("../shaders/shape.wgsl");
pub const IMAGE_WGSL: &str = include_str!("../shaders/image.wgsl");
pub const GLYPH_ATLAS_WGSL: &str = include_str!("../shaders/glyph_atlas.wgsl");

/// Fullscreen-triangle vertex stage shared by the post-process shaders.
pub const FULLSCREEN_QUAD_VS_WGSL: &str = include_str!("../shaders/fullscreen_quad_vs.wgsl");
/// SDF rounded-rectangle helper shared by shape masking and blits.
pub const SDF_ROUNDED_RECT_FN_WGSL: &str = include_str!("../shaders/sdf_rounded_rect_fn.wgsl");
/// Box-filtered composite sampling helper shared by the blit shaders.
pub const COMPOSITE_SAMPLE_FN_WGSL: &str = include_str!("../shaders/composite_sample_fn.wgsl");

pub const BLUR_FS_WGSL: &str = include_str!("../shaders/blur_fs.wgsl");
pub const BLUR_ROUNDED_MASK_FS_WGSL: &str = include_str!("../shaders/blur_rounded_mask_fs.wgsl");
pub const OFFSET_FS_WGSL: &str = include_str!("../shaders/offset_fs.wgsl");
pub const BLIT_FS_WGSL: &str = include_str!("../shaders/blit_fs.wgsl");
pub const BLIT_FS_MAIN_WGSL: &str = include_str!("../shaders/blit_fs_main.wgsl");
pub const PROJECTIVE_BLIT_FS_WGSL: &str = include_str!("../shaders/projective_blit_fs.wgsl");
pub const PROJECTIVE_BLIT_MAIN_WGSL: &str = include_str!("../shaders/projective_blit_main.wgsl");

/// RuntimeShader source for GPU text brush effects (gradient/stroked text).
pub const GPU_TEXT_BRUSH_EFFECT_WGSL: &str = include_str!("../shaders/gpu_text_brush_effect.wgsl");
