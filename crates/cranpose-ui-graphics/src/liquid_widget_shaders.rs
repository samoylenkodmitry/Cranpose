// The runtime shaders of the Liquid widgets' content and lighting, each the
// runtime shader prelude, the glass geometry and its fragment stage. As
// framework sources (`BUILTIN_RUNTIME_SHADER_SOURCES`), the framework's tests
// validate them for every backend, and a renderer does not on each launch.

/// The vibrancy of a Liquid control's ink over its backdrop.
pub const LIQUID_VIBRANCY_WGSL: &str =
    runtime_shader_wgsl!("glass_geometry.wgsl", "liquid_vibrancy.wgsl");

/// The lighting composited over a Liquid tab bar's backdrop.
pub const LIQUID_TAB_LIGHTING_WGSL: &str =
    runtime_shader_wgsl!("glass_geometry.wgsl", "liquid_tab_lighting.wgsl");

/// The content of a Liquid control's material, its foreground or its body.
pub const LIQUID_CONTROL_CONTENT_WGSL: &str =
    runtime_shader_wgsl!("glass_geometry.wgsl", "liquid_control_content.wgsl");

/// The contact lighting of a Liquid floating button.
pub const LIQUID_FLOATING_LIGHTING_WGSL: &str =
    runtime_shader_wgsl!("glass_geometry.wgsl", "liquid_floating_lighting.wgsl");
