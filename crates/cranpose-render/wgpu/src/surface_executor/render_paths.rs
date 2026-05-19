use super::backend::{LayerSurface, LayerSurfaceTexture, SurfaceExecutionBackend};
use super::geometry::{
    axis_aligned_quad_rect, clamp_effect_surface_scale, fit_capture_rect_to_scale_budget_for_axes,
    local_effect_pixel_rect, offscreen_byte_size, quantize_motion_stable_target_scale, scaled_quad,
    snap_delta_for_anchor, snap_motion_stable_dest_quad, surface_pixel_rect, surface_target_size,
    target_quad, visible_layer_rect,
};
use crate::effect_renderer::CompositeSampleMode;
use crate::normalized_scene::{
    build_scene_window, collected_layer_bounds, filtered_effect_layer_index,
    motion_stable_capture_bounds, resolved_child_surface_composite, resolved_layer_surface_rect,
    translate_quad, visible_draw_rect, ChildLayerComposite, CollectedLayer, SceneWindowSource,
    TranslateBy,
};
use crate::offscreen::OffscreenTarget;
use crate::render::{
    has_backdrop_layer_in_range, scissor_rect_for_rect, CLEAR_COLOR, MAX_LAYER_SURFACE_CACHE_BYTES,
};
use crate::scene::{
    BackdropLayer, CompositorScene, DrawShape, EffectLayer, ImageDraw, ShadowDraw, SnapAnchor,
    TextDraw,
};
use crate::surface_plan::{
    composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
    effect_layer_minimum_scale, effect_layer_target_scale, effective_surface_requirements,
    layer_surface_target_scale, layer_uses_external_backdrop_input,
    translated_content_axes_for_layer, LayerSurfaceRenderOptions, LayerSurfaceRequest,
    LayerSurfaceRequirements, TranslatedContentAxes, TranslationRenderContext,
};
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use crate::TextSystemState;
use cranpose_render_common::graph::{CachePolicy, LayerNode, ProjectiveTransform};
use cranpose_render_common::layer_composition::effective_layer_isolation;
use cranpose_render_common::raster_cache::{LayerRasterCacheKey, ScaleBucket};
use cranpose_ui_graphics::{BlendMode, Rect, RenderEffect};

fn anchored_composite_dest_quad(
    dest_quad: [[f32; 2]; 4],
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
    sample_mode: CompositeSampleMode,
) -> [[f32; 2]; 4] {
    let scaled = if let Some(anchor) = snap_anchor {
        let snap_delta = snap_delta_for_anchor(anchor, root_scale);
        scaled_quad(translate_quad(dest_quad, snap_delta), root_scale)
    } else {
        scaled_quad(dest_quad, root_scale)
    };

    snap_motion_stable_dest_quad(scaled, sample_mode)
}

fn composite_dest_viewport(
    dest_rect: Rect,
    _source_width: u32,
    _source_height: u32,
    sample_mode: CompositeSampleMode,
) -> (f32, f32, f32, f32) {
    if sample_mode != CompositeSampleMode::Box4 {
        return (dest_rect.x, dest_rect.y, dest_rect.width, dest_rect.height);
    }

    (
        dest_rect.x.round(),
        dest_rect.y.round(),
        dest_rect.width,
        dest_rect.height,
    )
}

fn layer_surface_dest_quad(
    child_logical_rect: Rect,
    child_dest_quad: [[f32; 2]; 4],
    surface_logical_rect: Rect,
) -> [[f32; 2]; 4] {
    ProjectiveTransform::from_rect_to_quad(child_logical_rect, child_dest_quad)
        .map_rect(surface_logical_rect)
}

fn combined_capture_clip(layer_clip: Option<Rect>, capture_clip: Option<Rect>) -> Option<Rect> {
    match (layer_clip, capture_clip) {
        (Some(layer_clip), Some(capture_clip)) => layer_clip.intersect(capture_clip),
        (Some(clip), None) | (None, Some(clip)) => Some(clip),
        (None, None) => None,
    }
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
    let result = (|| -> Result<(), String> {
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
                backend.render_shadow_draw(
                    text_state,
                    surface_view,
                    shadow,
                    width,
                    height,
                    root_scale,
                );
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
                    capture_clip_override: resolved_child.surface_clip,
                    activates_nested_capture: true,
                    translation_context: TranslationRenderContext::default(),
                },
            )?;
            if child_surface.backdrop.is_some() {
                return Err("root direct path does not support backdrop child surfaces".to_string());
            }

            let dest_quad = layer_surface_dest_quad(
                resolved_child.logical_rect,
                resolved_child.dest_quad,
                child_surface.logical_rect,
            );
            let dest_quad = anchored_composite_dest_quad(
                dest_quad,
                resolved_child.snap_anchor,
                root_scale,
                child_surface.sample_mode,
            );
            composite_layer_surface_to_view(
                backend,
                &child_surface,
                surface_view,
                (width, height),
                dest_quad,
                wgpu::LoadOp::Load,
                child
                    .visual_clip
                    .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height)),
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
    })();
    drop(local_scene);
    result
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
        capture_clip_override,
        activates_nested_capture,
        translation_context,
    } = request;
    let surface_requirements = backend.layer_surface_requirements(layer);
    let direct_translated_content_context =
        translation_context.inherited_content_translation || layer.translated_content_context;
    let effective_translated_content_context =
        direct_translated_content_context || surface_requirements.contains_translated_content;
    let effective_requirements = effective_surface_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let composite_sample_mode = composite_sample_mode_for_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let target_scale = layer_surface_target_scale(
        direct_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
        root_scale,
    );
    let translation_context = layer_surface_translation_context(
        translation_context,
        activates_nested_capture
            && effective_requirements.contains(SurfaceRequirement::MotionStableCapture),
    );
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
                deferred_effect: None,
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
                allow_runtime_cache,
                cache_candidate: Some((cache_key, logical_rect)),
                logical_rect_override,
                capture_clip_override,
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
            allow_runtime_cache,
            cache_candidate: None,
            logical_rect_override,
            capture_clip_override,
            composite_sample_mode,
            translation_context,
        },
    )
}

fn layer_surface_translation_context(
    translation_context: TranslationRenderContext,
    surface_provides_motion_stable_capture: bool,
) -> TranslationRenderContext {
    TranslationRenderContext {
        inherited_content_translation: translation_context.inherited_content_translation,
        translated_content_axes: translation_context.translated_content_axes,
        surface_capture_active: translation_context.surface_capture_active
            || surface_provides_motion_stable_capture,
        local_picture_capture_active: translation_context.local_picture_capture_active,
    }
}

fn minimum_surface_scale_for_composite(
    target_scale: f32,
    sample_mode: CompositeSampleMode,
    effective_requirements: SurfaceRequirementSet,
) -> f32 {
    if sample_mode == CompositeSampleMode::Box4
        && !effective_requirements.contains(SurfaceRequirement::MotionStableCapture)
    {
        target_scale
    } else {
        target_scale.min(1.0)
    }
}

#[allow(clippy::too_many_arguments)]
fn runtime_effect_source_cache_key(
    layer: &LayerNode,
    surface_requirements: LayerSurfaceRequirements,
    surface_rect: Rect,
    pixel_size: (u32, u32),
    target_scale: f32,
    has_backdrop_underlay: bool,
    allow_runtime_cache: bool,
) -> Option<LayerRasterCacheKey> {
    if !layer
        .effect()
        .is_some_and(|effect| effect.contains_runtime_shader())
    {
        return None;
    }

    let cache_is_allowed = layer.cache_policy == CachePolicy::Auto
        || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface());
    if !cache_is_allowed || layer_uses_external_backdrop_input(layer, has_backdrop_underlay) {
        return None;
    }

    Some(LayerRasterCacheKey::source_content(
        layer.node_id,
        layer.target_content_hash(),
        surface_rect,
        pixel_size,
        ScaleBucket::from_scale(target_scale),
    ))
}

#[allow(clippy::too_many_arguments)]
fn render_layer_source_uncached<B: SurfaceExecutionBackend>(
    backend: &mut B,
    text_state: &mut TextSystemState,
    local_scene: &CompositorScene,
    child_layers: Vec<ChildLayerComposite<'_>>,
    target_scale: f32,
    backdrop_underlay: Option<&OffscreenTarget>,
    width: u32,
    height: u32,
    effective_translated_content_context: bool,
    effective_translated_content_axes: TranslatedContentAxes,
    translation_context: TranslationRenderContext,
) -> Result<OffscreenTarget, String> {
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
        let child_dest_quad = if let Some(anchor) = resolved_child.snap_anchor {
            translate_quad(
                resolved_child.dest_quad,
                snap_delta_for_anchor(anchor, target_scale),
            )
        } else {
            resolved_child.dest_quad
        };
        let child_underlay = child.needs_nested_underlay.then(|| {
            create_projected_child_underlay(
                backend,
                &target,
                backdrop_underlay,
                resolved_child.logical_rect,
                child_dest_quad,
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
                capture_clip_override: resolved_child.surface_clip,
                activates_nested_capture: true,
                translation_context: TranslationRenderContext {
                    inherited_content_translation: effective_translated_content_context,
                    translated_content_axes: effective_translated_content_axes,
                    surface_capture_active: translation_context.surface_capture_active,
                    local_picture_capture_active: translation_context.local_picture_capture_active,
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

        let dest_quad = layer_surface_dest_quad(
            resolved_child.logical_rect,
            resolved_child.dest_quad,
            child_surface.logical_rect,
        );
        let dest_quad = anchored_composite_dest_quad(
            dest_quad,
            resolved_child.snap_anchor,
            target_scale,
            child_surface.sample_mode,
        );
        composite_layer_surface_to_view(
            backend,
            &child_surface,
            &target.view,
            (width, height),
            dest_quad,
            wgpu::LoadOp::Load,
            child
                .visual_clip
                .and_then(|clip| scissor_rect_for_rect(clip, target_scale, width, height)),
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

    Ok(target)
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
        allow_runtime_cache,
        mut cache_candidate,
        logical_rect_override,
        capture_clip_override,
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
    let result = (|| -> Result<LayerSurface, String> {
        let surface_requirements = backend.layer_surface_requirements(layer);
        let effective_translated_content_context = translation_context
            .inherited_content_translation
            || layer.translated_content_context
            || surface_requirements.contains_translated_content;
        let effective_translated_content_axes = translation_context
            .translated_content_axes
            .union(translated_content_axes_for_layer(layer))
            .union(surface_requirements.translated_content_axes);
        let effective_requirements = effective_surface_requirements(
            effective_translated_content_context,
            translation_context.surface_capture_active,
            surface_requirements,
        );
        let capture_clip = combined_capture_clip(layer.clip_rect(), capture_clip_override);
        let estimated_surface_rect = cache_candidate
            .as_ref()
            .map(|(_, logical_rect)| *logical_rect)
            .or(logical_rect_override)
            .unwrap_or_else(|| {
                let bounds = motion_stable_capture_bounds(
                    layer,
                    &local_scene,
                    &child_layers,
                    effective_requirements,
                    effective_translated_content_axes,
                    capture_clip,
                );
                resolved_layer_surface_rect(layer, bounds)
            });
        let mut surface_rect =
            if effective_requirements.contains(SurfaceRequirement::MotionStableCapture) {
                let bounds = motion_stable_capture_bounds(
                    layer,
                    &local_scene,
                    &child_layers,
                    effective_requirements,
                    effective_translated_content_axes,
                    capture_clip,
                );
                resolved_layer_surface_rect(layer, bounds)
            } else {
                estimated_surface_rect
            };
        if cache_candidate
            .as_ref()
            .is_some_and(|(_, logical_rect)| *logical_rect != surface_rect)
        {
            cache_candidate = None;
        }
        let max_dim = backend.max_texture_dim() as f32;
        if effective_requirements.contains(SurfaceRequirement::MotionStableCapture) {
            if let Some(visible_bounds) = collected_layer_bounds(&local_scene, &child_layers, true)
                .and_then(|bounds| visible_draw_rect(bounds, capture_clip))
            {
                let required_rect = resolved_layer_surface_rect(layer, Some(visible_bounds));
                let desired_scale =
                    quantize_motion_stable_target_scale(target_scale, composite_sample_mode);
                surface_rect = fit_capture_rect_to_scale_budget_for_axes(
                    surface_rect,
                    required_rect,
                    desired_scale,
                    backend.max_texture_dim(),
                    effective_translated_content_axes,
                );
            }
        }
        let target_scale = target_scale
            .min(max_dim / surface_rect.width.max(1.0))
            .min(max_dim / surface_rect.height.max(1.0));
        let minimum_surface_scale = minimum_surface_scale_for_composite(
            target_scale,
            composite_sample_mode,
            effective_requirements,
        );
        let target_scale = clamp_effect_surface_scale(
            surface_rect,
            minimum_surface_scale,
            target_scale,
            backend.max_texture_dim(),
        );
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
            effective_requirements,
        );
        let source_cache_key = runtime_effect_source_cache_key(
            layer,
            surface_requirements,
            surface_rect,
            (width, height),
            target_scale,
            backdrop_underlay.is_some(),
            allow_runtime_cache,
        );
        let mut target = if let Some(cache_key) = source_cache_key {
            if let Some((cached_target, _)) = backend.cached_layer_surface(&cache_key) {
                LayerSurfaceTexture::Cached(cached_target)
            } else {
                backend.record_layer_cache_miss(width, height);
                let rendered = render_layer_source_uncached(
                    backend,
                    text_state,
                    &local_scene,
                    child_layers,
                    target_scale,
                    backdrop_underlay,
                    width,
                    height,
                    effective_translated_content_context,
                    effective_translated_content_axes,
                    translation_context,
                )?;
                if offscreen_byte_size(rendered.width, rendered.height)
                    <= MAX_LAYER_SURFACE_CACHE_BYTES
                {
                    let cached_target =
                        backend.insert_cached_layer_surface(cache_key, rendered, surface_rect);
                    LayerSurfaceTexture::Cached(cached_target)
                } else {
                    LayerSurfaceTexture::Owned(rendered)
                }
            }
        } else {
            LayerSurfaceTexture::Owned(render_layer_source_uncached(
                backend,
                text_state,
                &local_scene,
                child_layers,
                target_scale,
                backdrop_underlay,
                width,
                height,
                effective_translated_content_context,
                effective_translated_content_axes,
                translation_context,
            )?)
        };
        let mut deferred_effect = None;
        if let Some(effect) = isolation.as_ref().and_then(|params| params.effect.as_ref()) {
            if backend.is_render_effect_supported(effect) {
                if matches!(effect, RenderEffect::Shader { .. }) {
                    deferred_effect = Some(effect.clone());
                } else {
                    let effected = backend.acquire_offscreen(width, height);
                    backend.apply_effect(
                        target.target(),
                        &effected.view,
                        effect,
                        local_effect_pixel_rect(width, height),
                    );
                    backend.release_layer_surface_target(target);
                    target = LayerSurfaceTexture::Owned(effected);
                }
            } else {
                backend.warn_unsupported_effect_once();
            }
        }

        let composite_alpha = isolation
            .as_ref()
            .map(|params| params.composite_alpha)
            .unwrap_or(1.0);
        let blend_mode = isolation
            .as_ref()
            .map(|params| params.blend_mode)
            .unwrap_or(BlendMode::SrcOver);
        let backdrop = layer.backdrop().cloned();

        if let Some((cache_key, logical_rect)) = cache_candidate {
            if let LayerSurfaceTexture::Owned(owned_target) = target {
                if offscreen_byte_size(owned_target.width, owned_target.height)
                    <= MAX_LAYER_SURFACE_CACHE_BYTES
                {
                    let cached_target =
                        backend.insert_cached_layer_surface(cache_key, owned_target, logical_rect);
                    return Ok(LayerSurface {
                        target: LayerSurfaceTexture::Cached(cached_target),
                        logical_rect,
                        composite_alpha,
                        blend_mode,
                        backdrop,
                        deferred_effect: None,
                        sample_mode: composite_sample_mode,
                    });
                }
                target = LayerSurfaceTexture::Owned(owned_target);
            }
        }

        Ok(LayerSurface {
            target,
            logical_rect: surface_rect,
            composite_alpha,
            blend_mode,
            backdrop,
            deferred_effect,
            sample_mode: composite_sample_mode,
        })
    })();
    drop(local_scene);
    result
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
    let has_nested_backdrop =
        has_backdrop_layer_in_range(&window_scene.backdrop_layers, layer.z_start, layer.z_end);
    let Some(window_effect_index) = filtered_effect_layer_index(
        effect_layers,
        effect_layer_index,
        layer.z_start,
        layer.z_end,
    ) else {
        return Err("effect layer window index is missing".to_string());
    };

    let source = backend.acquire_offscreen(effect_width, effect_height);
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

    let dest_quad = anchored_composite_dest_quad(
        crate::rect_to_quad(capture_rect),
        layer.snap_anchor,
        root_scale,
        sample_mode,
    );
    if layer.effect.is_none() {
        let composite_result = composite_surface_to_view(
            backend,
            &source,
            &target.view,
            (width, height),
            dest_quad,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
            layer.blend_mode,
            sample_mode,
        );
        backend.release_offscreen(source);
        return composite_result;
    }

    let dest = backend.acquire_offscreen(effect_width, effect_height);
    let mut composited_effect_to_target = false;
    let Some(effect) = &layer.effect else {
        unreachable!("effect-free layers return before allocating a destination surface");
    };
    if backend.is_render_effect_supported(effect) {
        if let RenderEffect::Shader { shader } = effect {
            if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
                backend.apply_shader_and_composite_to_view(
                    &source,
                    &dest,
                    shader,
                    local_effect_pixel_rect(effect_width, effect_height),
                    &target.view,
                    layer.composite_alpha,
                    wgpu::LoadOp::Load,
                    Some(scissor),
                    layer.blend_mode,
                    Some(composite_dest_viewport(
                        dest_rect,
                        effect_width,
                        effect_height,
                        sample_mode,
                    )),
                    sample_mode,
                );
                composited_effect_to_target = true;
            } else {
                backend.apply_effect(
                    &source,
                    &dest.view,
                    effect,
                    local_effect_pixel_rect(effect_width, effect_height),
                );
            }
        } else {
            backend.apply_effect(
                &source,
                &dest.view,
                effect,
                local_effect_pixel_rect(effect_width, effect_height),
            );
        }
    } else {
        backend.warn_unsupported_effect_once();
        backend.composite_to_view(
            &source,
            &dest.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            CompositeSampleMode::Linear,
        );
    }

    if !composited_effect_to_target {
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
    }

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
            Some(composite_dest_viewport(
                dest_rect,
                source.width,
                source.height,
                sample_mode,
            )),
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

#[allow(clippy::too_many_arguments)]
fn composite_layer_surface_to_view<B: SurfaceExecutionBackend>(
    backend: &mut B,
    surface: &LayerSurface,
    dest_view: &wgpu::TextureView,
    viewport: (u32, u32),
    dest_quad: [[f32; 2]; 4],
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    let source = surface.target.target();
    if let Some(RenderEffect::Shader { shader }) = &surface.deferred_effect {
        if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
            let intermediate = backend.acquire_offscreen(source.width, source.height);
            backend.apply_shader_and_composite_to_view(
                source,
                &intermediate,
                shader,
                local_effect_pixel_rect(source.width, source.height),
                dest_view,
                surface.composite_alpha,
                load_op,
                scissor,
                surface.blend_mode,
                Some(composite_dest_viewport(
                    dest_rect,
                    source.width,
                    source.height,
                    surface.sample_mode,
                )),
                surface.sample_mode,
            );
            backend.release_offscreen(intermediate);
            return Ok(());
        }

        let intermediate = backend.acquire_offscreen(source.width, source.height);
        let effect = surface
            .deferred_effect
            .as_ref()
            .expect("deferred effect was just matched");
        backend.apply_effect(
            source,
            &intermediate.view,
            effect,
            local_effect_pixel_rect(source.width, source.height),
        );
        let result = composite_surface_to_view(
            backend,
            &intermediate,
            dest_view,
            viewport,
            dest_quad,
            surface.composite_alpha,
            load_op,
            scissor,
            surface.blend_mode,
            surface.sample_mode,
        );
        backend.release_offscreen(intermediate);
        return result;
    }

    composite_surface_to_view(
        backend,
        source,
        dest_view,
        viewport,
        dest_quad,
        surface.composite_alpha,
        load_op,
        scissor,
        surface.blend_mode,
        surface.sample_mode,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        anchored_composite_dest_quad, composite_dest_viewport, layer_surface_dest_quad,
        layer_surface_translation_context, minimum_surface_scale_for_composite,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::scene::SnapAnchor;
    use crate::surface_plan::TranslationRenderContext;
    use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
    use cranpose_ui_graphics::Point;
    use cranpose_ui_graphics::Rect;

    #[test]
    fn anchored_box4_composite_snaps_final_dest_quad() {
        let quad = [
            [0.0, -1953.0],
            [1119.0, -1953.0],
            [0.0, 808.0],
            [1119.0, 808.0],
        ];

        assert_eq!(
            anchored_composite_dest_quad(
                quad,
                Some(SnapAnchor::rigid(Point::new(0.0, 0.0))),
                1.25,
                CompositeSampleMode::Box4,
            ),
            [
                [0.0, -2441.0],
                [1398.75, -2441.0],
                [0.0, 1010.25],
                [1398.75, 1010.25],
            ]
        );
    }

    #[test]
    fn unanchored_box4_composite_snaps_final_dest_quad() {
        let quad = [[0.25, 10.5], [40.75, 10.5], [0.25, 20.25], [40.75, 20.25]];

        assert_eq!(
            anchored_composite_dest_quad(quad, None, 1.0, CompositeSampleMode::Box4),
            [[0.0, 11.0], [40.5, 11.0], [0.0, 20.75], [40.5, 20.75]],
            "unanchored pixel-stable surfaces should keep their composite phase snapped"
        );
    }

    #[test]
    fn box4_composite_viewport_snaps_origin_without_changing_geometry_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.0,
                y: -2441.0,
                width: 1398.75,
                height: 3451.25,
            },
            1399,
            3452,
            CompositeSampleMode::Box4,
        );

        assert_eq!(viewport, (0.0, -2441.0, 1398.75, 3451.25));
    }

    #[test]
    fn linear_composite_viewport_keeps_fractional_geometry_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.25,
                y: 10.5,
                width: 40.75,
                height: 20.25,
            },
            41,
            21,
            CompositeSampleMode::Linear,
        );

        assert_eq!(viewport, (0.25, 10.5, 40.75, 20.25));
    }

    #[test]
    fn box4_composite_viewport_keeps_scaled_geometry_extent() {
        let viewport = composite_dest_viewport(
            Rect {
                x: 0.25,
                y: 10.5,
                width: 140.0,
                height: 80.0,
            },
            70,
            40,
            CompositeSampleMode::Box4,
        );

        assert_eq!(viewport, (0.0, 11.0, 140.0, 80.0));
    }

    #[test]
    fn box4_non_capture_surface_keeps_device_scale_as_minimum() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                1.35,
                CompositeSampleMode::Box4,
                SurfaceRequirementSet::default().with(SurfaceRequirement::ExplicitOffscreen),
            ),
            1.35
        );
    }

    #[test]
    fn linear_surface_can_use_memory_budget_scale_floor() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                1.35,
                CompositeSampleMode::Linear,
                SurfaceRequirementSet::default()
                    .with(SurfaceRequirement::ExplicitOffscreen)
                    .with(SurfaceRequirement::NonTranslationTransform),
            ),
            1.0
        );
    }

    #[test]
    fn motion_stable_capture_keeps_its_existing_budget_scale_floor() {
        assert_eq!(
            minimum_surface_scale_for_composite(
                8.0,
                CompositeSampleMode::Box4,
                SurfaceRequirementSet::default().with(SurfaceRequirement::MotionStableCapture),
            ),
            1.0
        );
    }

    #[test]
    fn layer_surface_dest_quad_maps_actual_trimmed_surface_rect() {
        let child_rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        let child_quad = [
            [110.0, 220.0],
            [310.0, 220.0],
            [110.0, 620.0],
            [310.0, 620.0],
        ];
        let surface_rect = Rect {
            x: 10.0,
            y: 70.0,
            width: 100.0,
            height: 80.0,
        };

        assert_eq!(
            layer_surface_dest_quad(child_rect, child_quad, surface_rect),
            [
                [110.0, 320.0],
                [310.0, 320.0],
                [110.0, 480.0],
                [310.0, 480.0],
            ],
        );
    }

    #[test]
    fn layer_surface_context_marks_nested_motion_stable_capture_active() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            true,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn layer_surface_context_keeps_generic_parent_surface_inactive() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: true,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn layer_surface_context_keeps_existing_capture_active() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
                ..TranslationRenderContext::default()
            }
        );
    }

    #[test]
    fn layer_surface_context_keeps_root_viewport_uncaptured() {
        let context = layer_surface_translation_context(
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            },
            false,
        );

        assert_eq!(
            context,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: false,
                ..TranslationRenderContext::default()
            }
        );
    }
}
