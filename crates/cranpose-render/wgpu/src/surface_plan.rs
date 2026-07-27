use crate::effect_renderer::CompositeSampleMode;
use crate::offscreen::OffscreenTarget;
use crate::scene::EffectLayer;
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use cranpose_render_common::graph::{
    LayerNode, PrimitiveEntry, PrimitiveNode, ProjectiveTransform, RenderNode,
};
use cranpose_render_common::layer_composition::effective_layer_isolation;
use cranpose_render_common::layer_transform::layer_uniform_scale;
use cranpose_ui_graphics::{BlendMode, Brush, CompositingStrategy, Point, Rect};

const SURFACE_PLAN_AFFINE_TOLERANCE: f32 = 1e-4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayerSurfaceRequirements {
    pub(crate) direct_translation: Option<Point>,
    pub(crate) surface_requirements: SurfaceRequirementSet,
    pub(crate) contains_translated_content: bool,
    pub(crate) translated_content_axes: TranslatedContentAxes,
    pub(crate) contains_backdrop_content: bool,
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
pub(crate) struct TranslatedContentAxes {
    pub(crate) x: bool,
    pub(crate) y: bool,
}

impl TranslatedContentAxes {
    pub(crate) fn from_offset(offset: Point) -> Self {
        Self {
            x: offset.x.abs() > SURFACE_PLAN_AFFINE_TOLERANCE,
            y: offset.y.abs() > SURFACE_PLAN_AFFINE_TOLERANCE,
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            x: self.x || other.x,
            y: self.y || other.y,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TranslationRenderContext {
    pub(crate) inherited_content_translation: bool,
    pub(crate) translated_content_axes: TranslatedContentAxes,
    pub(crate) surface_capture_active: bool,
    pub(crate) local_picture_capture_active: bool,
}

pub(crate) struct LayerSurfaceRequest<'a> {
    pub(crate) root_scale: f32,
    pub(crate) backdrop_underlay: Option<&'a OffscreenTarget>,
    pub(crate) allow_runtime_cache: bool,
    pub(crate) logical_rect_override: Option<Rect>,
    pub(crate) capture_clip_override: Option<Rect>,
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
    pub(crate) capture_clip_override: Option<Rect>,
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

fn layer_contains_rendered_content(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| match child {
        RenderNode::Primitive(_) => true,
        RenderNode::Layer(child_layer) => layer_contains_rendered_content(child_layer),
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
    match draw {
        cranpose_ui_graphics::DrawPrimitive::Image { .. } => true,
        cranpose_ui_graphics::DrawPrimitive::Blend { primitive, .. } => {
            draw_primitive_is_pixel_sensitive(primitive)
        }
        _ => false,
    }
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

pub(crate) fn translated_content_axes_for_layer(layer: &LayerNode) -> TranslatedContentAxes {
    if layer.translated_content_context {
        TranslatedContentAxes::from_offset(layer.translated_content_offset)
    } else {
        TranslatedContentAxes::default()
    }
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
    effective.composite_sample_mode()
}

/// The density a layer's forced surface must be rasterized at.
///
/// `layer_scale` is the uniform scale of the layer's own transform; it raises
/// the density so a magnifying (pinch-zoomed) layer resolves real detail instead
/// of being sampled up from a 1x raster. See
/// [`SurfaceRequirementSet::target_scale`].
pub(crate) fn layer_surface_target_scale(
    translated_content_context: bool,
    surface_capture_active: bool,
    requirements: LayerSurfaceRequirements,
    root_scale: f32,
    layer_scale: f32,
) -> f32 {
    let effective = effective_surface_requirements(
        translated_content_context,
        surface_capture_active,
        requirements,
    );
    let motion_stable_uses_root_scale = effective.contains(SurfaceRequirement::MotionStableCapture)
        && (surface_capture_active
            || (!translated_content_context && requirements.contains_translated_content));
    if motion_stable_uses_root_scale {
        root_scale
    } else {
        effective.target_scale(root_scale, layer_scale)
    }
}

/// The uniform scale of `layer`'s own transform, for
/// [`layer_surface_target_scale`].
pub(crate) fn layer_surface_scale(layer: &LayerNode) -> f32 {
    layer_uniform_scale(&layer.graphics_layer)
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
        // Effect layers are already flattened into the scene's device space;
        // there is no residual layer transform for the composite to magnify.
        layer.requirements.target_scale(root_scale, 1.0)
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

pub(crate) fn effective_surface_requirements(
    translated_content_context: bool,
    surface_capture_active: bool,
    requirements: LayerSurfaceRequirements,
) -> SurfaceRequirementSet {
    let mut effective = requirements.surface_requirements;
    let contains_translated_content =
        translated_content_context || requirements.contains_translated_content;
    let translated_text_material = contains_translated_content
        && !surface_capture_active
        && effective.contains(SurfaceRequirement::TextMaterialMask);
    if translated_text_material {
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
    if layer.isolation.shape_clip {
        surface_requirements.insert(SurfaceRequirement::ShapeClip);
    }
    let direct_translation = direct_translation(layer.transform_to_parent);
    let mut contains_translated_content = layer.translated_content_context;
    let mut translated_content_axes = translated_content_axes_for_layer(layer);
    let mut contains_backdrop_content = layer.backdrop().is_some();
    let mut has_direct_safe_primitive = false;
    let mut has_isolating_child_layer = false;
    let mut has_pixel_sensitive_content = false;

    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => match &primitive.node {
                PrimitiveNode::Text(text) => {
                    has_direct_safe_primitive = true;
                    let needs_local_surface = text_primitive_needs_local_surface(text);
                    if needs_local_surface {
                        surface_requirements.insert(SurfaceRequirement::TextMaterialMask);
                    } else {
                        has_pixel_sensitive_content = true;
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
                contains_translated_content |= child_requirements.contains_translated_content;
                translated_content_axes =
                    translated_content_axes.union(child_requirements.translated_content_axes);
                contains_backdrop_content |= child_requirements.contains_backdrop_content;
                if child_requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::PixelStableComposite)
                {
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

    if layer.translated_content_context
        && layer.motion_context_animated
        && layer.clip_rect().is_none()
        && layer_contains_rendered_content(layer)
    {
        surface_requirements.insert(SurfaceRequirement::MotionStableCapture);
    }

    if has_pixel_sensitive_content && direct_translation.is_some() {
        surface_requirements.insert(SurfaceRequirement::PixelStableComposite);
    }

    let requirements = LayerSurfaceRequirements {
        direct_translation,
        surface_requirements,
        contains_translated_content,
        translated_content_axes,
        contains_backdrop_content,
    };
    layer_surface_requirements_cache.insert(cache_key, requirements);
    requirements
}

#[cfg(test)]
mod tests {
    use super::{
        composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
        effect_layer_target_scale, effective_surface_requirements, layer_surface_requirements,
        layer_surface_scale, layer_surface_target_scale,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::scene::EffectLayer;
    use crate::surface_requirements::SurfaceRequirement;
    use cranpose_render_common::graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderNode,
    };
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui_graphics::{
        BlendMode, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, Point, Rect,
        RenderEffect,
    };

    fn test_layer(local_bounds: Rect) -> LayerNode {
        LayerNode {
            node_id: None,
            local_bounds,
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: cranpose_ui_graphics::Point::default(),
            content_offset: cranpose_ui_graphics::Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
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
    fn blended_image_marks_layer_pixel_stable_without_forcing_isolation() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
        let image_rect = Rect {
            x: 4.0,
            y: 6.0,
            width: 16.0,
            height: 16.0,
        };
        layer.children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Blend {
                    primitive: Box::new(DrawPrimitive::Image {
                        rect: image_rect,
                        image,
                        alpha: 1.0,
                        color_filter: None,
                        sampling: ImageSampling::Nearest,
                        src_rect: None,
                    }),
                    blend_mode: BlendMode::SrcOver,
                },
                clip: None,
            }),
        }));

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::PixelStableComposite));
        assert!(!requirements.has_isolating_requirement());
        assert_eq!(
            composite_sample_mode_for_requirements(false, false, requirements),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            layer_surface_target_scale(
                false,
                false,
                requirements,
                2.0,
                layer_surface_scale(&layer)
            ),
            2.0
        );
    }

    #[test]
    fn effect_layer_pixel_stable_requirement_uses_box4_without_motion_scale() {
        let layer = EffectLayer {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            },
            clip: None,
            snap_anchor: None,
            effect: None,
            blend_mode: BlendMode::SrcOver,
            composite_alpha: 1.0,
            z_start: 0,
            z_end: 1,
            requirements: crate::surface_requirements::SurfaceRequirementSet::default()
                .with(SurfaceRequirement::PixelStableComposite),
        };

        assert_eq!(
            composite_sample_mode_for_effect_layer(&layer),
            CompositeSampleMode::Box4
        );
        assert_eq!(effect_layer_target_scale(&layer, 3.0), 3.0);
    }

    #[test]
    fn shape_clip_marks_layer_isolated_without_render_effect_requirement() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.isolation.shape_clip = true;

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements.has_isolating_requirement());
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::ShapeClip));
        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::RenderEffect));
    }

    #[test]
    fn inherited_translated_isolated_pixel_surface_uses_pixel_stable_composite() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));
        let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
        layer.children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Image {
                    rect: Rect {
                        x: 4.0,
                        y: 6.0,
                        width: 16.0,
                        height: 16.0,
                    },
                    image,
                    alpha: 1.0,
                    color_filter: None,
                    sampling: ImageSampling::Nearest,
                    src_rect: None,
                },
                clip: None,
            }),
        }));

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::RenderEffect));
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::PixelStableComposite));
        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        let effective = effective_surface_requirements(true, false, requirements);
        assert!(!effective.contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            layer_surface_target_scale(true, false, requirements, 2.0, layer_surface_scale(&layer)),
            2.0
        );
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Box4
        );
    }

    #[test]
    fn translated_descendant_isolated_pixel_surface_uses_pixel_stable_composite() {
        let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
        let mut scrolled_content = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 240.0,
        });
        scrolled_content.translated_content_context = true;
        scrolled_content.translated_content_offset = Point::new(0.0, -48.0);
        scrolled_content
            .children
            .push(RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Image {
                        rect: Rect {
                            x: 4.0,
                            y: 96.0,
                            width: 16.0,
                            height: 16.0,
                        },
                        image,
                        alpha: 1.0,
                        color_filter: None,
                        sampling: ImageSampling::Nearest,
                        src_rect: None,
                    },
                    clip: None,
                }),
            }));

        let mut viewport = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        viewport.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));
        viewport
            .children
            .push(RenderNode::Layer(Box::new(scrolled_content)));

        let requirements = layer_surface_requirements(&viewport);

        assert!(requirements.contains_translated_content);
        assert!(!requirements.translated_content_axes.x);
        assert!(requirements.translated_content_axes.y);
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::RenderEffect));
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::PixelStableComposite));
        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert!(!effective_surface_requirements(false, false, requirements)
            .contains(SurfaceRequirement::MotionStableCapture));
        assert!(!effective_surface_requirements(false, true, requirements)
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            layer_surface_target_scale(
                false,
                false,
                requirements,
                2.0,
                layer_surface_scale(&viewport)
            ),
            2.0
        );
        assert_eq!(
            layer_surface_target_scale(
                false,
                true,
                requirements,
                2.0,
                layer_surface_scale(&viewport)
            ),
            2.0
        );
    }

    #[test]
    fn translated_backdrop_dependent_surface_does_not_use_motion_stable_capture() {
        let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
        let mut backdrop_child = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 48.0,
        });
        backdrop_child.graphics_layer.backdrop_effect = Some(RenderEffect::blur(4.0));
        backdrop_child
            .children
            .push(RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Image {
                        rect: Rect {
                            x: 8.0,
                            y: 8.0,
                            width: 16.0,
                            height: 16.0,
                        },
                        image,
                        alpha: 1.0,
                        color_filter: None,
                        sampling: ImageSampling::Nearest,
                        src_rect: None,
                    },
                    clip: None,
                }),
            }));

        let mut scrolled_content = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 240.0,
        });
        scrolled_content.translated_content_context = true;
        scrolled_content
            .children
            .push(RenderNode::Layer(Box::new(backdrop_child)));

        let mut viewport = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        viewport.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));
        viewport
            .children
            .push(RenderNode::Layer(Box::new(scrolled_content)));

        let requirements = layer_surface_requirements(&viewport);

        assert!(requirements.contains_translated_content);
        assert!(requirements.contains_backdrop_content);
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::RenderEffect));
        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::PixelStableComposite));
        assert!(!effective_surface_requirements(false, false, requirements)
            .contains(SurfaceRequirement::MotionStableCapture));
        assert!(!effective_surface_requirements(false, true, requirements)
            .contains(SurfaceRequirement::MotionStableCapture));
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
            layer_surface_target_scale(true, false, requirements, 9.0, layer_surface_scale(&layer)),
            9.0
        );
    }

    #[test]
    fn active_translated_clip_layer_outside_capture_stays_direct() {
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

        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Linear
        );
        assert_eq!(
            layer_surface_target_scale(true, false, requirements, 9.0, layer_surface_scale(&layer)),
            9.0
        );
    }

    #[test]
    fn active_translated_clip_layer_inside_capture_stays_direct() {
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

        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            composite_sample_mode_for_requirements(true, true, requirements),
            CompositeSampleMode::Linear
        );
        assert_eq!(
            layer_surface_target_scale(true, true, requirements, 9.0, layer_surface_scale(&layer)),
            9.0
        );
    }

    /// A pinch-zoomed layer: a scaling `transform_to_parent` forces an
    /// offscreen surface, and that surface must be allocated at the layer's
    /// EFFECTIVE device scale. Rasterizing it at plain `root_scale` and letting
    /// the composite magnify the texture is what made zoom fuzzy.
    #[test]
    fn zoomed_layer_surface_resolves_at_root_scale_times_the_layer_scale() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.transform_to_parent = ProjectiveTransform::uniform_scale(4.0);
        layer.graphics_layer.scale = 4.0;

        let requirements = layer_surface_requirements(&layer);

        assert!(
            requirements
                .surface_requirements
                .contains(SurfaceRequirement::NonTranslationTransform),
            "a scaling transform is not a direct translation, so it forces a surface"
        );
        assert!(requirements.has_renderer_forced_surface());
        assert_eq!(
            composite_sample_mode_for_requirements(false, false, requirements),
            CompositeSampleMode::Linear,
            "the composite resamples, so the source density is what decides sharpness"
        );
        assert_eq!(
            layer_surface_target_scale(
                false,
                false,
                requirements,
                3.0,
                layer_surface_scale(&layer)
            ),
            12.0
        );
    }

    /// The scale must come off the layer, not off a constant: halving the zoom
    /// halves the density the surface is allocated at.
    #[test]
    fn zoomed_layer_surface_scale_tracks_the_layer_scale() {
        let mut layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });
        layer.transform_to_parent = ProjectiveTransform::uniform_scale(2.0);
        layer.graphics_layer.scale = 2.0;

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            layer_surface_target_scale(
                false,
                false,
                requirements,
                3.0,
                layer_surface_scale(&layer)
            ),
            6.0
        );
    }

    /// An unscaled layer is untouched: same texture density as before the fix.
    #[test]
    fn unscaled_layer_surface_keeps_the_root_scale_density() {
        let layer = test_layer(Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        });

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(layer_surface_scale(&layer), 1.0);
        assert_eq!(
            layer_surface_target_scale(
                false,
                false,
                requirements,
                3.0,
                layer_surface_scale(&layer)
            ),
            3.0
        );
    }

    /// The invariant the robot's layout-jitter contract watches, without a GPU:
    /// a layer whose scale ANIMATES must stay put in layout space and must not
    /// re-rasterize every frame.
    ///
    /// Raising `target_scale` to the live layer scale made both false at once.
    /// A 36 dp box sweeping 0.85 -> 1.15 got a distinct surface size on nearly
    /// every frame (measured on the demo's Animations tab: 37, 38, 39, 40, 41,
    /// 42 px within one sweep), so its rounded-corner antialiasing was
    /// recomputed at a new sub-pixel phase per frame, and the ceil'd texture
    /// was stretched across a destination quad built from the unpadded rect,
    /// compressing the content toward the quad's origin by up to half a device
    /// pixel. Nothing moved in layout space, but every edge pixel did.
    #[test]
    fn an_animated_layer_scale_holds_its_surface_and_its_layout_rect() {
        use crate::surface_executor::{device_pixel_exact_surface_rect, surface_target_size};
        use cranpose_render_common::layer_transform::layer_transform_to_parent;
        use cranpose_ui_graphics::TransformOrigin;

        const MAX_TEXTURE_DIM: u32 = 8192;
        const ROOT_SCALE: f32 = 1.0;
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 36.0,
            height: 36.0,
        };
        let placement = Point { x: 54.0, y: 416.0 };

        let mut surface_sizes: Vec<(u32, u32)> = Vec::new();
        for frame in 0..=40 {
            // The demo's "Scale + Fade (layer lambda)" animation.
            let progress = frame as f32 / 40.0;
            let mut layer = test_layer(local_bounds);
            layer.graphics_layer.scale = 0.85 + 0.3 * progress;
            layer.graphics_layer.alpha = 0.4 + 0.6 * progress;
            layer.graphics_layer.transform_origin = TransformOrigin::CENTER;
            layer.transform_to_parent =
                layer_transform_to_parent(local_bounds, placement, &layer.graphics_layer);

            let requirements = layer_surface_requirements(&layer);
            let target_scale = layer_surface_target_scale(
                false,
                false,
                requirements,
                ROOT_SCALE,
                layer_surface_scale(&layer),
            );
            let (width, height) = surface_target_size(local_bounds, target_scale, MAX_TEXTURE_DIM);
            if surface_sizes.last() != Some(&(width, height)) {
                surface_sizes.push((width, height));
            }

            // What the composite actually maps: the whole texture onto the quad
            // the surface rect maps to. The layer's own content stops at
            // `local_bounds * target_scale` texels, so its layout-space extent
            // is that fraction of the quad.
            let surface_rect =
                device_pixel_exact_surface_rect(local_bounds, target_scale, width, height);
            let quad = layer.transform_to_parent.bounds_for_rect(surface_rect);
            let composited = Rect {
                x: quad.x,
                y: quad.y,
                width: quad.width * (local_bounds.width * target_scale) / width as f32,
                height: quad.height * (local_bounds.height * target_scale) / height as f32,
            };

            // ... and that has to be exactly where layout put the layer,
            // whatever resolution the texture happened to get.
            let laid_out = layer.transform_to_parent.bounds_for_rect(local_bounds);
            for (label, composited, laid_out) in [
                ("x", composited.x, laid_out.x),
                ("y", composited.y, laid_out.y),
                ("width", composited.width, laid_out.width),
                ("height", composited.height, laid_out.height),
            ] {
                assert!(
                    (composited - laid_out).abs() < 1e-2,
                    "frame {frame} (scale {}) composited {label} {composited} away from its laid-out {laid_out}",
                    layer.graphics_layer.scale
                );
            }
        }

        assert_eq!(
            surface_sizes,
            vec![(36, 36), (45, 45)],
            "a 41-frame scale sweep must reuse two surfaces, not reallocate per frame"
        );
    }
}
