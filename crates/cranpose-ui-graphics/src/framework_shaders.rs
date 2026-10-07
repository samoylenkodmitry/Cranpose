//! WGSL sources for the framework's rendering pipelines.
//!
//! They live in this pure data crate, packaged with a crate every renderer
//! already depends on. The build script embeds them without their comments.

/// Batched shape shader. The uniform array lengths in the source are the
/// wasm/downlevel defaults; native pipelines rewrite them per device class.
pub const SHAPE_WGSL: &str = framework_wgsl!("shape.wgsl");
/// The arc trig fill's compute stage, appended to the storage tables' shape
/// shader on a device with one.
pub const ARC_TRIG_FILL_WGSL: &str = framework_wgsl!("arc_trig_fill.wgsl");
pub const IMAGE_WGSL: &str = framework_wgsl!("image.wgsl");
pub const GLYPH_ATLAS_WGSL: &str = framework_wgsl!("glyph_atlas.wgsl");
/// Glyphs drawn from the retained runs' glyph arena, appended to the glyph
/// shader on a device whose vertex stage reads storage buffers.
pub const GLYPH_PULLED_WGSL: &str = framework_wgsl!("glyph_pulled.wgsl");
/// Elevation shadows drawn straight into a pass: Skia's round rect shadow
/// at each pixel.
pub const RRECT_SHADOW_WGSL: &str = framework_wgsl!("rrect_shadow.wgsl");

/// Fullscreen-triangle vertex stage shared by the post-process shaders.
pub const FULLSCREEN_QUAD_VS_WGSL: &str = framework_wgsl!("fullscreen_quad_vs.wgsl");
/// SDF rounded-rectangle helper shared by shape masking and blits.
pub const SDF_ROUNDED_RECT_FN_WGSL: &str = framework_wgsl!("sdf_rounded_rect_fn.wgsl");
/// Box-filtered composite sampling helper shared by the blit shaders.
pub const COMPOSITE_SAMPLE_FN_WGSL: &str = framework_wgsl!("composite_sample_fn.wgsl");

pub const BLUR_FS_WGSL: &str = framework_wgsl!("blur_fs.wgsl");
pub const OFFSET_FS_WGSL: &str = framework_wgsl!("offset_fs.wgsl");
pub const BLIT_FS_WGSL: &str = framework_wgsl!("blit_fs.wgsl");
pub const BLIT_FS_MAIN_WGSL: &str = framework_wgsl!("blit_fs_main.wgsl");
pub const PROJECTIVE_BLIT_FS_WGSL: &str = framework_wgsl!("projective_blit_fs.wgsl");
pub const PROJECTIVE_BLIT_MAIN_WGSL: &str = framework_wgsl!("projective_blit_main.wgsl");

/// RuntimeShader source for GPU text brush effects (gradient/stroked text).
pub const GPU_TEXT_BRUSH_EFFECT_WGSL: &str = framework_wgsl!("gpu_text_brush_effect.wgsl");

/// Changes whenever any framework WGSL file does, and with nothing else, so a
/// cache of what a driver compiled from these shaders can tell its blob was
/// filled by other ones.
pub const SOURCES_KEY: u64 = include!(concat!(env!("OUT_DIR"), "/framework_shaders_key.rs"));
