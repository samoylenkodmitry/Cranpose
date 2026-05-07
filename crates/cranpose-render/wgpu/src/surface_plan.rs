use crate::effect_renderer::CompositeSampleMode;
use crate::offscreen::OffscreenTarget;
use crate::scene::EffectLayer;
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use cranpose_render_common::graph::{
    LayerNode, PrimitiveEntry, PrimitiveNode, ProjectiveTransform, RenderNode,
};
use cranpose_render_common::layer_composition::effective_layer_isolation;
use cranpose_ui_graphics::{BlendMode, Brush, CompositingStrategy, Point, Rect};

const SURFACE_PLAN_AFFINE_TOLERANCE: f32 = 1e-4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayerSurfaceRequirements {
    pub(crate) direct_translation: Option<Point>,
    pub(crate) surface_requirements: SurfaceRequirementSet,
    pub(crate) pixel_stable_composite: bool,
}

impl LayerSurfaceRequirements {
    pub(crate) fn has_isolating_requirement(self) -> bool {
        self.surface_requirements.has_isolating_requirement()
    }

    pub(crate) fn has_renderer_forced_surface(self) -> bool {
        self.surface_requirements.has_renderer_forced_surface()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TranslationRenderContext {
    pub(crate) inherited_content_translation: bool,
    pub(crate) surface_capture_active: bool,
}

pub(crate) struct LayerSurfaceRequest<'a> {
    pub(crate) root_scale: f32,
    pub(crate) backdrop_underlay: Option<&'a OffscreenTarget>,
    pub(crate) allow_runtime_cache: bool,
    pub(crate) logical_rect_override: Option<Rect>,
    pub(crate) activates_nested_capture: bool,
    pub(crate) translation_context: TranslationRenderContext,
}

pub(crate) struct LayerSurfaceRenderOptions<'a> {
    pub(crate) target_scale: f32,
    pub(crate) backdrop_underlay: Option<&'a OffscreenTarget>,
    pub(crate) allow_runtime_cache: bool,
    pub(crate) cache_candidate: Option<(
        cranpose_render_common::raster_cache::LayerRasterCacheKey,
        Rect,
    )>,
    pub(crate) logical_rect_override: Option<Rect>,
    pub(crate) composite_sample_mode: CompositeSampleMode,
    pub(crate) translation_context: TranslationRenderContext,
}

fn layer_contains_text_primitives(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| {
        matches!(
            child,
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Text(_),
                ..
            })
        )
    })
}

fn layer_contains_draw_primitives(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| {
        matches!(
            child,
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Draw(_),
                ..
            })
        )
    })
}

fn span_has_foreground_override(span_style: &cranpose_ui::text::SpanStyle) -> bool {
    matches!(
        span_style.brush.as_ref(),
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    ) || span_style.alpha.is_some()
        || span_style.draw_style.is_some()
}

fn text_primitive_uses_gpu_effect(text: &cranpose_render_common::graph::TextPrimitiveNode) -> bool {
    if text
        .text
        .span_styles
        .iter()
        .any(|span| span_has_foreground_override(&span.item))
    {
        return true;
    }

    matches!(
        text.text_style.span_style.brush,
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    ) || matches!(
        text.text_style.span_style.draw_style,
        Some(cranpose_ui::text::TextDrawStyle::Stroke { width })
            if width.is_finite() && width > 0.0
    )
}

fn text_primitive_needs_local_surface(
    text: &cranpose_render_common::graph::TextPrimitiveNode,
) -> bool {
    let span_style = &text.text_style.span_style;
    let has_non_identity_geometric_transform =
        span_style
            .text_geometric_transform
            .is_some_and(|transform| {
                (transform.scale_x - 1.0).abs() > SURFACE_PLAN_AFFINE_TOLERANCE
                    || transform.skew_x.abs() > SURFACE_PLAN_AFFINE_TOLERANCE
            });
    let has_baseline_shift = span_style
        .baseline_shift
        .is_some_and(|shift| shift.is_specified() && shift.0.abs() > SURFACE_PLAN_AFFINE_TOLERANCE);

    text_primitive_uses_gpu_effect(text)
        || span_style.shadow.is_some()
        || span_style.background.is_some()
        || has_baseline_shift
        || has_non_identity_geometric_transform
        || span_style.letter_spacing.is_specified()
}

fn layer_contains_gpu_effect_text_primitives(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| match child {
        RenderNode::Primitive(PrimitiveEntry {
            node: PrimitiveNode::Text(text),
            ..
        }) => text_primitive_uses_gpu_effect(text),
        _ => false,
    })
}

fn draw_primitive_is_pixel_sensitive(draw: &cranpose_ui_graphics::DrawPrimitive) -> bool {
    matches!(draw, cranpose_ui_graphics::DrawPrimitive::Image { .. })
}

pub(crate) fn layer_needs_rigid_snap(layer: &LayerNode, translated_content_context: bool) -> bool {
    (translated_content_context
        && (layer_contains_draw_primitives(layer) || layer_contains_text_primitives(layer)))
        || (layer_contains_text_primitives(layer)
            && !layer_contains_gpu_effect_text_primitives(layer))
}

pub(crate) fn layer_cache_key(layer: &LayerNode) -> usize {
    layer as *const LayerNode as usize
}

fn layer_contains_backdrop(layer: &LayerNode) -> bool {
    layer.backdrop().is_some()
        || layer.children.iter().any(|child| match child {
            RenderNode::Layer(layer) => layer_contains_backdrop(layer),
            RenderNode::Primitive(_) => false,
        })
}

pub(crate) fn layer_contains_descendant_backdrop(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| match child {
        RenderNode::Layer(layer) => layer_contains_backdrop(layer),
        RenderNode::Primitive(_) => false,
    })
}

pub(crate) fn direct_translation(transform: ProjectiveTransform) -> Option<Point> {
    let matrix = transform.matrix();
    if (matrix[0][0] - 1.0).abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || matrix[0][1].abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || matrix[1][0].abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || (matrix[1][1] - 1.0).abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || matrix[2][0].abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || matrix[2][1].abs() > SURFACE_PLAN_AFFINE_TOLERANCE
        || (matrix[2][2] - 1.0).abs() > SURFACE_PLAN_AFFINE_TOLERANCE
    {
        return None;
    }
    Some(Point::new(matrix[0][2], matrix[1][2]))
}

pub(crate) fn root_can_render_directly_cached(
    layer: &LayerNode,
    layer_surface_requirements_cache: &mut std::collections::HashMap<
        usize,
        LayerSurfaceRequirements,
    >,
) -> bool {
    let requirements = layer_surface_requirements_cached(layer, layer_surface_requirements_cache);
    !requirements
        .surface_requirements
        .has_isolating_requirement()
        && layer.backdrop().is_none()
        && layer.graphics_layer.shadow_elevation <= 0.0
        && !layer_contains_descendant_backdrop(layer)
}

pub(crate) fn layer_uses_external_backdrop_input(
    layer: &LayerNode,
    has_backdrop_underlay: bool,
) -> bool {
    has_backdrop_underlay && layer_contains_descendant_backdrop(layer)
}

pub(crate) fn composite_sample_mode_for_requirements(
    translated_content_context: bool,
    surface_capture_active: bool,
    requirements: LayerSurfaceRequirements,
) -> CompositeSampleMode {
    let effective = effective_surface_requirements(
        translated_content_context,
        surface_capture_active,
        requirements,
    );
    if effective.contains(SurfaceRequirement::MotionStableCapture)
        || requirements.pixel_stable_composite
    {
        CompositeSampleMode::Box4
    } else {
        CompositeSampleMode::Linear
    }
}

pub(crate) fn layer_surface_target_scale(
    translated_content_context: bool,
    surface_capture_active: bool,
    requirements: LayerSurfaceRequirements,
    root_scale: f32,
) -> f32 {
    let effective = effective_surface_requirements(
        translated_content_context,
        surface_capture_active,
        requirements,
    );
    if surface_capture_active && effective.contains(SurfaceRequirement::MotionStableCapture) {
        root_scale
    } else {
        effective.target_scale(root_scale)
    }
}

pub(crate) fn composite_sample_mode_for_effect_layer(layer: &EffectLayer) -> CompositeSampleMode {
    layer.requirements.composite_sample_mode()
}

pub(crate) fn effect_layer_target_scale(layer: &EffectLayer, root_scale: f32) -> f32 {
    if layer
        .requirements
        .contains(SurfaceRequirement::MotionStableCapture)
        && layer
            .requirements
            .contains(SurfaceRequirement::TextMaterialMask)
    {
        root_scale
    } else {
        layer.requirements.target_scale(root_scale)
    }
}

pub(crate) fn effect_layer_minimum_scale(layer: &EffectLayer, root_scale: f32) -> f32 {
    if layer
        .requirements
        .contains(SurfaceRequirement::MotionStableCapture)
        && layer.effect.is_none()
    {
        1.0
    } else {
        root_scale
    }
}

fn effective_surface_requirements(
    translated_content_context: bool,
    surface_capture_active: bool,
    requirements: LayerSurfaceRequirements,
) -> SurfaceRequirementSet {
    let mut effective = requirements.surface_requirements;
    if translated_content_context
        && !surface_capture_active
        && effective.contains(SurfaceRequirement::TextMaterialMask)
    {
        effective.insert(SurfaceRequirement::MotionStableCapture);
    }
    effective
}

#[cfg(test)]
pub(crate) fn layer_surface_requirements(layer: &LayerNode) -> LayerSurfaceRequirements {
    let mut layer_surface_requirements_cache = std::collections::HashMap::new();
    layer_surface_requirements_cached(layer, &mut layer_surface_requirements_cache)
}

pub(crate) fn layer_surface_requirements_cached(
    layer: &LayerNode,
    layer_surface_requirements_cache: &mut std::collections::HashMap<
        usize,
        LayerSurfaceRequirements,
    >,
) -> LayerSurfaceRequirements {
    let cache_key = layer_cache_key(layer);
    if let Some(cached) = layer_surface_requirements_cache.get(&cache_key) {
        return *cached;
    }

    let effective_isolation = effective_layer_isolation(&layer.graphics_layer);
    let mut surface_requirements = SurfaceRequirementSet::default();
    if layer.translated_content_context && layer.motion_context_animated && layer.clip_to_bounds {
        surface_requirements.insert(SurfaceRequirement::MotionStableCapture);
    }
    if layer.isolation.explicit_offscreen
        || layer.graphics_layer.compositing_strategy == CompositingStrategy::Offscreen
    {
        surface_requirements.insert(SurfaceRequirement::ExplicitOffscreen);
    }
    if layer.isolation.effect || layer.effect().is_some() {
        surface_requirements.insert(SurfaceRequirement::RenderEffect);
    }
    if layer.isolation.backdrop || layer.backdrop().is_some() {
        surface_requirements.insert(SurfaceRequirement::Backdrop);
    }
    if layer.isolation.group_opacity
        || matches!(
            layer.graphics_layer.compositing_strategy,
            CompositingStrategy::Auto | CompositingStrategy::Offscreen
        ) && effective_isolation.is_some()
            && layer.opacity() < 1.0
    {
        surface_requirements.insert(SurfaceRequirement::GroupOpacity);
    }
    if layer.isolation.blend_mode || layer.blend_mode() != BlendMode::SrcOver {
        surface_requirements.insert(SurfaceRequirement::BlendMode);
    }
    let direct_translation = direct_translation(layer.transform_to_parent);
    let mut has_direct_safe_primitive = false;
    let mut has_isolating_child_layer = false;
    let mut has_pixel_sensitive_content = false;

    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => match &primitive.node {
                PrimitiveNode::Text(text) => {
                    has_direct_safe_primitive = true;
                    let needs_local_surface = text_primitive_needs_local_surface(text);
                    if !needs_local_surface {
                        has_pixel_sensitive_content = true;
                    }
                    if needs_local_surface {
                        surface_requirements.insert(SurfaceRequirement::TextMaterialMask);
                    }
                }
                PrimitiveNode::Draw(draw) => match &draw.primitive {
                    cranpose_ui_graphics::DrawPrimitive::Shadow(_) => {
                        surface_requirements.insert(SurfaceRequirement::ImmediateShadow);
                    }
                    primitive => {
                        has_direct_safe_primitive = true;
                        if draw_primitive_is_pixel_sensitive(primitive) {
                            has_pixel_sensitive_content = true;
                        }
                    }
                },
            },
            RenderNode::Layer(child_layer) => {
                let child_requirements = layer_surface_requirements_cached(
                    child_layer.as_ref(),
                    layer_surface_requirements_cache,
                );
                if child_requirements.pixel_stable_composite {
                    has_pixel_sensitive_content = true;
                }
                if child_requirements
                    .surface_requirements
                    .has_isolating_requirement()
                    || child_requirements.direct_translation.is_none()
                {
                    has_isolating_child_layer = true;
                }
            }
        }
    }

    if has_direct_safe_primitive && has_isolating_child_layer {
        surface_requirements.insert(SurfaceRequirement::MixedDirectContent);
    }
    if direct_translation.is_none() {
        surface_requirements.insert(SurfaceRequirement::NonTranslationTransform);
    }

    let requirements = LayerSurfaceRequirements {
        direct_translation,
        surface_requirements,
        pixel_stable_composite: has_pixel_sensitive_content
            && direct_translation.is_some()
            && !layer.motion_context_animated,
    };
    layer_surface_requirements_cache.insert(cache_key, requirements);
    requirements
}

#[cfg(test)]
mod tests {
    use super::{
        composite_sample_mode_for_requirements, layer_surface_requirements,
        layer_surface_target_scale,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::surface_requirements::{SurfaceRequirement, MOTION_STABLE_SURFACE_SCALE_MULTIPLIER};
    use cranpose_render_common::graph::{
        CachePolicy, IsolationReasons, LayerNode, ProjectiveTransform, RenderNode,
    };
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui_graphics::{GraphicsLayer, Rect};

    fn test_layer(local_bounds: Rect) -> LayerNode {
        LayerNode {
            node_id: None,
            local_bounds,
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: cranpose_ui_graphics::Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: Vec::<RenderNode>::new(),
        }
    }

    #[test]
    fn rested_translated_clip_layer_does_not_force_motion_stable_capture() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.translated_content_context = true;
        layer.clip_to_bounds = true;

        let requirements = layer_surface_requirements(&layer);

        assert!(
            !requirements
                .surface_requirements
                .contains(SurfaceRequirement::MotionStableCapture),
            "rested scroll clips should stay on the direct path so content is not resampled"
        );
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Linear
        );
        assert_eq!(
            layer_surface_target_scale(true, false, requirements, 9.0),
            9.0
        );
    }

    #[test]
    fn active_translated_clip_layer_outside_capture_uses_motion_stable_scale() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            layer_surface_target_scale(true, false, requirements, 9.0),
            9.0 * MOTION_STABLE_SURFACE_SCALE_MULTIPLIER
        );
    }

    #[test]
    fn active_translated_clip_layer_inside_capture_keeps_parent_scale() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.translated_content_context = true;
        layer.motion_context_animated = true;
        layer.clip_to_bounds = true;

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            composite_sample_mode_for_requirements(true, true, requirements),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            layer_surface_target_scale(true, true, requirements, 9.0),
            9.0
        );
    }
}
