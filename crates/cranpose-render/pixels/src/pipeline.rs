use std::{ops::Range, rc::Rc, sync::Arc};

use cranpose_render_common::{
    Brush,
    graph::{
        LayerNode, PrimitiveEntry, PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode,
        TextPrimitiveNode, for_each_in_draw_order,
    },
    graph_scene::RenderDiagnostics,
    hit_graph::collect_hits_from_graph,
    layer_composition::folded_layer_alpha,
    layer_shadow::{ShadowLight, layer_shadow_geometry},
    layer_transform::{apply_layer_affine_to_rect, apply_layer_to_rect, layer_uniform_scale},
    primitive_emit::{
        DrawPrimitiveSink, ImageDrawParams, PrimitiveClipSpace, ShapeDrawParams, TextDrawParams,
        draw_shape_params_for_primitive, emit_draw_primitive, resolve_clip, resolve_primitive_clip,
    },
    text_decoration::{visit_text_backgrounds, visit_text_lines},
    text_raster::GlyphInk,
};
use cranpose_ui::{LayoutBox, text::TextStyle};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, DrawPrimitive, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect, RoundedCornerShape, TransformOrigin, whole,
};

use crate::{
    draw::TextRaster,
    scene::{ImageDraw, ItemCounts, PassedDown, RasterScene, Scene, TextDraw},
    style::{
        apply_layer_to_brush, apply_layer_to_color, compose_color_filters, scale_corner_radii,
    },
    surface::{SurfacePlacement, rasterize_surface, scales_and_moves},
};

fn graphics_layer_supports_rigid_snap(layer: &GraphicsLayer) -> bool {
    (layer.scale - 1.0).abs() <= f32::EPSILON
        && (layer.scale_x - 1.0).abs() <= f32::EPSILON
        && (layer.scale_y - 1.0).abs() <= f32::EPSILON
        && layer.rotation_x.abs() <= f32::EPSILON
        && layer.rotation_y.abs() <= f32::EPSILON
        && layer.rotation_z.abs() <= f32::EPSILON
}

fn rigid_snap_anchor(layer_bounds: Rect, layer: &GraphicsLayer) -> Option<Point> {
    if !graphics_layer_supports_rigid_snap(layer) {
        return None;
    }
    let mapped = apply_layer_affine_to_rect(layer_bounds, layer_bounds, layer);
    Some(Point::new(mapped.x, mapped.y))
}

fn assign_snap_anchor_since(
    scene: &mut RasterScene,
    counts: ItemCounts,
    snap_anchor: Option<Point>,
) {
    let Some(snap_anchor) = snap_anchor else {
        return;
    };

    for shape in &mut scene.shapes[counts.shapes..] {
        shape.snap_anchor = Some(snap_anchor);
    }
    for image in &mut scene.images[counts.images..] {
        image.snap_anchor = Some(snap_anchor);
    }
    for text in &mut scene.texts[counts.texts..] {
        text.snap_anchor = Some(snap_anchor);
    }
    for shadow in &mut scene.shadows[counts.shadows..] {
        shadow.snap_anchor = Some(snap_anchor);
    }
}

#[derive(Clone, Copy)]
struct PrimitiveRenderContext<'a> {
    layer_bounds: RasterLayerBounds,
    node_layer: &'a GraphicsLayer,
    visual_clip: Option<Rect>,
    motion_context_animated: bool,
    content_offset_translation: bool,
    layer_snap_anchor: Option<Point>,
}

/// The alpha and colour filter content draws with: a layer's own folded
/// alpha and filter combined with those of every layer above it. No other
/// property of a layer passes to the content beneath it.
#[derive(Clone)]
struct ContentStyle {
    alpha: f32,
    color_filter: Option<ColorFilter>,
}

impl ContentStyle {
    const PLAIN: Self = Self {
        alpha: 1.0,
        color_filter: None,
    };

    /// The style of `layer`'s content beneath content drawn in `self`.
    fn under(&self, layer: &GraphicsLayer) -> Self {
        Self {
            alpha: (self.alpha * folded_layer_alpha(layer)).clamp(0.0, 1.0),
            color_filter: compose_color_filters(
                self.color_filter.as_ref(),
                layer.color_filter.as_ref(),
            ),
        }
    }
}

struct RasterTraversalContext<'a> {
    parent_transform: &'a ProjectiveTransform,
    parent_content_style: &'a ContentStyle,
    parent_visual_clip: Option<Rect>,
    inherited_translated_snap_anchor: Option<Point>,
    inherited_translated_content_context: bool,
    diagnostics: &'a RenderDiagnostics,
    /// The window's shadow light in device pixels, the space the raster
    /// scene is drawn in.
    light: ShadowLight,
    /// Whether everything above is what the walk passed down to the layer's
    /// old record in the frame before's scene.
    same_as_before: bool,
    /// The fonts and glyphs a surface's text is drawn with.
    text: &'a TextRaster,
    /// Whether the layer is the one whose surface the walk draws: it draws
    /// its items straight into the surface, and its parent drew its shadow.
    surface_root: bool,
    /// Whether the walk lies in a scene's space, whose scene draws the
    /// mesh surfaces and 3D transforms there, as its eye sees them.
    in_space: bool,
}

/// The scene the frame before built, whose items a walk copies for each
/// subtree that did not change since.
struct Reuse<'a> {
    old: &'a RasterScene,
    /// The old record of the layer the walk reaches next, while the graph
    /// keeps the shape it had.
    cursor: usize,
    /// The old records of unchanged subtrees, siblings one after another,
    /// to copy before the scene takes anything else, and the counts their
    /// items reach there: a run of them copies as one.
    pending: Option<(Range<usize>, ItemCounts)>,
}

impl Reuse<'_> {
    /// The index of `layer`'s old record, at the cursor or one subtree on,
    /// after a sibling before it went away.
    fn find(&self, layer: &LayerNode) -> Option<usize> {
        let identity = (layer.node_id, layer.wraps);
        let records = &self.old.subtrees;
        let at = records.get(self.cursor)?;
        if at.identity == identity {
            return Some(self.cursor);
        }
        records
            .get(at.end)
            .is_some_and(|next| next.identity == identity)
            .then_some(at.end)
    }

    /// Queues the copy of the subtree of the old record `index`, which ends
    /// at `end` and pushed its items up to `stop`.
    fn copy_later(&mut self, scene: &mut RasterScene, index: usize, end: usize, stop: ItemCounts) {
        match &mut self.pending {
            Some((records, pending_stop)) if records.end == index => {
                records.end = end;
                *pending_stop = stop;
            }
            _ => {
                self.flush(scene);
                self.pending = Some((index..end, stop));
            }
        }
    }

    /// Copies the queued subtrees into `scene`.
    fn flush(&mut self, scene: &mut RasterScene) {
        if let Some((records, stop)) = self.pending.take() {
            scene.copy_subtrees(self.old, records, stop);
        }
    }
}

/// Copies the subtrees `reuse` queued into `scene`, before it takes items of
/// its own.
fn flush_copies(reuse: &mut Option<Reuse<'_>>, scene: &mut RasterScene) {
    if let Some(reuse) = reuse.as_mut() {
        reuse.flush(scene);
    }
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

fn layer_needs_rigid_snap(layer: &LayerNode, translated_content_context: bool) -> bool {
    (translated_content_context
        && (layer_contains_draw_primitives(layer) || layer_contains_text_primitives(layer)))
        || (layer_contains_text_primitives(layer) && layer_contains_draw_primitives(layer))
}

fn is_render_effect_supported(_effect: &RenderEffect) -> bool {
    false
}

fn layer_requires_effect_fallback(layer: &GraphicsLayer) -> bool {
    layer
        .render_effect
        .as_ref()
        .is_some_and(|effect| !is_render_effect_supported(effect))
        || layer
            .backdrop_effect
            .as_ref()
            .is_some_and(|effect| !is_render_effect_supported(effect))
        || layer.blend_mode != BlendMode::SrcOver
}

fn report_unsupported_effects(layer: &GraphicsLayer, diagnostics: &RenderDiagnostics) {
    if layer_requires_effect_fallback(layer)
        && diagnostics.claim_warning_once("pixels.unsupported-layer-effect")
    {
        log::warn!(
            "Pixels renderer does not support render/backdrop effects or non-SrcOver layer blend modes; falling back to base layer rendering"
        );
    }
}

#[derive(Clone, Copy)]
struct ShadowSample {
    expansion: f32,
    weight: f32,
}

fn blur_samples(blur_radius: f32) -> Vec<ShadowSample> {
    if blur_radius <= f32::EPSILON {
        return Vec::new();
    }

    let sample_count = (whole::ceil(blur_radius * 2.4) as usize).clamp(8, 36);
    let sigma = (blur_radius * 0.5).max(1.0);
    let mut samples = Vec::with_capacity(sample_count);
    let mut weight_sum = 0.0f32;

    for index in 0..sample_count {
        let t0 = index as f32 / sample_count as f32;
        let t1 = (index + 1) as f32 / sample_count as f32;
        let center = blur_radius * (t0 + t1) * 0.5;
        let expansion = blur_radius * t1;
        let weight = (-0.5 * (center / sigma).powi(2)).exp().max(0.0001);
        samples.push(ShadowSample { expansion, weight });
        weight_sum += weight;
    }

    if weight_sum <= f32::EPSILON {
        return vec![ShadowSample {
            expansion: blur_radius,
            weight: 1.0,
        }];
    }

    for sample in &mut samples {
        sample.weight /= weight_sum;
    }

    samples
}

fn expanded_shape_rect(shape: &crate::scene::DrawShape, expansion: f32) -> Rect {
    Rect {
        x: shape.rect.x - expansion,
        y: shape.rect.y - expansion,
        width: (shape.rect.width + expansion * 2.0).max(0.0),
        height: (shape.rect.height + expansion * 2.0).max(0.0),
    }
}

fn push_blurred_shape_samples(
    scene: &mut RasterScene,
    shape: &crate::scene::DrawShape,
    blend_mode: BlendMode,
    clip: Option<Rect>,
    blur_radius: f32,
) {
    let samples = blur_samples(blur_radius.max(1.0));
    if samples.is_empty() {
        scene.push_shape(
            shape.rect,
            shape.brush.clone(),
            shape.outline.rounded(),
            clip,
            blend_mode,
        );
        return;
    }

    for sample in samples.iter().rev() {
        scene.push_shape(
            expanded_shape_rect(shape, sample.expansion),
            scale_brush_alpha(shape.brush.clone(), sample.weight),
            shape.outline.rounded(),
            clip,
            blend_mode,
        );
    }
}

fn scale_color_alpha(color: Color, alpha: f32) -> Color {
    Color(
        color.r(),
        color.g(),
        color.b(),
        (color.a() * alpha).clamp(0.0, 1.0),
    )
}

fn scale_brush_alpha(brush: Brush, alpha: f32) -> Brush {
    brush.map_colors(|color| scale_color_alpha(color, alpha))
}

fn push_layer_shadow(
    scene: &mut RasterScene,
    layer: &GraphicsLayer,
    layer_bounds: RasterLayerBounds,
    transformed_bounds: Rect,
    clip: Option<Rect>,
    light: ShadowLight,
) {
    if layer.shadow_elevation <= 0.0 {
        return;
    }
    let caster = match layer.shape {
        LayerShape::Rectangle => None,
        LayerShape::Rounded(shape) => {
            let scale = layer_uniform_scale(layer).max(0.1);
            let resolved = shape.resolve(
                layer_bounds.local_bounds.width,
                layer_bounds.local_bounds.height,
            );
            Some(RoundedCornerShape::with_radii(scale_corner_radii(
                resolved, scale,
            )))
        }
    };
    for (shadow, color) in layer_shadow_geometry(layer, transformed_bounds, caster, light).passes()
    {
        scene.push_shadow(shadow, color, clip);
    }
}

pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(root, 1.0);
    collect_hits_from_graph(&graph.root, ProjectiveTransform::identity(), scene, None);
    scene.replace_graph(graph);
}

fn resolve_text_color_without_gradient_fallback(text_style: &TextStyle, default: Color) -> Color {
    let mut color = text_style
        .span_style
        .color
        .or(match text_style.span_style.brush.as_ref() {
            Some(Brush::Solid(color)) => Some(*color),
            _ => None,
        })
        .unwrap_or(default);
    if let Some(alpha) = text_style.span_style.alpha {
        color.3 *= alpha.clamp(0.0, 1.0);
    }
    color
}

// Where a text node draws: the bounds and transform of its layer, its box
// before the transform and its clip.
#[derive(Clone, Copy)]
struct TextPlacement<'a> {
    bounds: Rect,
    text_rect: Rect,
    layer: &'a GraphicsLayer,
    clip: Option<Rect>,
}

fn push_text_style_draws(
    scene: &mut RasterScene,
    node: &TextPrimitiveNode,
    placement: TextPlacement<'_>,
) {
    let (rect, node_layer, text_clip) = (placement.bounds, placement.layer, placement.clip);
    let (text, text_style, layout) = (&node.text, &node.text_style, &node.layout);
    let text_rect = placement.text_rect;
    let transformed_text_rect = apply_layer_to_rect(text_rect, rect, node_layer);
    // A line or a box of the text, in layout pixels, where the node draws it.
    let placed = |part: Rect| {
        apply_layer_to_rect(part.translate(text_rect.x, text_rect.y), rect, node_layer)
    };

    visit_text_backgrounds(layout, text.as_ref().into(), text_style, |part, brush| {
        scene.push_shape(
            placed(part),
            apply_layer_to_brush(brush.clone(), node_layer),
            None,
            text_clip,
            BlendMode::SrcOver,
        );
    });

    let text_color =
        resolve_text_color_without_gradient_fallback(text_style, Color(1.0, 1.0, 1.0, 1.0));
    let transformed_text_color = apply_layer_to_color(text_color, node_layer);
    // The glyphs draw without the shadow and with the brush the layer
    // tints; a style with neither is shared as it is.
    let transformed_text_style =
        if text_style.span_style.shadow.is_none() && text_style.span_style.brush.is_none() {
            Arc::clone(text_style)
        } else {
            let mut style = TextStyle::clone(text_style);
            style.span_style.shadow = None;
            style.span_style.brush = text_style
                .span_style
                .brush
                .clone()
                .map(|brush| apply_layer_to_brush(brush, node_layer));
            Arc::new(style)
        };

    if let Some(shadow) = text_style.span_style.shadow {
        let shadow_rect = text_rect.translate(shadow.offset.x, shadow.offset.y);
        let mut shadow_text_style = TextStyle::clone(&transformed_text_style);
        shadow_text_style.span_style.shadow = Some(cranpose_ui::text::Shadow {
            color: apply_layer_to_color(shadow.color, node_layer),
            offset: Point::new(0.0, 0.0),
            blur_radius: shadow.blur_radius,
        });
        scene.push_text(TextDraw {
            rect: apply_layer_to_rect(shadow_rect, rect, node_layer),
            snap_anchor: None,
            text: Rc::clone(text),
            layout: layout.clone(),
            color: Color::TRANSPARENT,
            text_style: Arc::new(shadow_text_style),
            font_size: node.font_size,
            scale: layer_uniform_scale(node_layer),
            ink: GlyphInk::Hidden,
            clip: text_clip,
        });
    }

    scene.push_text(TextDraw {
        rect: transformed_text_rect,
        snap_anchor: None,
        text: Rc::clone(text),
        layout: layout.clone(),
        color: transformed_text_color,
        text_style: transformed_text_style,
        font_size: node.font_size,
        scale: layer_uniform_scale(node_layer),
        ink: GlyphInk::Painted,
        clip: text_clip,
    });

    visit_text_lines(
        layout,
        text.as_ref().into(),
        text_style,
        text_color,
        |part, brush| {
            scene.push_pixel_snapped_shape(
                placed(part),
                apply_layer_to_brush(brush.clone(), node_layer),
                None,
                text_clip,
                BlendMode::SrcOver,
            );
        },
    );
}

pub(crate) fn build_raster_scene(
    graph: &RenderGraph,
    diagnostics: &RenderDiagnostics,
    scale: f32,
    text: &TextRaster,
) -> RasterScene {
    let mut scene = RasterScene::new();
    fill_raster_scene(&mut scene, graph, diagnostics, scale, None, text);
    scene
}

/// Replaces what `scene` holds with `graph` drawn at `scale`, keeping the
/// scene's storage. The items of each subtree that `previous`, the scene of
/// the frame before, holds for the same content in the same place are
/// copied from it.
mod in_place;
mod space;

pub(crate) use in_place::{InPlaceUpdate, UpdatedReach};
use space::{push_seen_surfaces, seen_surfaces};

/// The shadow light of a scene `graph` gives at `scale`.
fn scene_light(graph: &RenderGraph, scale: f32) -> ShadowLight {
    ShadowLight::for_window(
        graph.root.local_bounds.width * scale,
        graph.root.local_bounds.height * scale,
        scale,
        1.0,
    )
}

pub(crate) fn fill_raster_scene(
    scene: &mut RasterScene,
    graph: &RenderGraph,
    diagnostics: &RenderDiagnostics,
    scale: f32,
    previous: Option<&RasterScene>,
    text: &TextRaster,
) {
    scene.clear();
    let light = scene_light(graph, scale);
    scene.start = Some((scale, light));
    let mut reuse = previous.map(|old| Reuse {
        old,
        cursor: 0,
        pending: None,
    });
    populate_draws_from_graph(
        &graph.root,
        scene,
        &RasterTraversalContext {
            parent_transform: &ProjectiveTransform::uniform_scale(scale),
            parent_content_style: &ContentStyle::PLAIN,
            parent_visual_clip: None,
            inherited_translated_snap_anchor: None,
            inherited_translated_content_context: false,
            diagnostics,
            light,
            same_as_before: previous.is_some_and(|old| old.start == scene.start),
            text,
            surface_root: false,
            in_space: false,
        },
        &mut reuse,
    );
    flush_copies(&mut reuse, scene);
}

#[derive(Clone, Copy)]
struct RasterLayerBounds {
    device_origin: Point,
    local_bounds: Rect,
}

impl RasterLayerBounds {
    fn from_transformed_bounds(transformed_bounds: Rect, local_bounds: Rect) -> Self {
        Self {
            device_origin: Point::new(transformed_bounds.x, transformed_bounds.y),
            local_bounds,
        }
    }

    fn raster_rect(self) -> Rect {
        Rect {
            x: self.device_origin.x,
            y: self.device_origin.y,
            width: self.local_bounds.width,
            height: self.local_bounds.height,
        }
    }
}

struct RasterLayerMapping {
    layer_bounds: RasterLayerBounds,
    /// Where the node's own draws and text sit: its rect, whose origin is
    /// the node's even where its layer is bounded at a coordinator away
    /// from it.
    content_bounds: RasterLayerBounds,
    transformed_bounds: Rect,
    content_style: ContentStyle,
    scale_x: f32,
    scale_y: f32,
}

/// The layer of content drawn at alpha one with no colour filter and at
/// scale one, which most layers' content takes.
const PLAIN_CONTENT_LAYER: GraphicsLayer = GraphicsLayer {
    transform_origin: TransformOrigin::new(0.0, 0.0),
    ..GraphicsLayer::DEFAULT
};

impl RasterLayerMapping {
    /// Whether the node's own draws take [`PLAIN_CONTENT_LAYER`], so need no
    /// [`Self::content_layer`] built.
    fn draws_plain(&self) -> bool {
        self.content_style.alpha == 1.0
            && self.content_style.color_filter.is_none()
            && self.scale_x == 1.0
            && self.scale_y == 1.0
    }

    /// The layer the node's own draws take: its content style, scaled from
    /// local to device pixels about its origin.
    fn content_layer(&self) -> GraphicsLayer {
        GraphicsLayer {
            alpha: self.content_style.alpha,
            color_filter: self.content_style.color_filter.clone(),
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            transform_origin: TransformOrigin::new(0.0, 0.0),
            ..GraphicsLayer::default()
        }
    }
}

fn raster_layer_scale(transformed_bounds: Rect, local_bounds: Rect) -> (f32, f32) {
    (
        axis_scale(transformed_bounds.width, local_bounds.width).max(0.0),
        axis_scale(transformed_bounds.height, local_bounds.height).max(0.0),
    )
}

/// How far a transform stretches an axis of `local` extent to `transformed`,
/// 1 for an empty axis. An axis it keeps gives 1 without the division, which
/// a chip with no float divider pays for on every layer.
fn axis_scale(transformed: f32, local: f32) -> f32 {
    if local.abs() <= f32::EPSILON || (transformed == local && local.is_finite()) {
        1.0
    } else {
        transformed / local
    }
}

fn raster_layer_mapping(
    layer: &LayerNode,
    transform: &ProjectiveTransform,
    parent_content_style: &ContentStyle,
) -> RasterLayerMapping {
    let transformed_bounds = transform.bounds_for_rect(layer.local_bounds);
    let (scale_x, scale_y) = raster_layer_scale(transformed_bounds, layer.local_bounds);
    // A draw group's layer copies the paint of the node whose draws it
    // holds, which reaches it here already.
    let content_style = if layer.draw_phase.is_some() {
        parent_content_style.clone()
    } else {
        parent_content_style.under(&layer.graphics_layer)
    };
    let layer_bounds =
        RasterLayerBounds::from_transformed_bounds(transformed_bounds, layer.local_bounds);
    let node_rect = layer.node_rect();
    let content_bounds =
        RasterLayerBounds::from_transformed_bounds(transform.bounds_for_rect(node_rect), node_rect);

    RasterLayerMapping {
        layer_bounds,
        content_bounds,
        transformed_bounds,
        content_style,
        scale_x,
        scale_y,
    }
}

/// Queues `layer`'s elevation shadow, when it casts one, cut to its shadow
/// clip within `parent_visual_clip`.
// Out of line, as are the surface and a text's draw: inlined, their locals
// would take room on the stack at every layer nesting level of the walk,
// which recurses through `push_layer_items`. A child's other draws stay
// inline: out of line, the ESP32-S3 demo waited longer for their code.
#[inline(never)]
fn push_layer_shadow_of(
    scene: &mut RasterScene,
    layer: &LayerNode,
    mapping: &RasterLayerMapping,
    transform: &ProjectiveTransform,
    parent_visual_clip: Option<Rect>,
    light: ShadowLight,
) {
    if layer.graphics_layer.shadow_elevation <= 0.0 {
        return;
    }
    let shadow_clip = resolve_clip(
        parent_visual_clip,
        layer
            .shadow_clip()
            .map(|clip| transform.bounds_for_rect(clip)),
    );
    let shadow_layer = GraphicsLayer {
        alpha: layer.graphics_layer.alpha,
        scale_x: mapping.scale_x,
        scale_y: mapping.scale_y,
        transform_origin: TransformOrigin::new(0.0, 0.0),
        shadow_elevation: layer.graphics_layer.shadow_elevation,
        ambient_shadow_color: layer.graphics_layer.ambient_shadow_color,
        spot_shadow_color: layer.graphics_layer.spot_shadow_color,
        shape: layer.graphics_layer.shape,
        ..GraphicsLayer::default()
    };
    push_layer_shadow(
        scene,
        &shadow_layer,
        mapping.layer_bounds,
        mapping.transformed_bounds,
        shadow_clip,
        light,
    );
}

/// Pushes the items of `layer` and its subtree, copied from the frame
/// before's scene when they did not change since, and records them for the
/// next frame.
fn populate_draws_from_graph(
    layer: &LayerNode,
    scene: &mut RasterScene,
    context: &RasterTraversalContext<'_>,
    reuse: &mut Option<Reuse<'_>>,
) {
    // The end of the layer's old record and where the old scene holds what
    // the layer passed down then.
    let mut before = None;
    if let Some(reuse) = reuse.as_mut()
        && let Some(index) = reuse.find(layer)
        && let Some(record) = reuse.old.subtrees.get(index)
    {
        // The layer's number covers its subtree, save its placement.
        if context.same_as_before
            && record.change == layer.change
            && record.transform == layer.transform_to_parent
        {
            let (end, stop) = (record.end, record.stop);
            reuse.cursor = end;
            reuse.copy_later(scene, index, end, stop);
            return;
        }
        reuse.cursor = index + 1;
        before = Some((record.end, record.passed_down));
    }
    flush_copies(reuse, scene);
    let record = scene.open_subtree(layer);
    push_layer_items(
        layer,
        scene,
        context,
        record,
        before.and_then(|(_, passed_down)| passed_down),
        reuse,
    );
    flush_copies(reuse, scene);
    scene.close_subtree(record);
    if let (Some(reuse), Some((end, _))) = (reuse.as_mut(), before) {
        reuse.cursor = end;
    }
}

/// Draws `layer` and its subtree into a surface of its own and queues the
/// surface's pixels, placed by `transform`, the layer's map to device
/// pixels.
#[inline(never)]
fn push_layer_surface(
    layer: &LayerNode,
    scene: &mut RasterScene,
    context: &RasterTraversalContext<'_>,
    transform: &ProjectiveTransform,
) {
    let Some(placement) = SurfacePlacement::of(layer.local_bounds, transform) else {
        return;
    };
    // The walk below places the layer by its own transform first.
    let Some(parent_transform) = layer
        .transform_to_parent
        .inverse()
        .map(|inverse| inverse.then(placement.to_surface))
    else {
        return;
    };
    // The surface takes the alpha as it lands, so its items overlap as
    // they would in the layer's own buffer and a mask keeps its alpha.
    let content_style = ContentStyle {
        alpha: 1.0,
        color_filter: context.parent_content_style.color_filter.clone(),
    };
    let mut items = RasterScene::new();
    let record = items.open_subtree(layer);
    push_layer_items(
        layer,
        &mut items,
        &RasterTraversalContext {
            parent_transform: &parent_transform,
            parent_content_style: &content_style,
            parent_visual_clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: placement.width as f32,
                height: placement.height as f32,
            }),
            inherited_translated_snap_anchor: None,
            inherited_translated_content_context: false,
            diagnostics: context.diagnostics,
            light: context.light,
            same_as_before: false,
            text: context.text,
            surface_root: true,
            in_space: context.in_space,
        },
        record,
        None,
        &mut None,
    );
    items.close_subtree(record);
    if let Some(image) = rasterize_surface(
        &items,
        placement.width,
        placement.height,
        context.diagnostics,
        context.text,
    ) {
        scene.push_surface_image(
            image,
            placement.rect,
            placement.to_device,
            context.parent_visual_clip,
            context.parent_content_style.alpha,
        );
    }
}

/// Pushes the items of `layer` and its subtree, whose record is `record`.
/// `passed_down_before` is where the old scene holds what the layer passed
/// to its child layers in the frame before.
fn push_layer_items(
    layer: &LayerNode,
    scene: &mut RasterScene,
    context: &RasterTraversalContext<'_>,
    record: usize,
    passed_down_before: Option<usize>,
    reuse: &mut Option<Reuse<'_>>,
) {
    let diagnostics = context.diagnostics;
    let transform = layer.transform_to_parent.then(*context.parent_transform);
    let mapping = raster_layer_mapping(layer, &transform, context.parent_content_style);
    let styled_content_layer;
    let raster_content_layer = if mapping.draws_plain() {
        &PLAIN_CONTENT_LAYER
    } else {
        styled_content_layer = mapping.content_layer();
        &styled_content_layer
    };
    report_unsupported_effects(&layer.graphics_layer, diagnostics);

    if mapping.transformed_bounds.width <= 0.0 || mapping.transformed_bounds.height <= 0.0 {
        return;
    }

    let visual_clip = resolve_clip(
        context.parent_visual_clip,
        layer
            .visual_clip_rect()
            .map(|clip| transform.bounds_for_rect(clip)),
    );
    let effective_translated_content_context =
        context.inherited_translated_content_context || layer.translated_content_context;
    let (translated_snap_anchor, layer_snap_anchor) =
        layer_snap_anchors(layer, context, &transform, &mapping, raster_content_layer);

    if visual_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }

    // A surface's layer casts its shadow where its parent draws the surface.
    if !context.surface_root {
        push_layer_shadow_of(
            scene,
            layer,
            &mapping,
            &transform,
            context.parent_visual_clip,
            context.light,
        );
        if layer.isolation.explicit_offscreen || !scales_and_moves(&transform) {
            push_layer_surface(layer, scene, context, &transform);
            return;
        }
    }

    let primitive_context = PrimitiveRenderContext {
        layer_bounds: mapping.content_bounds,
        node_layer: raster_content_layer,
        visual_clip,
        motion_context_animated: layer.motion_context_animated,
        content_offset_translation: effective_translated_content_context,
        layer_snap_anchor,
    };
    // A scene draws the surfaces its eye sees in its own space where its
    // first child in 3D comes, far ones first: this renderer keeps no depth
    // between them, and draws a scene behind a doorway flat on its surface.
    let (mut seen, in_space) = layer_space(layer, &transform, context.in_space);
    // Worked out at the first child layer: most layers have none.
    let mut children_same_as_before = None;
    let mut push_child =
        |child: &RenderNode, scene: &mut RasterScene, reuse: &mut Option<Reuse<'_>>| match child {
            RenderNode::Layer(child_layer) => {
                if let Some(surfaces) = seen.take_if(|_| child_layer.has_space()) {
                    flush_copies(reuse, scene);
                    push_seen_surfaces(
                        surfaces,
                        scene,
                        context,
                        (&mapping.content_style, visual_clip),
                    );
                }
                if drawn_by_its_scene(child_layer, in_space) {
                    return;
                }
                let same_as_before = *children_same_as_before.get_or_insert_with(|| {
                    let passed_down = PassedDown {
                        transform,
                        alpha: mapping.content_style.alpha,
                        color_filter: mapping.content_style.color_filter.clone(),
                        clip: visual_clip,
                        snap_anchor: translated_snap_anchor,
                        translated: effective_translated_content_context,
                    };
                    note_passed_down(
                        scene,
                        record,
                        passed_down,
                        context,
                        passed_down_before,
                        reuse,
                    )
                });
                populate_draws_from_graph(
                    child_layer,
                    scene,
                    &RasterTraversalContext {
                        parent_transform: &transform,
                        parent_content_style: &mapping.content_style,
                        parent_visual_clip: visual_clip,
                        inherited_translated_snap_anchor: translated_snap_anchor,
                        inherited_translated_content_context: effective_translated_content_context,
                        diagnostics,
                        light: context.light,
                        same_as_before,
                        text: context.text,
                        surface_root: false,
                        in_space,
                    },
                    reuse,
                );
            }
            _ => {
                flush_copies(reuse, scene);
                render_draw_child(scene, child, primitive_context);
            }
        };
    for_each_in_draw_order(&layer.children, |child| push_child(child, scene, reuse));
}

/// The surfaces `layer` draws for the eye of the scene it shows, if it shows
/// one (see [`seen_surfaces`]), and whether its children lie in a scene's
/// space, given whether `layer` does.
fn layer_space<'a>(
    layer: &'a LayerNode,
    transform: &ProjectiveTransform,
    in_space: bool,
) -> (Option<Vec<(&'a LayerNode, ProjectiveTransform)>>, bool) {
    if layer.graphics_layer.space.is_none() {
        return (None, in_space);
    }
    let seen = seen_surfaces(layer, transform);
    let in_space = in_space || seen.is_some();
    (seen, in_space)
}

/// Whether a scene draws `layer` from its eye's view: so it does for a layer
/// in 3D that lies in a scene's space, as `in_space` says, and shows no scene
/// of its own. Such layers draw where the scene's first child in 3D comes.
fn drawn_by_its_scene(layer: &LayerNode, in_space: bool) -> bool {
    in_space
        && layer.graphics_layer.space.is_some()
        && cranpose_render_common::space::scene_of(layer).is_none()
}

/// The snap anchor `layer`'s child layers inherit and the one its own draws
/// take.
fn layer_snap_anchors(
    layer: &LayerNode,
    context: &RasterTraversalContext<'_>,
    transform: &ProjectiveTransform,
    mapping: &RasterLayerMapping,
    content_layer: &GraphicsLayer,
) -> (Option<Point>, Option<Point>) {
    let effective_translated_content_context =
        context.inherited_translated_content_context || layer.translated_content_context;
    let allow_rigid_snap = effective_translated_content_context || !layer.motion_context_animated;
    let boundary_snap_anchor = if !context.inherited_translated_content_context
        && layer.translated_content_context
        && allow_rigid_snap
    {
        rigid_snap_anchor(
            transform.bounds_for_rect(layer.local_bounds.translate(
                layer.translated_content_offset.x,
                layer.translated_content_offset.y,
            )),
            content_layer,
        )
    } else {
        None
    };
    let translated_snap_anchor = context
        .inherited_translated_snap_anchor
        .or(boundary_snap_anchor);
    let layer_snap_anchor = translated_snap_anchor.or_else(|| {
        if allow_rigid_snap && layer_needs_rigid_snap(layer, effective_translated_content_context) {
            rigid_snap_anchor(mapping.layer_bounds.raster_rect(), content_layer)
        } else {
            None
        }
    });
    (translated_snap_anchor, layer_snap_anchor)
}

/// Notes in `record` what its layer passes down to its child layers, and
/// returns whether that is what it passed in the frame before, which the old
/// scene holds at `before`, under an unchanged walk above.
fn note_passed_down(
    scene: &mut RasterScene,
    record: usize,
    passed_down: PassedDown,
    context: &RasterTraversalContext<'_>,
    before: Option<usize>,
    reuse: &Option<Reuse<'_>>,
) -> bool {
    let same = context.same_as_before
        && before.and_then(|at| reuse.as_ref()?.old.passed_down.get(at)) == Some(&passed_down);
    scene.note_passed_down(record, passed_down);
    same
}

/// Pushes the items of `child`, a primitive or draw run of a layer.
fn render_draw_child(
    scene: &mut RasterScene,
    child: &RenderNode,
    context: PrimitiveRenderContext<'_>,
) {
    match child {
        RenderNode::Primitive(primitive) => render_graph_primitive(scene, primitive, context),
        RenderNode::DrawRun(run) => render_graph_draw_run(scene, run, context),
        RenderNode::Layer(_) => {}
    }
}

fn render_graph_draw_run(
    scene: &mut RasterScene,
    run: &cranpose_render_common::graph::DrawRunNode,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene.counts();
    run.for_each_primitive(|primitive| {
        push_draw_primitive(
            &primitive,
            rect,
            context.node_layer,
            context.visual_clip,
            scene,
            None,
            context.motion_context_animated || context.content_offset_translation,
        );
    });
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
}

fn render_graph_primitive(
    scene: &mut RasterScene,
    primitive: &PrimitiveEntry,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene.counts();
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            let effective_clip = resolve_primitive_clip(
                draw.clip,
                rect,
                context.node_layer,
                context.visual_clip,
                PrimitiveClipSpace::LayerTransformed,
            );
            if effective_clip.is_some_and(|clip| clip.is_empty()) {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
                rect,
                context.node_layer,
                effective_clip,
                scene,
                None,
                context.motion_context_animated || context.content_offset_translation,
            );
        }
        PrimitiveNode::Text(text) => {
            render_graph_text(
                scene,
                text,
                context.layer_bounds,
                context.node_layer,
                context.visual_clip,
            );
        }
    }
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
}

// Out of line for the walk's stack, as `push_layer_shadow_of` tells.
#[inline(never)]
fn render_graph_text(
    scene: &mut RasterScene,
    text: &TextPrimitiveNode,
    layer_bounds: RasterLayerBounds,
    node_layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
) {
    let rect = layer_bounds.raster_rect();
    let text_rect = Rect {
        x: rect.x + text.rect.x,
        y: rect.y + text.rect.y,
        width: text.rect.width,
        height: text.rect.height,
    };
    let text_clip = resolve_primitive_clip(
        text.clip,
        rect,
        node_layer,
        visual_clip,
        PrimitiveClipSpace::LayerTransformed,
    );
    if text_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }

    let placement = TextPlacement {
        bounds: rect,
        text_rect,
        layer: node_layer,
        clip: text_clip,
    };
    push_text_style_draws(scene, text, placement);
}

pub(crate) fn push_draw_primitive(
    primitive: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
    blend_mode: Option<BlendMode>,
    motion_context_animated: bool,
) {
    struct SceneEmitter<'a> {
        scene: &'a mut RasterScene,
    }

    impl DrawPrimitiveSink for SceneEmitter<'_> {
        fn push_shape(&mut self, params: ShapeDrawParams) {
            self.scene.push_shape_params(params);
        }

        fn push_image(&mut self, params: ImageDrawParams) {
            self.scene.push_image(ImageDraw {
                rect: params.rect,
                snap_anchor: None,
                image: params.image,
                alpha: params.alpha,
                color_filter: params.color_filter,
                sampling: params.sampling,
                clip: params.clip,
                blend_mode: params.blend_mode,
                src_rect: params.src_rect,
                transform: None,
            });
        }

        fn push_shadow(
            &mut self,
            shadow_primitive: &cranpose_ui_graphics::ShadowPrimitive,
            layer_bounds: Rect,
            layer: &GraphicsLayer,
            clip: Option<Rect>,
        ) {
            push_shadow_primitive(shadow_primitive, layer_bounds, layer, clip, self.scene);
        }

        fn push_text(&mut self, params: TextDrawParams) {
            self.scene.push_text(TextDraw {
                rect: params.rect,
                snap_anchor: None,
                text: params.text,
                layout: params.layout,
                ink: GlyphInk::Painted,
                color: params.color,
                text_style: params.text_style,
                font_size: params.font_size,
                scale: params.scale,
                clip: params.clip,
            });
        }
    }

    let mut emitter = SceneEmitter { scene };
    emit_draw_primitive(
        primitive,
        layer_bounds,
        layer,
        clip,
        &mut emitter,
        blend_mode,
        motion_context_animated,
    );
}

fn push_shadow_primitive(
    shadow_prim: &cranpose_ui_graphics::ShadowPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
) {
    fn shape_pair_for_primitive(
        prim: &DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        blend_mode: BlendMode,
    ) -> Option<(crate::scene::DrawShape, BlendMode)> {
        let params = draw_shape_params_for_primitive(prim, layer_bounds, layer, None, blend_mode)?;
        let blend_mode = params.blend_mode;
        Some((crate::scene::DrawShape::of_params(params), blend_mode))
    }

    match shadow_prim {
        cranpose_ui_graphics::ShadowPrimitive::Drop {
            shape,
            cutout,
            blur_radius,
            blend_mode,
        } => {
            let Some(shape_pair) =
                shape_pair_for_primitive(shape, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let cutout_pair = match cutout {
                Some(cutout) => {
                    let Some(pair) =
                        shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
                    else {
                        return;
                    };
                    Some(pair)
                }
                None => None,
            };
            let Some(cutout_pair) = cutout_pair else {
                push_blurred_shape_samples(scene, &shape_pair.0, shape_pair.1, clip, *blur_radius);
                return;
            };
            let samples = blur_samples(blur_radius.max(1.0));
            if samples.is_empty() {
                scene.push_shape(
                    shape_pair.0.rect,
                    shape_pair.0.brush,
                    shape_pair.0.outline.rounded(),
                    clip,
                    shape_pair.1,
                );
                scene.push_shape(
                    cutout_pair.0.rect,
                    cutout_pair.0.brush,
                    cutout_pair.0.outline.rounded(),
                    clip,
                    cutout_pair.1,
                );
                return;
            }
            for sample in samples.iter().rev() {
                scene.push_shape(
                    expanded_shape_rect(&shape_pair.0, sample.expansion),
                    scale_brush_alpha(shape_pair.0.brush.clone(), sample.weight),
                    shape_pair.0.outline.rounded(),
                    clip,
                    shape_pair.1,
                );
                scene.push_shape(
                    expanded_shape_rect(&cutout_pair.0, sample.expansion),
                    scale_brush_alpha(cutout_pair.0.brush.clone(), sample.weight),
                    cutout_pair.0.outline.rounded(),
                    clip,
                    cutout_pair.1,
                );
            }
        }
        cranpose_ui_graphics::ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            blend_mode,
            clip_rect,
        } => {
            let Some(fill_pair) = shape_pair_for_primitive(fill, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let Some(cutout_pair) =
                shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
            else {
                return;
            };
            let abs_clip = Rect {
                x: clip_rect.x + layer_bounds.x,
                y: clip_rect.y + layer_bounds.y,
                width: clip_rect.width,
                height: clip_rect.height,
            };
            let transformed_clip = apply_layer_to_rect(abs_clip, layer_bounds, layer);
            let effective_clip = clip.map_or(Some(transformed_clip), |parent_clip| {
                parent_clip.intersect(transformed_clip)
            });
            let samples = blur_samples(blur_radius.max(1.0));
            if samples.is_empty() {
                scene.push_shape(
                    fill_pair.0.rect,
                    fill_pair.0.brush,
                    fill_pair.0.outline.rounded(),
                    effective_clip,
                    fill_pair.1,
                );
                scene.push_shape(
                    cutout_pair.0.rect,
                    cutout_pair.0.brush,
                    cutout_pair.0.outline.rounded(),
                    effective_clip,
                    cutout_pair.1,
                );
                return;
            }

            for sample in samples.iter().rev() {
                scene.push_shape(
                    expanded_shape_rect(&fill_pair.0, sample.expansion),
                    scale_brush_alpha(fill_pair.0.brush.clone(), sample.weight),
                    fill_pair.0.outline.rounded(),
                    effective_clip,
                    fill_pair.1,
                );
                scene.push_shape(
                    expanded_shape_rect(&cutout_pair.0, sample.expansion),
                    scale_brush_alpha(cutout_pair.0.brush.clone(), sample.weight),
                    cutout_pair.0.outline.rounded(),
                    effective_clip,
                    cutout_pair.1,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/pipeline_tests.rs"]
mod tests;
