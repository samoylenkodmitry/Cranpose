use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{
        CachePolicy, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderNode, quad_bounds,
    },
    layer_composition::{layer_requires_isolation, local_content_layer_for},
    layer_transform::{apply_layer_affine_to_rect, layer_uniform_scale},
    primitive_emit::{PrimitiveClipSpace, resolve_clip, resolve_primitive_clip},
};
use cranpose_ui_graphics::{
    BlendMode, CompositingStrategy, DrawPrimitive, GraphicsLayer, LayerShape, Point, Rect,
    RenderEffect,
};

use crate::{
    pipeline::{TextLayoutResolver, push_draw_primitive, push_layer_shadow, push_text_style_draws},
    scene::{
        BackdropLayer, CompositorScene, LayerRoundedClip, SceneCapacityHint, ShadowDraw, SnapAnchor,
    },
};

const AFFINE_TOLERANCE: f32 = 1e-4;
const ROUNDED_CLIP_AA_MARGIN: f32 = 1.0;

/// One isolated layer's content in that layer's own coordinate space: the flat
/// z-ordered ops, the isolated children composited at their z, and the
/// backdrop effects that read what lies beneath them at their z.
pub(crate) struct LayerScene {
    pub(crate) scene: CompositorScene,
    pub(crate) children: Vec<ChildLayer>,
}

impl LayerScene {
    pub(crate) fn contains_backdrop(&self) -> bool {
        !self.scene.backdrop_layers.is_empty()
            || self
                .children
                .iter()
                .any(|child| child.backdrop.is_some() || child.content.contains_backdrop())
    }
}

/// An isolated child composited into its parent at `z_index`: its content is
/// rendered into its own texture, then drawn with `transform`, `alpha`,
/// `blend_mode` and the optional rounded mask.
pub(crate) struct ChildLayer {
    pub(crate) z_index: usize,
    pub(crate) node_id: Option<NodeId>,
    pub(crate) local_bounds: Rect,
    pub(crate) transform: ProjectiveTransform,
    pub(crate) clip: Option<Rect>,
    pub(crate) rounded_clip: Option<LayerRoundedClip>,
    pub(crate) alpha: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) effect: Option<RenderEffect>,
    pub(crate) backdrop: Option<RenderEffect>,
    pub(crate) snap_anchor: Option<SnapAnchor>,
    pub(crate) surface_scale: f32,
    pub(crate) content_hash: u64,
    pub(crate) cache_policy: CachePolicy,
    pub(crate) content: LayerScene,
}

impl ChildLayer {
    pub(crate) fn reads_backdrop(&self) -> bool {
        self.backdrop.is_some() || self.content.contains_backdrop()
    }
}

#[derive(Clone, Copy)]
struct WalkContext {
    offset: Point,
    visual_clip: Option<Rect>,
    snap_anchor: Option<SnapAnchor>,
    translated: bool,
}

pub(crate) fn direct_translation(transform: ProjectiveTransform) -> Option<Point> {
    uniform_scale_translation(transform)
        .filter(|(scale, _)| (scale - 1.0).abs() <= AFFINE_TOLERANCE)
        .map(|(_, translation)| translation)
}

pub(crate) fn uniform_scale_translation(transform: ProjectiveTransform) -> Option<(f32, Point)> {
    let matrix = transform.matrix();
    let scale = matrix[0][0];
    if scale <= 0.0
        || (matrix[1][1] - scale).abs() > AFFINE_TOLERANCE
        || matrix[0][1].abs() > AFFINE_TOLERANCE
        || matrix[1][0].abs() > AFFINE_TOLERANCE
        || matrix[2][0].abs() > AFFINE_TOLERANCE
        || matrix[2][1].abs() > AFFINE_TOLERANCE
        || (matrix[2][2] - 1.0).abs() > AFFINE_TOLERANCE
    {
        return None;
    }
    Some((scale, Point::new(matrix[0][2], matrix[1][2])))
}

fn graphics_layer_is_rigid(layer: &GraphicsLayer) -> bool {
    (layer.scale - 1.0).abs() <= AFFINE_TOLERANCE
        && (layer.scale_x - 1.0).abs() <= AFFINE_TOLERANCE
        && (layer.scale_y - 1.0).abs() <= AFFINE_TOLERANCE
        && layer.rotation_x.abs() <= AFFINE_TOLERANCE
        && layer.rotation_y.abs() <= AFFINE_TOLERANCE
        && layer.rotation_z.abs() <= AFFINE_TOLERANCE
}

fn rigid_snap_anchor(layer_bounds: Rect, layer: &GraphicsLayer) -> Option<SnapAnchor> {
    if !graphics_layer_is_rigid(layer) {
        return None;
    }
    let mapped = apply_layer_affine_to_rect(layer_bounds, layer_bounds, layer);
    Some(SnapAnchor::rigid(Point::new(mapped.x, mapped.y)))
}

fn primitive_is_pixel_sensitive(primitive: &DrawPrimitive) -> bool {
    match primitive {
        DrawPrimitive::Blend { primitive, .. } => primitive_is_pixel_sensitive(primitive),
        DrawPrimitive::Image { .. } | DrawPrimitive::Text(_) => true,
        _ => false,
    }
}

fn primitive_is_drawn(primitive: &DrawPrimitive) -> bool {
    !matches!(primitive, DrawPrimitive::Content | DrawPrimitive::Shadow(_))
}

/// Whether the layer's own primitives must move as one rigid raster: any text
/// or image, and under a translated content context any drawn primitive at all.
/// Whether any pixel-sensitive draw (text, image) sits in this layer or in a
/// descendant that only translates against it: compositing such a subtree's
/// raster at a fractional device offset resamples every glyph and texel, so
/// the composite must land on whole device pixels.
fn layer_has_pixel_sensitive_subtree(layer: &LayerNode) -> bool {
    layer.children.iter().any(|child| match child {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(_) => true,
            PrimitiveNode::Draw(draw) => primitive_is_pixel_sensitive(&draw.primitive),
        },
        RenderNode::DrawRun(run) => run.summary.has_text || run.summary.has_pixel_sensitive,
        RenderNode::Layer(child) => {
            direct_translation(child.transform_to_parent).is_some()
                && layer_has_pixel_sensitive_subtree(child)
        }
    })
}

fn layer_needs_rigid_snap(layer: &LayerNode, translated: bool) -> bool {
    let mut has_text = false;
    let mut has_drawn = false;
    let mut has_pixel_sensitive = false;
    for child in &layer.children {
        match child {
            RenderNode::Primitive(entry) => match &entry.node {
                PrimitiveNode::Text(_) => {
                    has_text = true;
                    has_drawn = true;
                }
                PrimitiveNode::Draw(draw) => {
                    has_drawn |= primitive_is_drawn(&draw.primitive);
                    has_pixel_sensitive |= primitive_is_pixel_sensitive(&draw.primitive);
                }
            },
            RenderNode::DrawRun(run) => {
                has_drawn |= run.summary.has_non_shadow;
                has_pixel_sensitive |= run.summary.has_pixel_sensitive;
                has_text |= run.summary.has_text;
            }
            RenderNode::Layer(_) => {}
        }
    }
    (translated && has_drawn) || has_text || has_pixel_sensitive
}

pub(crate) fn rounded_clip_for_layer(layer: &LayerNode) -> Option<LayerRoundedClip> {
    if !layer.graphics_layer.clip {
        return None;
    }
    let LayerShape::Rounded(shape) = layer.graphics_layer.shape else {
        return None;
    };
    let radii = shape.resolve(layer.local_bounds.width, layer.local_bounds.height);
    let radii = [
        radii.top_left,
        radii.top_right,
        radii.bottom_left,
        radii.bottom_right,
    ];
    if radii.iter().all(|radius| *radius <= f32::EPSILON) {
        return None;
    }
    Some(LayerRoundedClip {
        rect: layer.local_bounds,
        radii,
    })
}

/// The four corner squares of a rounded rect are the only places a rounded
/// clip differs from its rect clip. Content whose coverage stays inside the
/// corner circles there is clipped identically by both.
pub(crate) struct RoundedClipCorners {
    rect: Rect,
    radii: [f32; 4],
}

impl RoundedClipCorners {
    pub(crate) fn of(clip: LayerRoundedClip) -> Self {
        Self {
            rect: clip.rect,
            radii: clip.radii,
        }
    }

    /// Whether `region` lies inside the rounded rect: for every corner square
    /// it enters, its point farthest from that corner's circle centre is still
    /// within the circle.
    pub(crate) fn admits(&self, region: Rect) -> bool {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.rect;
        let right = x + width;
        let bottom = y + height;
        let corners = [
            (self.radii[0], x, y, 1.0, 1.0),
            (self.radii[1], right, y, -1.0, 1.0),
            (self.radii[2], x, bottom, 1.0, -1.0),
            (self.radii[3], right, bottom, -1.0, -1.0),
        ];
        let region_right = region.x + region.width;
        let region_bottom = region.y + region.height;
        for (radius, corner_x, corner_y, sign_x, sign_y) in corners {
            if radius <= 0.0 {
                continue;
            }
            let square_left = corner_x.min(corner_x + sign_x * radius);
            let square_right = corner_x.max(corner_x + sign_x * radius);
            let square_top = corner_y.min(corner_y + sign_y * radius);
            let square_bottom = corner_y.max(corner_y + sign_y * radius);
            let overlap_left = region.x.max(square_left);
            let overlap_right = region_right.min(square_right);
            let overlap_top = region.y.max(square_top);
            let overlap_bottom = region_bottom.min(square_bottom);
            if overlap_left >= overlap_right || overlap_top >= overlap_bottom {
                continue;
            }
            let centre_x = corner_x + sign_x * radius;
            let centre_y = corner_y + sign_y * radius;
            let farthest_x = if sign_x > 0.0 {
                overlap_left
            } else {
                overlap_right
            };
            let farthest_y = if sign_y > 0.0 {
                overlap_top
            } else {
                overlap_bottom
            };
            let dx = farthest_x - centre_x;
            let dy = farthest_y - centre_y;
            if dx * dx + dy * dy > radius * radius {
                return false;
            }
        }
        true
    }
}

fn primitive_coverage_rect(primitive: &DrawPrimitive) -> Option<Rect> {
    match primitive {
        DrawPrimitive::Blend { primitive, .. } => primitive_coverage_rect(primitive),
        DrawPrimitive::Rect { rect, stroke, .. }
        | DrawPrimitive::RoundRect { rect, stroke, .. }
        | DrawPrimitive::Arc { rect, stroke, .. } => {
            let half_stroke = stroke.as_ref().map_or(0.0, |stroke| stroke.width * 0.5);
            Some(expand_rect(*rect, half_stroke))
        }
        DrawPrimitive::Image { rect, .. } => Some(*rect),
        DrawPrimitive::Text(text) => Some(text.rect),
        DrawPrimitive::Content | DrawPrimitive::Shadow(_) => None,
    }
}

fn expand_rect(rect: Rect, margin: f32) -> Rect {
    Rect {
        x: rect.x - margin,
        y: rect.y - margin,
        width: rect.width + margin * 2.0,
        height: rect.height + margin * 2.0,
    }
}

/// Whether every op the layer would inline into its parent stays out of its
/// rounded clip's corner cuts, so the rect clip alone reproduces the rounded
/// clip exactly. Shadows and isolated child composites carry the rounded
/// mask themselves and are not counted.
fn content_admits_rounded_clip(layer: &LayerNode, clip: LayerRoundedClip) -> bool {
    let corners = RoundedClipCorners::of(clip);
    layer_admits_corners(layer, Point::default(), None, &corners)
}

fn layer_admits_corners(
    layer: &LayerNode,
    offset: Point,
    clip: Option<Rect>,
    corners: &RoundedClipCorners,
) -> bool {
    let inherited_clip = resolve_clip(
        clip,
        layer
            .clip_rect()
            .map(|rect| rect.translate(offset.x, offset.y)),
    );
    let check = |rect: Rect, primitive_clip: Option<Rect>| -> bool {
        let rect = rect.translate(offset.x, offset.y);
        let clip = resolve_clip(
            inherited_clip,
            primitive_clip.map(|clip| clip.translate(offset.x, offset.y)),
        );
        let visible = match clip {
            Some(clip) => rect.intersect(clip),
            None => Some(rect),
        };
        visible.is_none_or(|visible| corners.admits(expand_rect(visible, ROUNDED_CLIP_AA_MARGIN)))
    };
    layer
        .children
        .iter()
        .all(|child| node_admits_corners(child, offset, inherited_clip, corners, &check))
}

fn node_admits_corners(
    node: &RenderNode,
    offset: Point,
    inherited_clip: Option<Rect>,
    corners: &RoundedClipCorners,
    check: &impl Fn(Rect, Option<Rect>) -> bool,
) -> bool {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) => {
                primitive_coverage_rect(&draw.primitive).is_none_or(|rect| check(rect, draw.clip))
            }
            PrimitiveNode::Text(text) => check(text.rect, text.clip),
        },
        RenderNode::DrawRun(run) => run.primitives.iter().all(|primitive| {
            primitive_coverage_rect(primitive).is_none_or(|rect| check(rect, None))
        }),
        RenderNode::Layer(child) => {
            let Some(translation) = direct_translation(child.transform_to_parent) else {
                return true;
            };
            if child_needs_surface(child) {
                return true;
            }
            let child_offset = Point::new(offset.x + translation.x, offset.y + translation.y);
            layer_admits_corners(child, child_offset, inherited_clip, corners)
        }
    }
}

/// Whether the layer's own composition needs its content on a separate
/// texture, before any rounded clip or transform is considered.
fn child_needs_surface(layer: &LayerNode) -> bool {
    let graphics = &layer.graphics_layer;
    layer.isolation.explicit_offscreen
        || layer.isolation.effect
        || layer.isolation.blend_mode
        || layer.isolation.group_opacity
        || graphics.compositing_strategy == CompositingStrategy::Offscreen
        || layer_requires_isolation(graphics)
}

enum Placement {
    Direct(Point),
    Isolated,
}

fn child_placement(layer: &LayerNode) -> Placement {
    let Some(translation) = direct_translation(layer.transform_to_parent) else {
        return Placement::Isolated;
    };
    if child_needs_surface(layer) {
        return Placement::Isolated;
    }
    if let Some(clip) = rounded_clip_for_layer(layer)
        && !content_admits_rounded_clip(layer, clip)
    {
        return Placement::Isolated;
    }
    Placement::Direct(translation)
}

pub(crate) fn collect_root(
    root: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    capacity: SceneCapacityHint,
) -> LayerScene {
    let mut out = LayerScene {
        scene: CompositorScene::with_capacity(capacity),
        children: Vec::new(),
    };
    let context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        snap_anchor: None,
        translated: false,
    };
    collect_child(root, text_layout, context, &mut out);
    out
}

pub(crate) fn collect_overlay(
    root: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
) -> LayerScene {
    collect_root(root, text_layout, SceneCapacityHint::default())
}

fn push_backdrop_layer(
    layer: &LayerNode,
    offset: Point,
    context: WalkContext,
    scene: &mut CompositorScene,
) {
    let Some(effect) = layer.backdrop() else {
        return;
    };
    let rect = layer.local_bounds.translate(offset.x, offset.y);
    let clip = resolve_clip(
        context.visual_clip,
        layer
            .clip_rect()
            .map(|clip| clip.translate(offset.x, offset.y)),
    );
    let rounded_clip = rounded_clip_for_layer(layer).map(|clip| LayerRoundedClip {
        rect: clip.rect.translate(offset.x, offset.y),
        radii: clip.radii,
    });
    let snap_anchor = context
        .snap_anchor
        .or_else(|| rigid_snap_anchor(rect, &local_content_layer_for(&layer.graphics_layer)));
    scene.push_backdrop_layer(BackdropLayer {
        node_id: layer.node_id,
        rect,
        clip,
        rounded_clip,
        snap_anchor,
        effect: effect.clone(),
        z_index: 0,
    });
}

fn isolated_child(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    context: WalkContext,
    parent_scene: &CompositorScene,
) -> ChildLayer {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let content_context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        snap_anchor: None,
        translated: context.translated || layer.translated_content_context,
    };
    let mut content = LayerScene {
        scene: CompositorScene::new(),
        children: Vec::new(),
    };
    collect_into(layer, text_layout, content_context, &mut content);
    let transform = layer
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let parent_bounds = quad_bounds(transform.map_rect(layer.local_bounds));
    let rigid = context.translated
        || layer_needs_rigid_snap(layer, context.translated)
        || layer_has_pixel_sensitive_subtree(layer);
    let snap_anchor = context.snap_anchor.or_else(|| {
        rigid
            .then(|| rigid_snap_anchor(parent_bounds, &local_layer))
            .flatten()
    });
    let alpha = if layer.graphics_layer.compositing_strategy == CompositingStrategy::ModulateAlpha {
        1.0
    } else {
        GraphicsLayer::composite_alpha_8bit(layer.graphics_layer.alpha)
    };
    ChildLayer {
        z_index: parent_scene.next_z,
        node_id: layer.node_id,
        local_bounds: layer.local_bounds,
        transform,
        clip: context.visual_clip,
        rounded_clip: rounded_clip_for_layer(layer),
        alpha,
        blend_mode: layer.graphics_layer.blend_mode,
        effect: layer.effect().cloned(),
        backdrop: layer.backdrop().cloned(),
        snap_anchor,
        surface_scale: layer_uniform_scale(&layer.graphics_layer),
        content_hash: layer.target_content_hash(),
        cache_policy: layer.cache_policy,
        content,
    }
}

fn collect_into(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    context: WalkContext,
    out: &mut LayerScene,
) {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let layer_bounds = layer
        .local_bounds
        .translate(context.offset.x, context.offset.y);
    let layer_clip = layer
        .clip_rect()
        .map(|clip| clip.translate(context.offset.x, context.offset.y));
    let visual_clip = resolve_clip(context.visual_clip, layer_clip);
    if layer_clip.is_some() && context.visual_clip.is_some() && visual_clip.is_none() {
        return;
    }
    let translated = context.translated || layer.translated_content_context;
    let allow_rigid_snap = translated || !layer.motion_context_animated;
    let boundary_anchor =
        if !context.translated && layer.translated_content_context && allow_rigid_snap {
            rigid_snap_anchor(
                layer_bounds.translate(
                    layer.translated_content_offset.x,
                    layer.translated_content_offset.y,
                ),
                &local_layer,
            )
        } else {
            None
        };
    let translated_anchor = context.snap_anchor.or(boundary_anchor);
    let layer_anchor = translated_anchor.or_else(|| {
        (allow_rigid_snap && layer_needs_rigid_snap(layer, translated))
            .then(|| rigid_snap_anchor(layer_bounds, &local_layer))
            .flatten()
    });
    let content = ContentContext {
        layer_bounds,
        local_layer: &local_layer,
        visual_clip,
        anchor: layer_anchor,
        motion_context_animated: layer.motion_context_animated || translated,
    };
    let mut deferred: Vec<&RenderNode> = Vec::new();

    for child in &layer.children {
        match child {
            RenderNode::Layer(child_layer) => {
                let child_context = WalkContext {
                    offset: context.offset,
                    visual_clip,
                    snap_anchor: translated_anchor,
                    translated,
                };
                collect_child(child_layer, text_layout, child_context, out);
            }
            _ if content_phase(child) == PrimitivePhase::AfterChildren => deferred.push(child),
            _ => push_content(out, text_layout, child, &content),
        }
    }

    for child in deferred {
        push_content(out, text_layout, child, &content);
    }
}

/// Where a layer's own primitives land: the layer's bounds and local
/// graphics layer, the clip and anchor they inherit, and whether their
/// motion context animates.
struct ContentContext<'a> {
    layer_bounds: Rect,
    local_layer: &'a GraphicsLayer,
    visual_clip: Option<Rect>,
    anchor: Option<SnapAnchor>,
    motion_context_animated: bool,
}

fn content_phase(node: &RenderNode) -> PrimitivePhase {
    match node {
        RenderNode::Primitive(entry) => entry.phase,
        RenderNode::DrawRun(run) => run.phase,
        RenderNode::Layer(_) => PrimitivePhase::BeforeChildren,
    }
}

fn push_content(
    out: &mut LayerScene,
    text_layout: &mut impl TextLayoutResolver,
    node: &RenderNode,
    content: &ContentContext<'_>,
) {
    match node {
        RenderNode::Primitive(entry) => push_primitive(
            out,
            text_layout,
            entry,
            content.layer_bounds,
            content.local_layer,
            content.visual_clip,
            content.anchor,
            content.motion_context_animated,
        ),
        RenderNode::DrawRun(run) => push_draw_run(
            out,
            run,
            content.layer_bounds,
            content.local_layer,
            content.visual_clip,
            content.anchor,
            content.motion_context_animated,
        ),
        RenderNode::Layer(_) => {}
    }
}

fn collect_child(
    child: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    context: WalkContext,
    out: &mut LayerScene,
) {
    match child_placement(child) {
        Placement::Direct(translation) => {
            let child_offset = Point::new(
                context.offset.x + translation.x,
                context.offset.y + translation.y,
            );
            let child_bounds = child.local_bounds.translate(child_offset.x, child_offset.y);
            let child_local_layer = local_content_layer_for(&child.graphics_layer);
            let child_anchor = context.snap_anchor.or_else(|| {
                context
                    .translated
                    .then(|| rigid_snap_anchor(child_bounds, &child_local_layer))
                    .flatten()
            });
            let shadow_clip = resolve_clip(
                context.visual_clip,
                child
                    .shadow_clip
                    .map(|clip| clip.translate(child_offset.x, child_offset.y)),
            );
            let shadows_before = out.scene.shadow_draws.len();
            push_layer_shadow(
                &mut out.scene,
                &child.graphics_layer,
                child_bounds,
                child_bounds,
                shadow_clip,
            );
            assign_shadow_anchor(&mut out.scene, shadows_before, child_anchor);
            let child_context = WalkContext {
                offset: child_offset,
                visual_clip: context.visual_clip,
                snap_anchor: child_anchor,
                translated: context.translated,
            };
            if child.backdrop().is_some() {
                push_backdrop_layer(child, child_offset, child_context, &mut out.scene);
            }
            collect_into(child, text_layout, child_context, out);
        }
        Placement::Isolated => {
            let transform = child
                .transform_to_parent
                .then(ProjectiveTransform::translation(
                    context.offset.x,
                    context.offset.y,
                ));
            let child_bounds = quad_bounds(transform.map_rect(child.local_bounds));
            let shadow_clip = resolve_clip(
                context.visual_clip,
                child
                    .shadow_clip
                    .map(|clip| quad_bounds(transform.map_rect(clip))),
            );
            let shadows_before = out.scene.shadow_draws.len();
            push_layer_shadow(
                &mut out.scene,
                &child.graphics_layer,
                child.local_bounds,
                child_bounds,
                shadow_clip,
            );
            let isolated = isolated_child(child, text_layout, context, &out.scene);
            assign_shadow_anchor(&mut out.scene, shadows_before, isolated.snap_anchor);
            out.children.push(isolated);
            out.scene.next_z += 1;
        }
    }
}

fn assign_shadow_anchor(
    scene: &mut CompositorScene,
    shadows_before: usize,
    snap_anchor: Option<SnapAnchor>,
) {
    if let Some(anchor) = snap_anchor {
        anchor_shadows(&mut scene.shadow_draws[shadows_before..], anchor);
    }
}

fn anchor_shadows(shadows: &mut [ShadowDraw], anchor: SnapAnchor) {
    for shadow in shadows {
        for (shape, _) in &mut shadow.shapes {
            shape.snap_anchor = Some(anchor);
        }
        for (shape, _) in &mut shadow.post_blur_cutouts {
            shape.snap_anchor = Some(anchor);
        }
        for text in &mut shadow.texts {
            text.snap_anchor = Some(anchor);
        }
    }
}

#[derive(Clone, Copy)]
struct SceneCounts {
    shapes: usize,
    images: usize,
    texts: usize,
    shadow_draws: usize,
    effect_layers: usize,
}

fn scene_counts(scene: &CompositorScene) -> SceneCounts {
    SceneCounts {
        shapes: scene.shapes.len(),
        images: scene.images.len(),
        texts: scene.texts.len(),
        shadow_draws: scene.shadow_draws.len(),
        effect_layers: scene.effect_layers.len(),
    }
}

fn assign_snap_anchor_since(
    scene: &mut CompositorScene,
    counts: SceneCounts,
    snap_anchor: Option<SnapAnchor>,
) {
    let Some(anchor) = snap_anchor else {
        return;
    };
    for shape in &mut scene.shapes[counts.shapes..] {
        shape.snap_anchor = Some(anchor);
    }
    for image in &mut scene.images[counts.images..] {
        image.snap_anchor = Some(anchor);
    }
    for text in &mut scene.texts[counts.texts..] {
        text.snap_anchor = Some(anchor);
    }
    anchor_shadows(&mut scene.shadow_draws[counts.shadow_draws..], anchor);
    for layer in &mut scene.effect_layers[counts.effect_layers..] {
        layer.snap_anchor = Some(anchor);
    }
}

#[allow(clippy::too_many_arguments)]
fn push_primitive(
    out: &mut LayerScene,
    text_layout: &mut impl TextLayoutResolver,
    entry: &PrimitiveEntry,
    layer_bounds: Rect,
    local_layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
    snap_anchor: Option<SnapAnchor>,
    motion_context_animated: bool,
) {
    let counts = scene_counts(&out.scene);
    match &entry.node {
        PrimitiveNode::Draw(draw) => {
            let clip = resolve_primitive_clip(
                draw.clip,
                layer_bounds,
                local_layer,
                visual_clip,
                PrimitiveClipSpace::Local,
            );
            if draw.clip.is_some() && clip.is_none() {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
                layer_bounds,
                local_layer,
                clip,
                &mut out.scene,
                None,
                motion_context_animated,
            );
        }
        PrimitiveNode::Text(text) => {
            let text_rect = text.rect.translate(layer_bounds.x, layer_bounds.y);
            let text_clip = resolve_primitive_clip(
                text.clip,
                layer_bounds,
                local_layer,
                visual_clip,
                PrimitiveClipSpace::Local,
            );
            if text.clip.is_some() && text_clip.is_none() {
                return;
            }
            push_text_style_draws(
                &mut out.scene,
                text_layout,
                text.node_id,
                layer_bounds,
                text_rect,
                local_layer,
                &text.text,
                &text.text_style,
                text.font_size,
                text.layout_options,
                text_clip,
            );
        }
    }
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

fn push_draw_run(
    out: &mut LayerScene,
    run: &DrawRunNode,
    layer_bounds: Rect,
    local_layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
    snap_anchor: Option<SnapAnchor>,
    motion_context_animated: bool,
) {
    let counts = scene_counts(&out.scene);
    for primitive in run.primitives.iter() {
        push_draw_primitive(
            primitive,
            layer_bounds,
            local_layer,
            visual_clip,
            &mut out.scene,
            None,
            motion_context_animated,
        );
    }
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::RoundedCornerShape;

    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn corners(width: f32, height: f32, radius: f32) -> RoundedClipCorners {
        RoundedClipCorners::of(LayerRoundedClip {
            rect: rect(0.0, 0.0, width, height),
            radii: [radius; 4],
        })
    }

    #[test]
    fn content_clear_of_every_corner_square_is_admitted() {
        assert!(corners(200.0, 100.0, 20.0).admits(rect(20.0, 20.0, 160.0, 60.0)));
    }

    #[test]
    fn content_inside_the_corner_circle_is_admitted() {
        assert!(corners(200.0, 100.0, 20.0).admits(rect(14.0, 14.0, 60.0, 60.0)));
    }

    #[test]
    fn content_reaching_the_corner_cut_is_refused() {
        assert!(!corners(200.0, 100.0, 20.0).admits(rect(2.0, 2.0, 60.0, 60.0)));
    }

    #[test]
    fn content_touching_the_edge_between_corners_is_admitted() {
        assert!(corners(200.0, 100.0, 20.0).admits(rect(40.0, 0.0, 100.0, 100.0)));
    }

    #[test]
    fn a_rounded_layer_whose_content_enters_a_corner_isolates() {
        let mut layer = LayerNode {
            local_bounds: rect(0.0, 0.0, 200.0, 100.0),
            graphics_layer: GraphicsLayer {
                clip: true,
                shape: LayerShape::Rounded(RoundedCornerShape::uniform(20.0)),
                ..Default::default()
            },
            ..Default::default()
        };
        layer.children.push(RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![DrawPrimitive::Rect {
                rect: rect(0.0, 0.0, 200.0, 100.0),
                brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
                stroke: None,
            }],
        )));
        assert!(matches!(child_placement(&layer), Placement::Isolated));
        let RenderNode::DrawRun(run) = &mut layer.children[0] else {
            unreachable!()
        };
        *run = DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![DrawPrimitive::Rect {
                rect: rect(14.0, 14.0, 172.0, 72.0),
                brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
                stroke: None,
            }],
        );
        assert!(matches!(child_placement(&layer), Placement::Direct(_)));
    }
}
