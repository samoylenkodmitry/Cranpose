pub(crate) mod backend;
mod geometry;
mod render_paths;

pub(crate) use backend::DevicePixelBounds;
pub(crate) use backend::{CachedLayerSurface, LayerSurfaceTexture, SurfaceExecutionBackend};
pub(crate) use geometry::{
    axis_aligned_quad_rect, canonicalize_device_coordinate, canonicalized_scaled_quad,
    canonicalized_scaled_rect, device_pixel_bounds_for_rect, offscreen_byte_size, scaled_quad,
    snap_delta_for_anchor, snap_motion_stable_dest_quad,
    translation_stable_anchored_device_pixel_bounds,
};
#[cfg(test)]
pub(crate) use geometry::{
    clamp_effect_surface_scale, device_pixel_exact_surface_rect, surface_target_size,
    visible_layer_rect,
};
pub(crate) use render_paths::{
    apply_backdrop_layer_to_target, backdrop_underlay_is_covered_by_local_content,
    composite_surface_to_view, layer_source_uses_external_backdrop_underlay,
    layer_surface_translation_context, render_effect_layer_to_target, render_layer_surface,
    render_root_direct, root_direct_scene_events_are_supported,
};
