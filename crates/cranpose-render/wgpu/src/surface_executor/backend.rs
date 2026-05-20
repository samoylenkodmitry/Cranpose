use crate::effect_renderer::{CompositeSampleMode, RoundedCompositeMask};
use crate::normalized_scene::CollectedLayer;
use crate::offscreen::OffscreenTarget;
use crate::scene::{
    BackdropLayer, DrawOp, DrawShape, EffectLayer, ImageDraw, ShadowDraw, SnapAnchor, TextDraw,
};
use crate::surface_plan::{LayerSurfaceRequirements, TranslationRenderContext};
use crate::surface_requirements::SurfaceRequirementSet;
use crate::TextSystemState;
use cranpose_core::NodeId;
use cranpose_render_common::graph::LayerNode;
use cranpose_render_common::raster_cache::LayerRasterCacheKey;
use cranpose_ui_graphics::{BlendMode, Rect, RenderEffect, RuntimeShader};
use std::ops::Range;
use std::rc::Rc;

pub(crate) struct LayerSurface {
    pub(crate) target: LayerSurfaceTexture,
    pub(crate) logical_rect: Rect,
    pub(crate) composite_alpha: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) backdrop: Option<RenderEffect>,
    pub(crate) deferred_effect: Option<RenderEffect>,
    pub(crate) sample_mode: CompositeSampleMode,
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
    fn acquire_offscreen(&mut self, width: u32, height: u32) -> OffscreenTarget;
    fn release_offscreen(&mut self, target: OffscreenTarget);
    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture);
    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)>;
    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget>;
    fn layer_raster_cache_candidate(
        &mut self,
        layer: &LayerNode,
        root_scale: f32,
        has_backdrop_underlay: bool,
        allow_runtime_cache: bool,
        logical_rect_override: Option<Rect>,
    ) -> Option<(LayerRasterCacheKey, Rect)>;
    fn layer_surface_requirements(&mut self, layer: &LayerNode) -> LayerSurfaceRequirements;
    fn collect_layer_contents_with_translation_context<'a>(
        &mut self,
        layer: &'a LayerNode,
        inherited_clip: Option<Rect>,
        inherited_translated_snap_anchor: Option<SnapAnchor>,
        translation_context: TranslationRenderContext,
    ) -> CollectedLayer<'a>;
    fn clear_target_view_with_load_op(
        &self,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    );
    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment(
        &mut self,
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
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
    fn render_range_with_layer_events_to_target(
        &mut self,
        text_state: &mut TextSystemState,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        draw_ops: &[DrawOp],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
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
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    );
    fn composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        sample_mode: CompositeSampleMode,
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
    fn apply_effect(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
    );
    #[allow(clippy::too_many_arguments)]
    fn apply_shader_and_composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        intermediate: &OffscreenTarget,
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
    fn is_render_effect_supported(&self, effect: &RenderEffect) -> bool;
    fn warn_unsupported_effect_once(&self);
    fn record_layer_cache_miss(&self, width: u32, height: u32);
    fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
        requirements: SurfaceRequirementSet,
    );
}
