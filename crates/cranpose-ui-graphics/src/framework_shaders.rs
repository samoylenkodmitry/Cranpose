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
/// The image shader, with the rounded-rect distance its rounded clip takes.
pub const IMAGE_WGSL: &str = concat!(
    framework_wgsl!("sdf_rounded_rect_fn.wgsl"),
    framework_wgsl!("image.wgsl"),
);
/// Draws text glyphs from the glyph atlas texture.
pub const GLYPH_ATLAS_WGSL: &str = framework_wgsl!("glyph_atlas.wgsl");
/// Elevation shadows drawn straight into a pass: Skia's round rect shadow
/// at each pixel.
pub const RRECT_SHADOW_WGSL: &str = framework_wgsl!("rrect_shadow.wgsl");

/// Fullscreen-triangle vertex stage shared by the post-process shaders.
pub const FULLSCREEN_QUAD_VS_WGSL: &str = framework_wgsl!("fullscreen_quad_vs.wgsl");

/// One separable blur pass. Its uniform table holds `BLUR_TAP_PAIRS` pairs.
pub const BLUR_WGSL: &str = concat!(
    framework_wgsl!("fullscreen_quad_vs.wgsl"),
    framework_wgsl!("blur_fs.wgsl"),
);
/// Shifts a layer by an offset in physical pixels, leaving the vacated area transparent.
pub const OFFSET_WGSL: &str = concat!(
    framework_wgsl!("fullscreen_quad_vs.wgsl"),
    framework_wgsl!("offset_fs.wgsl"),
);
/// Composites a layer, with the rounded-rect distance of its clip and the
/// box-filtered sampling of a scaled source.
pub const BLIT_WGSL: &str = concat!(
    framework_wgsl!("fullscreen_quad_vs.wgsl"),
    framework_wgsl!("sdf_rounded_rect_fn.wgsl"),
    framework_wgsl!("blit_fs.wgsl"),
    framework_wgsl!("composite_sample_fn.wgsl"),
    framework_wgsl!("blit_fs_main.wgsl"),
);
/// Composites a layer under a projective (perspective) transform.
pub const PROJECTIVE_BLIT_WGSL: &str = concat!(
    framework_wgsl!("projective_blit_fs.wgsl"),
    framework_wgsl!("composite_sample_fn.wgsl"),
    framework_wgsl!("projective_blit_main.wgsl"),
);
/// Rounds a frame to 8-bit color for an output that stores more.
pub const OUTPUT_CONVERSION_WGSL: &str = framework_wgsl!("output_conversion.wgsl");
/// Draws a composable's surface on a mesh in a 3D scene, depth tested.
pub const MESH_SURFACE_WGSL: &str = framework_wgsl!("mesh_surface.wgsl");

/// Each module the renderer builds pipelines from, by name: the shaders a
/// WebGL page gets as GLSL templates written at build time.
pub const MODULES: [(&str, &str); 10] = [
    ("SHAPE", SHAPE_WGSL),
    ("IMAGE", IMAGE_WGSL),
    ("GLYPH_ATLAS", GLYPH_ATLAS_WGSL),
    ("RRECT_SHADOW", RRECT_SHADOW_WGSL),
    ("BLUR", BLUR_WGSL),
    ("OFFSET", OFFSET_WGSL),
    ("BLIT", BLIT_WGSL),
    ("PROJECTIVE_BLIT", PROJECTIVE_BLIT_WGSL),
    ("OUTPUT_CONVERSION", OUTPUT_CONVERSION_WGSL),
    ("MESH_SURFACE", MESH_SURFACE_WGSL),
];

/// RuntimeShader source for GPU text brush effects (gradient/stroked text).
pub const GPU_TEXT_BRUSH_EFFECT_WGSL: &str = runtime_shader_wgsl!("gpu_text_brush_effect.wgsl");

/// Changes whenever any framework WGSL file does, and with nothing else, so a
/// cache of what a driver compiled from these shaders can tell its blob was
/// filled by other ones.
pub const SOURCES_KEY: u64 = include!(concat!(env!("OUT_DIR"), "/framework_shaders_key.rs"));
