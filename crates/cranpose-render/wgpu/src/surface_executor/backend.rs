use std::{ops::Range, rc::Rc};

use cranpose_core::NodeId;
use cranpose_render_common::{graph::LayerNode, raster_cache::LayerRasterCacheKey};
use cranpose_ui_graphics::{BlendMode, Brush, LayerShape, Rect, RenderEffect, RuntimeShader};

use crate::{
    effect_renderer::{
        CompositeBatchItem, CompositeSampleMode, FusedCompositeItem, ProjectiveSurfaceComposite,
        RoundedCompositeMask, ShaderCompositeBatchItem,
    },
    offscreen::OffscreenTarget,
    scene::{
        BackdropLayer, DrawOp, DrawShape, EffectLayer, ImageDraw, RetainedDraw, ShadowDraw,
        TextDraw,
    },
    surface_requirements::SurfaceRequirementSet,
};

pub(crate) struct LayerSurface {
    pub(crate) target: LayerSurfaceTexture,
    pub(crate) logical_rect: Rect,
    pub(crate) composite_alpha: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) rounded_clip: Option<LayerSurfaceRoundedClip>,
    pub(crate) backdrop: Option<RenderEffect>,
    pub(crate) deferred_effect: Option<RenderEffect>,
    /// The owning layer's content bounds in `logical_rect`'s space. The
    /// deferred effect's shader geometry anchors to THIS rect — the surface
    /// itself is padded/clipped and must never define the effect geometry.
    pub(crate) effect_content_rect: Option<Rect>,
    pub(crate) sample_mode: CompositeSampleMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerSurfaceRoundedClip {
    pub(crate) rect: Rect,
    pub(crate) radii: [f32; 4],
}

impl LayerSurfaceRoundedClip {
    pub(crate) fn from_layer(layer: &LayerNode) -> Option<Self> {
        if !layer.graphics_layer.clip {
            return None;
        }
        let LayerShape::Rounded(shape) = layer.graphics_layer.shape else {
            return None;
        };
        let radii = shape.resolve(layer.local_bounds.width, layer.local_bounds.height);
        if radii.top_left <= f32::EPSILON
            && radii.top_right <= f32::EPSILON
            && radii.bottom_left <= f32::EPSILON
            && radii.bottom_right <= f32::EPSILON
        {
            return None;
        }
        Some(Self {
            rect: layer.local_bounds,
            radii: [
                radii.top_left,
                radii.top_right,
                radii.bottom_left,
                radii.bottom_right,
            ],
        })
    }

    pub(crate) fn composite_mask(
        self,
        surface_logical_rect: Rect,
        dest_rect: Rect,
    ) -> RoundedCompositeMask {
        let scale_x = if surface_logical_rect.width.abs() > f32::EPSILON {
            dest_rect.width / surface_logical_rect.width
        } else {
            1.0
        };
        let scale_y = if surface_logical_rect.height.abs() > f32::EPSILON {
            dest_rect.height / surface_logical_rect.height
        } else {
            1.0
        };
        let mask_x = dest_rect.x + (self.rect.x - surface_logical_rect.x) * scale_x;
        let mask_y = dest_rect.y + (self.rect.y - surface_logical_rect.y) * scale_y;
        let mask_width = self.rect.width * scale_x;
        let mask_height = self.rect.height * scale_y;
        let radius_scale = scale_x.abs().min(scale_y.abs());
        RoundedCompositeMask {
            rect: [mask_x, mask_y, mask_width, mask_height],
            radii: [
                self.radii[0] * radius_scale,
                self.radii[1] * radius_scale,
                self.radii[2] * radius_scale,
                self.radii[3] * radius_scale,
            ],
        }
    }
}

pub(crate) enum LayerSurfaceTexture {
    Owned(OffscreenTarget),
    Cached(Rc<OffscreenTarget>),
}

impl LayerSurfaceTexture {
    pub(crate) fn target(&self) -> &OffscreenTarget {
        match self {
            Self::Owned(target) => target,
            Self::Cached(target) => target.as_ref(),
        }
    }
}

pub(crate) struct CachedLayerSurface {
    pub(crate) target: Rc<OffscreenTarget>,
    pub(crate) logical_rect: Rect,
    pub(crate) byte_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DevicePixelBounds {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) trait SurfaceExecutionBackend {
    fn max_texture_dim(&self) -> u32;
    fn acquire_retained_surface(&mut self, width: u32, height: u32) -> OffscreenTarget;
    fn acquire_frame_surface(&mut self, width: u32, height: u32) -> OffscreenTarget;
    fn release_frame_surface(&mut self, target: OffscreenTarget);
    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture);
    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)>;
    fn admit_layer_surface_cache_miss(&mut self, key: &LayerRasterCacheKey) -> bool;
    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget>;
    fn clear_target_view_with_load_op(
        &mut self,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    );
    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment(
        &mut self,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        brushes: &[Brush],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        draw_ops: &[DrawOp],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String>;
    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment_with_composites(
        &mut self,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        brushes: &[Brush],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        draw_ops: &[DrawOp],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        composites: &[(usize, usize, CompositeBatchItem<'_>)],
        shader_composites: &[(usize, usize, ShaderCompositeBatchItem<'_>)],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String>;
    #[allow(clippy::too_many_arguments)]
    fn render_range_with_layer_events_to_target(
        &mut self,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        brushes: &[Brush],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        retained_draws: &[RetainedDraw],
        draw_ops: &[DrawOp],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        backdrop_input_hashes: &[u64],
        z_start: usize,
        z_end: usize,
        excluded_effect_layer: Option<usize>,
        width: u32,
        height: u32,
        root_scale: f32,
        backdrop_underlay: Option<&OffscreenTarget>,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String>;
    fn render_shadow_draw(
        &mut self,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    );
    #[allow(clippy::too_many_arguments)]
    fn composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    );
    fn composite_projective_surfaces_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        composites: &[ProjectiveSurfaceComposite<'_>],
    );
    fn composite_surface_batch_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        composites: &[CompositeBatchItem<'_>],
    );
    fn copy_texture_region_to_target(
        &mut self,
        source: &OffscreenTarget,
        source_origin: (u32, u32),
        target: &OffscreenTarget,
        size: (u32, u32),
    ) -> bool;
    fn shader_composite_batch_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        composites: &[ShaderCompositeBatchItem<'_>],
    ) -> bool;
    /// Draw blits and runtime-shader composites interleaved in the items'
    /// own order inside ONE render pass. Returns false when a shader
    /// pipeline fails validation; nothing is drawn and the caller flushes
    /// the items individually instead.
    fn fused_composite_batch_to_view(
        &mut self,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        items: &[FusedCompositeItem<'_>],
    ) -> bool;
    #[allow(clippy::too_many_arguments)]
    fn composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    );
    #[allow(clippy::too_many_arguments)]
    fn apply_effect_and_composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String>;
    /// Encode `effect` from `source` straight into `target`'s view — the
    /// materialize shape: opaque SrcOver onto a fully-owned target with a
    /// 1:1 texel mapping. Returns Ok(false) when the backend cannot encode
    /// directly (e.g. the sizes differ); the caller then composites through
    /// the generic scratch route.
    fn materialize_effect_direct(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        target: &OffscreenTarget,
    ) -> Result<bool, String>;
    #[allow(clippy::too_many_arguments)]
    fn apply_shader_and_composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    );
    #[allow(clippy::too_many_arguments)]
    fn apply_shader_and_composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        shader: &RuntimeShader,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    );
    #[allow(clippy::too_many_arguments)]
    fn apply_effect_and_composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String>;
    fn is_render_effect_supported(&self, effect: &RenderEffect) -> bool;
    fn warn_unsupported_effect_once(&self);
    fn record_layer_cache_miss(&self, key: &LayerRasterCacheKey, width: u32, height: u32);
    fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
        requirements: SurfaceRequirementSet,
    );
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::Rect;

    use super::LayerSurfaceRoundedClip;

    #[test]
    fn rounded_clip_mask_maps_surface_subrect_to_destination_pixels() {
        let clip = LayerSurfaceRoundedClip {
            rect: Rect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
            },
            radii: [4.0, 8.0, 12.0, 16.0],
        };

        let mask = clip.composite_mask(
            Rect {
                x: 0.0,
                y: 10.0,
                width: 200.0,
                height: 100.0,
            },
            Rect {
                x: 40.0,
                y: 80.0,
                width: 400.0,
                height: 200.0,
            },
        );

        assert_eq!(mask.rect, [60.0, 100.0, 200.0, 80.0]);
        assert_eq!(mask.radii, [8.0, 16.0, 24.0, 32.0]);
    }
}
