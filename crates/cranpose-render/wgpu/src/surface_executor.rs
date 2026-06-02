mod backend;
mod geometry;
mod render_paths;

pub(crate) use backend::DevicePixelBounds;
pub(crate) use backend::{CachedLayerSurface, LayerSurfaceTexture, SurfaceExecutionBackend};
pub(crate) use geometry::{
    axis_aligned_quad_rect, device_pixel_bounds_for_rect, offscreen_byte_size, scaled_quad,
    snap_delta_for_anchor, snap_motion_stable_dest_quad, surface_target_size,
};
#[cfg(test)]
pub(crate) use geometry::{clamp_effect_surface_scale, visible_layer_rect};
pub(crate) use render_paths::{
    apply_backdrop_layer_to_target, composite_surface_to_view, render_effect_layer_to_target,
    render_layer_surface, render_root_direct,
};
