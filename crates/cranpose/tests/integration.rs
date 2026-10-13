mod host_effects;
mod ios_scene_static;
mod platform_scheduling_static;
mod prelude_surface;
#[cfg(feature = "preview")]
mod recomposition_inspection;
#[cfg(feature = "localization")]
mod system_languages;
#[cfg(feature = "watchos")]
mod watchos;

#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod embedded_view;
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod external_frames;
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod gpu_support;
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod guest_view;
#[cfg(feature = "renderer-pixels")]
mod native_view;
