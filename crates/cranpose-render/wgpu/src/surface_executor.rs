use crate::effect_renderer::{CompositeSampleMode, RoundedCompositeMask};
use crate::normalized_scene::{
    build_scene_window, collected_layer_bounds, filtered_effect_layer_index,
    resolved_child_surface_composite, resolved_layer_surface_rect, CollectedLayer,
    SceneWindowSource, TranslateBy,
};
use crate::offscreen::OffscreenTarget;
use crate::render::{
    has_backdrop_layer_in_range, scissor_rect_for_rect, CLEAR_COLOR, MAX_LAYER_SURFACE_CACHE_BYTES,
};
use crate::scene::{
    BackdropLayer, DrawShape, EffectLayer, ImageDraw, ShadowDraw, SnapAnchor, TextDraw,
};
use crate::surface_plan::{
    composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
    effect_layer_minimum_scale, effect_layer_target_scale, layer_surface_target_scale,
    LayerSurfaceRenderOptions, LayerSurfaceRequest, LayerSurfaceRequirements,
    TranslationRenderContext,
};
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use crate::TextSystemState;
use cranpose_core::NodeId;
use cranpose_render_common::graph::{LayerNode, ProjectiveTransform};
use cranpose_render_common::layer_composition::effective_layer_isolation;
use cranpose_render_common::primitive_emit::resolve_clip;
use cranpose_render_common::raster_cache::LayerRasterCacheKey;
use cranpose_ui_graphics::{BlendMode, Rect, RenderEffect};
use std::ops::Range;
use std::rc::Rc;

const MAX_EFFECT_LAYER_SURFACE_BYTES: u64 = 4 * 1024 * 1024;
const COMPOSITE_DEST_SNAP_TOLERANCE: f32 = 1e-4;

pub(crate) struct LayerSurface {
    pub(crate) target: LayerSurfaceTexture,
    pub(crate) logical_rect: Rect,
    pub(crate) composite_alpha: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) backdrop: Option<RenderEffect>,
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

#[allow(clippy::too_many_arguments)]
fn copy_surface_region_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    source_rect: Rect,
    dest_view: &wgpu::TextureView,
    dest_width: u32,
    dest_height: u32,
    root_scale: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
) -> Result<(), String> {
    let source_pixel_rect = surface_pixel_rect(source_rect, root_scale);
    let dest_quad = target_quad(dest_width, dest_height);
    let inverse = ProjectiveTransform::from_rect_to_quad(source_pixel_rect, dest_quad)
        .inverse()
        .ok_or_else(|| "surface region transform is not invertible".to_string())?;
    backend.composite_to_view_projective(
        source,
        dest_view,
        (dest_width, dest_height),
        (source.width as f32, source.height as f32),
        inverse.matrix(),
        dest_quad,
        1.0,
        load_op,
        None,
        BlendMode::SrcOver,
        CompositeSampleMode::Linear,
    );
    Ok(())
}

fn create_projected_child_underlay<B: SurfaceExecutionBackend>(
    backend: &mut B,
    parent_target: &OffscreenTarget,
    parent_underlay: Option<&OffscreenTarget>,
    child_logical_rect: Rect,
    child_dest_quad: [[f32; 2]; 4],
    root_scale: f32,
) -> OffscreenTarget {
    let (width, height) =
        surface_target_size(child_logical_rect, root_scale, backend.max_texture_dim());
    let underlay = backend.acquire_offscreen(width, height);
    let child_source_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let transform = ProjectiveTransform::from_rect_to_quad(
        child_source_rect,
        scaled_quad(child_dest_quad, root_scale),
    );
    let dest_quad = target_quad(width, height);
    let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

    if let Some(ancestor_underlay) = parent_underlay {
        backend.composite_to_view_projective(
            ancestor_underlay,
            &underlay.view,
            (width, height),
            (
                ancestor_underlay.width as f32,
                ancestor_underlay.height as f32,
            ),
            transform.matrix(),
            dest_quad,
            1.0,
            next_load_op,
            None,
            BlendMode::SrcOver,
            CompositeSampleMode::Linear,
        );
        next_load_op = wgpu::LoadOp::Load;
    }

    backend.composite_to_view_projective(
        parent_target,
        &underlay.view,
        (width, height),
        (parent_target.width as f32, parent_target.height as f32),
        transform.matrix(),
        dest_quad,
        1.0,
        next_load_op,
        None,
        BlendMode::SrcOver,
        CompositeSampleMode::Linear,
    );

    underlay
}

pub(crate) fn render_root_direct<B: SurfaceExecutionBackend>(
    backend: &mut B,
    text_state: &mut TextSystemState,
    surface_view: &wgpu::TextureView,
    collected: CollectedLayer<'_>,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Result<(), String> {
    let CollectedLayer {
        scene: local_scene,
        child_layers,
    } = collected;

    let mut cursor_z = 0usize;
    let mut next_load_op = wgpu::LoadOp::Clear(CLEAR_COLOR);
    for child in child_layers {
        if cursor_z < child.z_index {
            backend.render_non_effect_segment(
                text_state,
                surface_view,
                &local_scene.shapes,
                &local_scene.images,
                &local_scene.texts,
                &local_scene.shadow_draws,
                cursor_z,
                child.z_index,
                &[],
                width,
                height,
                root_scale,
                next_load_op,
            )?;
            next_load_op = wgpu::LoadOp::Load;
        } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(surface_view, next_load_op);
            next_load_op = wgpu::LoadOp::Load;
        }

        let resolved_child = resolved_child_surface_composite(&child);

        for shadow in &resolved_child.shadow_draws {
            backend.render_shadow_draw(text_state, surface_view, shadow, width, height, root_scale);
        }

        let child_surface = render_layer_surface(
            backend,
            text_state,
            child.layer,
            LayerSurfaceRequest {
                root_scale,
                backdrop_underlay: None,
                allow_runtime_cache: true,
                logical_rect_override: Some(resolved_child.logical_rect),
                translation_context: TranslationRenderContext::default(),
            },
        )?;
        if child_surface.backdrop.is_some() {
            return Err("root direct path does not support backdrop child surfaces".to_string());
        }

        let dest_quad = snap_motion_stable_dest_quad(
            scaled_quad(resolved_child.dest_quad, root_scale),
            child_surface.sample_mode,
        );
        composite_surface_to_view(
            backend,
            child_surface.target.target(),
            surface_view,
            (width, height),
            dest_quad,
            child_surface.composite_alpha,
            wgpu::LoadOp::Load,
            child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height)),
            child_surface.blend_mode,
            child_surface.sample_mode,
        )?;
        backend.release_layer_surface_target(child_surface.target);
        cursor_z = child.z_index.saturating_add(1);
    }

    if cursor_z < local_scene.next_z {
        backend.render_non_effect_segment(
            text_state,
            surface_view,
            &local_scene.shapes,
            &local_scene.images,
            &local_scene.texts,
            &local_scene.shadow_draws,
            cursor_z,
            local_scene.next_z,
            &[],
            width,
            height,
            root_scale,
            next_load_op,
        )?;
    } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(surface_view, next_load_op);
    }

    Ok(())
}

pub(crate) fn render_layer_surface<B: SurfaceExecutionBackend>(
    backend: &mut B,
    text_state: &mut TextSystemState,
    layer: &LayerNode,
    request: LayerSurfaceRequest<'_>,
) -> Result<LayerSurface, String> {
    let LayerSurfaceRequest {
        root_scale,
        backdrop_underlay,
        allow_runtime_cache,
        logical_rect_override,
        translation_context,
    } = request;
    let surface_requirements = backend.layer_surface_requirements(layer);
    let effective_translated_content_context =
        translation_context.inherited_content_translation || layer.translated_content_context;
    let composite_sample_mode = composite_sample_mode_for_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let target_scale = layer_surface_target_scale(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
        root_scale,
    );
    let translation_context = TranslationRenderContext {
        inherited_content_translation: translation_context.inherited_content_translation,
        surface_capture_active: translation_context.surface_capture_active
            || composite_sample_mode == CompositeSampleMode::Box4,
    };
    let cache_candidate = backend.layer_raster_cache_candidate(
        layer,
        target_scale,
        backdrop_underlay.is_some(),
        allow_runtime_cache,
        logical_rect_override,
    );
    if let Some((cache_key, logical_rect)) = cache_candidate {
        if let Some((target, logical_rect)) = backend.cached_layer_surface(&cache_key) {
            let isolation = effective_layer_isolation(&layer.graphics_layer);
            return Ok(LayerSurface {
                target: LayerSurfaceTexture::Cached(target),
                logical_rect,
                composite_alpha: isolation
                    .as_ref()
                    .map(|params| params.composite_alpha)
                    .unwrap_or(1.0),
                blend_mode: isolation
                    .as_ref()
                    .map(|params| params.blend_mode)
                    .unwrap_or(BlendMode::SrcOver),
                backdrop: layer.backdrop().cloned(),
                sample_mode: composite_sample_mode,
            });
        }
        let (width, height) = cache_key.pixel_size();
        backend.record_layer_cache_miss(width, height);
        return render_layer_surface_uncached(
            backend,
            text_state,
            layer,
            LayerSurfaceRenderOptions {
                target_scale,
                backdrop_underlay,
                cache_candidate: Some((cache_key, logical_rect)),
                logical_rect_override,
                composite_sample_mode,
                translation_context,
            },
        );
    }

    render_layer_surface_uncached(
        backend,
        text_state,
        layer,
        LayerSurfaceRenderOptions {
            target_scale,
            backdrop_underlay,
            cache_candidate: None,
            logical_rect_override,
            composite_sample_mode,
            translation_context,
        },
    )
}

fn render_layer_surface_uncached<B: SurfaceExecutionBackend>(
    backend: &mut B,
    text_state: &mut TextSystemState,
    layer: &LayerNode,
    options: LayerSurfaceRenderOptions<'_>,
) -> Result<LayerSurface, String> {
    let LayerSurfaceRenderOptions {
        target_scale,
        backdrop_underlay,
        cache_candidate,
        logical_rect_override,
        composite_sample_mode,
        translation_context,
    } = options;
    let isolation = effective_layer_isolation(&layer.graphics_layer);
    let CollectedLayer {
        scene: mut local_scene,
        child_layers,
    } = backend.collect_layer_contents_with_translation_context(
        layer,
        None,
        None,
        translation_context,
    );
    let effective_translated_content_context =
        translation_context.inherited_content_translation || layer.translated_content_context;
    let surface_requirements = backend.layer_surface_requirements(layer);

    let capture_full_bounds = surface_requirements
        .surface_requirements
        .contains(SurfaceRequirement::MotionStableCapture)
        && layer.effect().is_none()
        && layer.backdrop().is_none();
    let surface_rect = cache_candidate
        .map(|(_, logical_rect)| logical_rect)
        .or(logical_rect_override)
        .unwrap_or_else(|| {
            let bounds = collected_layer_bounds(&local_scene, &child_layers, !capture_full_bounds);
            resolved_layer_surface_rect(layer, bounds)
        });
    let max_dim = backend.max_texture_dim() as f32;
    let target_scale = target_scale
        .min(max_dim / surface_rect.width.max(1.0))
        .min(max_dim / surface_rect.height.max(1.0));
    let target_scale = quantize_motion_stable_target_scale(target_scale, composite_sample_mode);
    let shift = cranpose_ui_graphics::Point {
        x: -surface_rect.x,
        y: -surface_rect.y,
    };
    local_scene.translate_by(shift);

    let mut child_layers = child_layers;
    for child in &mut child_layers {
        for point in &mut child.dest_quad {
            point[0] += shift.x;
            point[1] += shift.y;
        }
        child.backdrop_rect.x += shift.x;
        child.backdrop_rect.y += shift.y;
        if let Some(clip) = child.visual_clip.as_mut() {
            clip.x += shift.x;
            clip.y += shift.y;
        }
        child.shadow_draws.translate_by(shift);
    }

    let (width, height) =
        surface_target_size(surface_rect, target_scale, backend.max_texture_dim());
    backend.record_isolated_layer_render(
        width,
        height,
        layer.node_id,
        surface_rect,
        surface_requirements.surface_requirements,
    );
    let target = backend.acquire_offscreen(width, height);

    let mut cursor_z = 0usize;
    let mut next_load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
    for child in child_layers {
        if cursor_z < child.z_index {
            backend.render_range_with_layer_events_to_target(
                text_state,
                &target,
                &local_scene.shapes,
                &local_scene.images,
                &local_scene.texts,
                &local_scene.shadow_draws,
                &local_scene.effect_layers,
                &local_scene.backdrop_layers,
                cursor_z,
                child.z_index,
                None,
                width,
                height,
                target_scale,
                backdrop_underlay,
                next_load_op,
            )?;
            next_load_op = wgpu::LoadOp::Load;
        } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
            backend.clear_target_view_with_load_op(&target.view, next_load_op);
            next_load_op = wgpu::LoadOp::Load;
        }

        let resolved_child = resolved_child_surface_composite(&child);
        let child_underlay = child.needs_nested_underlay.then(|| {
            create_projected_child_underlay(
                backend,
                &target,
                backdrop_underlay,
                resolved_child.logical_rect,
                resolved_child.dest_quad,
                target_scale,
            )
        });
        let child_surface = render_layer_surface(
            backend,
            text_state,
            child.layer,
            LayerSurfaceRequest {
                root_scale: target_scale,
                backdrop_underlay: child_underlay.as_ref(),
                allow_runtime_cache: true,
                logical_rect_override: Some(resolved_child.logical_rect),
                translation_context: TranslationRenderContext {
                    inherited_content_translation: effective_translated_content_context,
                    surface_capture_active: translation_context.surface_capture_active,
                },
            },
        )?;

        if let Some(backdrop) = &child_surface.backdrop {
            apply_backdrop_layer_to_target(
                backend,
                &target,
                &BackdropLayer {
                    rect: resolved_child.backdrop_rect,
                    clip: child.visual_clip,
                    effect: backdrop.clone(),
                    z_index: child.z_index,
                },
                backdrop_underlay,
                width,
                height,
                target_scale,
            )?;
        }

        for shadow in &resolved_child.shadow_draws {
            backend.render_shadow_draw(
                text_state,
                &target.view,
                shadow,
                width,
                height,
                target_scale,
            );
        }

        let dest_quad = snap_motion_stable_dest_quad(
            scaled_quad(resolved_child.dest_quad, target_scale),
            child_surface.sample_mode,
        );
        composite_surface_to_view(
            backend,
            child_surface.target.target(),
            &target.view,
            (width, height),
            dest_quad,
            child_surface.composite_alpha,
            wgpu::LoadOp::Load,
            child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, target_scale, width, height)),
            child_surface.blend_mode,
            child_surface.sample_mode,
        )?;
        backend.release_layer_surface_target(child_surface.target);
        if let Some(underlay) = child_underlay {
            backend.release_offscreen(underlay);
        }
        cursor_z = child.z_index.saturating_add(1);
    }

    if cursor_z < local_scene.next_z {
        backend.render_range_with_layer_events_to_target(
            text_state,
            &target,
            &local_scene.shapes,
            &local_scene.images,
            &local_scene.texts,
            &local_scene.shadow_draws,
            &local_scene.effect_layers,
            &local_scene.backdrop_layers,
            cursor_z,
            local_scene.next_z,
            None,
            width,
            height,
            target_scale,
            backdrop_underlay,
            next_load_op,
        )?;
    } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
        backend.clear_target_view_with_load_op(&target.view, next_load_op);
    }

    let mut target = target;
    if let Some(effect) = isolation.as_ref().and_then(|params| params.effect.as_ref()) {
        if backend.is_render_effect_supported(effect) {
            let effected = backend.acquire_offscreen(width, height);
            backend.apply_effect(
                &target,
                &effected.view,
                effect,
                local_effect_pixel_rect(width, height),
            );
            backend.release_offscreen(target);
            target = effected;
        } else {
            backend.warn_unsupported_effect_once();
        }
    }

    if let Some((cache_key, logical_rect)) = cache_candidate {
        if offscreen_byte_size(target.width, target.height) <= MAX_LAYER_SURFACE_CACHE_BYTES {
            let cached_target =
                backend.insert_cached_layer_surface(cache_key, target, logical_rect);
            return Ok(LayerSurface {
                target: LayerSurfaceTexture::Cached(cached_target),
                logical_rect,
                composite_alpha: isolation
                    .as_ref()
                    .map(|params| params.composite_alpha)
                    .unwrap_or(1.0),
                blend_mode: isolation
                    .as_ref()
                    .map(|params| params.blend_mode)
                    .unwrap_or(BlendMode::SrcOver),
                backdrop: layer.backdrop().cloned(),
                sample_mode: composite_sample_mode,
            });
        }
    }

    Ok(LayerSurface {
        target: LayerSurfaceTexture::Owned(target),
        logical_rect: surface_rect,
        composite_alpha: isolation
            .as_ref()
            .map(|params| params.composite_alpha)
            .unwrap_or(1.0),
        blend_mode: isolation
            .as_ref()
            .map(|params| params.blend_mode)
            .unwrap_or(BlendMode::SrcOver),
        backdrop: layer.backdrop().cloned(),
        sample_mode: composite_sample_mode,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_effect_layer_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    text_state: &mut TextSystemState,
    target: &OffscreenTarget,
    shapes: &[DrawShape],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    effect_layer_index: usize,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Result<(), String> {
    let layer = effect_layers
        .get(effect_layer_index)
        .cloned()
        .ok_or_else(|| "effect layer index out of bounds".to_string())?;
    let Some(visible_rect) = visible_layer_rect(layer.rect, layer.clip, root_scale, width, height)
    else {
        return Ok(());
    };
    let Some(scissor) = scissor_rect_for_rect(visible_rect, root_scale, width, height) else {
        return Ok(());
    };
    let sample_mode = composite_sample_mode_for_effect_layer(&layer);
    let stable_local_capture = sample_mode == CompositeSampleMode::Box4
        && (layer.effect.is_none()
            || layer
                .requirements
                .contains(SurfaceRequirement::TextMaterialMask))
        && !has_backdrop_layer_in_range(backdrop_layers, layer.z_start, layer.z_end)
        && backdrop_underlay.is_none()
        && layer
            .requirements
            .contains(SurfaceRequirement::MotionStableCapture);
    let capture_rect = if stable_local_capture {
        layer.rect
    } else {
        visible_rect
    };
    let effect_root_scale = clamp_effect_surface_scale(
        capture_rect,
        effect_layer_minimum_scale(&layer, root_scale),
        effect_layer_target_scale(&layer, root_scale),
        backend.max_texture_dim(),
    );
    let effect_root_scale = quantize_motion_stable_target_scale(effect_root_scale, sample_mode);
    let (effect_width, effect_height) =
        surface_target_size(capture_rect, effect_root_scale, backend.max_texture_dim());
    let window_scene = build_scene_window(
        SceneWindowSource {
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
        },
        layer.z_start,
        layer.z_end,
        capture_rect,
    );
    let Some(window_effect_index) = filtered_effect_layer_index(
        effect_layers,
        effect_layer_index,
        layer.z_start,
        layer.z_end,
    ) else {
        return Err("effect layer window index is missing".to_string());
    };

    let source = backend.acquire_offscreen(effect_width, effect_height);
    let has_nested_backdrop =
        has_backdrop_layer_in_range(&window_scene.backdrop_layers, layer.z_start, layer.z_end);
    let layer_underlay = if has_nested_backdrop {
        let underlay = backend.acquire_offscreen(effect_width, effect_height);

        if let Some(existing_underlay) = backdrop_underlay {
            copy_surface_region_to_view(
                backend,
                existing_underlay,
                visible_rect,
                &underlay.view,
                effect_width,
                effect_height,
                effect_root_scale,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            )?;
            copy_surface_region_to_view(
                backend,
                target,
                visible_rect,
                &underlay.view,
                effect_width,
                effect_height,
                effect_root_scale,
                wgpu::LoadOp::Load,
            )?;
        } else {
            copy_surface_region_to_view(
                backend,
                target,
                visible_rect,
                &underlay.view,
                effect_width,
                effect_height,
                effect_root_scale,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            )?;
        }
        Some(underlay)
    } else {
        None
    };

    let render_result = backend.render_range_with_layer_events_to_target(
        text_state,
        &source,
        &window_scene.shapes,
        &window_scene.images,
        &window_scene.texts,
        &window_scene.shadow_draws,
        &window_scene.effect_layers,
        &window_scene.backdrop_layers,
        layer.z_start,
        layer.z_end,
        Some(window_effect_index),
        effect_width,
        effect_height,
        effect_root_scale,
        layer_underlay.as_ref(),
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    );

    if let Some(underlay) = layer_underlay {
        backend.release_offscreen(underlay);
    }

    render_result?;

    let dest = backend.acquire_offscreen(effect_width, effect_height);
    if let Some(effect) = &layer.effect {
        if backend.is_render_effect_supported(effect) {
            backend.apply_effect(
                &source,
                &dest.view,
                effect,
                local_effect_pixel_rect(effect_width, effect_height),
            );
        } else {
            backend.warn_unsupported_effect_once();
            backend.composite_to_view(
                &source,
                &dest.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                CompositeSampleMode::Linear,
            );
        }
    } else {
        backend.composite_to_view(
            &source,
            &dest.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            CompositeSampleMode::Linear,
        );
    }

    let dest_quad = snap_motion_stable_dest_quad(
        scaled_quad(rect_to_quad(capture_rect), root_scale),
        sample_mode,
    );
    composite_surface_to_view(
        backend,
        &dest,
        &target.view,
        (width, height),
        dest_quad,
        layer.composite_alpha,
        wgpu::LoadOp::Load,
        Some(scissor),
        layer.blend_mode,
        sample_mode,
    )?;

    backend.release_offscreen(source);
    backend.release_offscreen(dest);
    Ok(())
}

pub(crate) fn apply_backdrop_layer_to_target<B: SurfaceExecutionBackend>(
    backend: &mut B,
    target: &OffscreenTarget,
    layer: &BackdropLayer,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Result<(), String> {
    let Some(visible_rect) = visible_layer_rect(layer.rect, layer.clip, root_scale, width, height)
    else {
        return Ok(());
    };
    let Some(scissor) = scissor_rect_for_rect(visible_rect, root_scale, width, height) else {
        return Ok(());
    };
    let backdrop_scale = clamp_effect_surface_scale(
        visible_rect,
        root_scale,
        root_scale,
        backend.max_texture_dim(),
    );
    let (backdrop_width, backdrop_height) =
        surface_target_size(visible_rect, backdrop_scale, backend.max_texture_dim());
    let snapshot = backend.acquire_offscreen(backdrop_width, backdrop_height);
    if let Some(underlay) = backdrop_underlay {
        copy_surface_region_to_view(
            backend,
            underlay,
            visible_rect,
            &snapshot.view,
            backdrop_width,
            backdrop_height,
            backdrop_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        )?;
        copy_surface_region_to_view(
            backend,
            target,
            visible_rect,
            &snapshot.view,
            backdrop_width,
            backdrop_height,
            backdrop_scale,
            wgpu::LoadOp::Load,
        )?;
    } else {
        copy_surface_region_to_view(
            backend,
            target,
            visible_rect,
            &snapshot.view,
            backdrop_width,
            backdrop_height,
            backdrop_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        )?;
    }

    let dest = backend.acquire_offscreen(backdrop_width, backdrop_height);
    if backend.is_render_effect_supported(&layer.effect) {
        backend.apply_effect(
            &snapshot,
            &dest.view,
            &layer.effect,
            local_effect_pixel_rect(backdrop_width, backdrop_height),
        );
    } else {
        backend.warn_unsupported_effect_once();
        backend.composite_to_view(
            &snapshot,
            &dest.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            CompositeSampleMode::Linear,
        );
    }

    backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
        &dest,
        &target.view,
        1.0,
        wgpu::LoadOp::Load,
        Some(scissor),
        None,
        BlendMode::SrcOver,
        Some((
            visible_rect.x * root_scale,
            visible_rect.y * root_scale,
            backdrop_width as f32,
            backdrop_height as f32,
        )),
        CompositeSampleMode::Linear,
    );

    backend.release_offscreen(snapshot);
    backend.release_offscreen(dest);
    Ok(())
}

pub(crate) fn surface_target_size(rect: Rect, root_scale: f32, max_dim: u32) -> (u32, u32) {
    (
        (rect.width * root_scale).ceil().clamp(1.0, max_dim as f32) as u32,
        (rect.height * root_scale).ceil().clamp(1.0, max_dim as f32) as u32,
    )
}

pub(crate) fn offscreen_byte_size(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * 4
}

pub(crate) fn surface_pixel_rect(rect: Rect, root_scale: f32) -> Rect {
    Rect {
        x: rect.x * root_scale,
        y: rect.y * root_scale,
        width: rect.width * root_scale,
        height: rect.height * root_scale,
    }
}

pub(crate) fn local_effect_pixel_rect(width: u32, height: u32) -> [f32; 4] {
    [0.0, 0.0, width as f32, height as f32]
}

pub(crate) fn visible_layer_rect(
    rect: Rect,
    clip: Option<Rect>,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<Rect> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let viewport_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32 / root_scale,
        height: height as f32 / root_scale,
    };
    let clipped_rect = resolve_clip(Some(viewport_rect), Some(rect))?;
    resolve_clip(Some(clipped_rect), clip)
}

pub(crate) fn clamp_effect_surface_scale(
    rect: Rect,
    minimum_scale: f32,
    desired_scale: f32,
    max_texture_dim: u32,
) -> f32 {
    let safe_minimum_scale = if minimum_scale.is_finite() && minimum_scale > 0.0 {
        minimum_scale
    } else {
        1.0
    };
    let mut scale = if desired_scale.is_finite() && desired_scale > 0.0 {
        desired_scale
    } else {
        safe_minimum_scale
    };

    scale = scale
        .min(max_texture_dim as f32 / rect.width.max(1.0))
        .min(max_texture_dim as f32 / rect.height.max(1.0));

    let area = rect.width.max(1.0) * rect.height.max(1.0);
    let max_scale_by_bytes = ((MAX_EFFECT_LAYER_SURFACE_BYTES as f32) / (area * 4.0)).sqrt();
    scale = scale.min(max_scale_by_bytes);

    scale.max(safe_minimum_scale)
}

pub(crate) fn device_pixel_bounds_for_rect(
    rect: Rect,
    viewport_width: u32,
    viewport_height: u32,
    root_scale: f32,
) -> Option<DevicePixelBounds> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let min_x = (rect.x * root_scale).floor().max(0.0);
    let min_y = (rect.y * root_scale).floor().max(0.0);
    let max_x = ((rect.x + rect.width) * root_scale)
        .ceil()
        .min(viewport_width as f32);
    let max_y = ((rect.y + rect.height) * root_scale)
        .ceil()
        .min(viewport_height as f32);
    let width = (max_x - min_x).max(0.0) as u32;
    let height = (max_y - min_y).max(0.0) as u32;
    if width == 0 || height == 0 {
        return None;
    }

    Some(DevicePixelBounds {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

pub(crate) fn target_quad(width: u32, height: u32) -> [[f32; 2]; 4] {
    [
        [0.0, 0.0],
        [width as f32, 0.0],
        [0.0, height as f32],
        [width as f32, height as f32],
    ]
}

pub(crate) fn scaled_quad(quad: [[f32; 2]; 4], scale: f32) -> [[f32; 2]; 4] {
    quad.map(|[x, y]| [x * scale, y * scale])
}

fn quad_is_axis_aligned_rect(quad: [[f32; 2]; 4]) -> bool {
    (quad[0][1] - quad[1][1]).abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && (quad[2][1] - quad[3][1]).abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && (quad[0][0] - quad[2][0]).abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && (quad[1][0] - quad[3][0]).abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
}

pub(crate) fn axis_aligned_quad_rect(dest_quad: [[f32; 2]; 4]) -> Option<Rect> {
    if !quad_is_axis_aligned_rect(dest_quad) {
        return None;
    }

    let min_x = dest_quad[0][0].min(dest_quad[2][0]);
    let max_x = dest_quad[1][0].max(dest_quad[3][0]);
    let min_y = dest_quad[0][1].min(dest_quad[1][1]);
    let max_y = dest_quad[2][1].max(dest_quad[3][1]);

    if !min_x.is_finite()
        || !max_x.is_finite()
        || !min_y.is_finite()
        || !max_y.is_finite()
        || max_x <= min_x
        || max_y <= min_y
    {
        return None;
    }

    Some(Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

pub(crate) fn snap_motion_stable_dest_quad(
    dest_quad: [[f32; 2]; 4],
    sample_mode: CompositeSampleMode,
) -> [[f32; 2]; 4] {
    if sample_mode != CompositeSampleMode::Box4 || !quad_is_axis_aligned_rect(dest_quad) {
        return dest_quad;
    }

    let delta_x = dest_quad[0][0].round() - dest_quad[0][0];
    let delta_y = dest_quad[0][1].round() - dest_quad[0][1];
    if delta_x.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && delta_y.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
    {
        return dest_quad;
    }

    dest_quad.map(|[x, y]| [x + delta_x, y + delta_y])
}

fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
    [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
}

fn quantize_motion_stable_target_scale(target_scale: f32, sample_mode: CompositeSampleMode) -> f32 {
    if sample_mode != CompositeSampleMode::Box4 || !target_scale.is_finite() {
        return target_scale;
    }

    target_scale.floor().max(1.0)
}

#[allow(clippy::too_many_arguments)]
fn composite_surface_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    source: &OffscreenTarget,
    dest_view: &wgpu::TextureView,
    viewport: (u32, u32),
    dest_quad: [[f32; 2]; 4],
    alpha: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    blend_mode: BlendMode,
    sample_mode: CompositeSampleMode,
) -> Result<(), String> {
    if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
        backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
            source,
            dest_view,
            alpha,
            load_op,
            scissor,
            None,
            blend_mode,
            Some((dest_rect.x, dest_rect.y, dest_rect.width, dest_rect.height)),
            sample_mode,
        );
        return Ok(());
    }

    let source_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: source.width as f32,
        height: source.height as f32,
    };
    let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
        .inverse()
        .ok_or_else(|| "child layer transform is not invertible".to_string())?;
    backend.composite_to_view_projective(
        source,
        dest_view,
        viewport,
        (source_rect.width, source_rect.height),
        inverse.matrix(),
        dest_quad,
        alpha,
        load_op,
        scissor,
        blend_mode,
        sample_mode,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        axis_aligned_quad_rect, quantize_motion_stable_target_scale, snap_motion_stable_dest_quad,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use cranpose_ui_graphics::Rect;

    #[test]
    fn box4_motion_stable_dest_quad_snaps_axis_aligned_translation() {
        let quad = [[12.33, 8.66], [20.33, 8.66], [12.33, 18.66], [20.33, 18.66]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Box4),
            [[12.0, 9.0], [20.0, 9.0], [12.0, 19.0], [20.0, 19.0]]
        );
    }

    #[test]
    fn linear_dest_quad_preserves_fractional_translation() {
        let quad = [[12.33, 8.66], [20.33, 8.66], [12.33, 18.66], [20.33, 18.66]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Linear),
            quad
        );
    }

    #[test]
    fn box4_non_axis_aligned_quad_stays_unsnapped() {
        let quad = [[12.33, 8.66], [20.33, 9.16], [12.33, 18.66], [20.33, 19.16]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Box4),
            quad
        );
    }

    #[test]
    fn axis_aligned_quad_rect_returns_rect_for_cardinal_quad() {
        let quad = [[12.0, 9.0], [20.0, 9.0], [12.0, 19.0], [20.0, 19.0]];

        assert_eq!(
            axis_aligned_quad_rect(quad),
            Some(Rect {
                x: 12.0,
                y: 9.0,
                width: 8.0,
                height: 10.0,
            })
        );
    }

    #[test]
    fn axis_aligned_quad_rect_rejects_skewed_quad() {
        let quad = [[12.0, 9.0], [20.0, 9.5], [12.0, 19.0], [20.0, 19.0]];

        assert_eq!(axis_aligned_quad_rect(quad), None);
    }

    #[test]
    fn box4_target_scale_quantizes_to_integer_texel_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Box4),
            4.0
        );
    }

    #[test]
    fn linear_target_scale_preserves_fractional_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Linear),
            4.72
        );
    }
}
