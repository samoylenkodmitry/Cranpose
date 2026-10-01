use cranpose_core::{NodeId, collections::map::HashMap};
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
    BlendMode, CompositingStrategy, DrawPrimitive, GraphicsLayer, LayerShape, Point, RecordLane,
    Rect, RenderEffect, expand_rect, primitive_coverage_rect,
};

use crate::{
    pipeline::{TextLayoutResolver, push_draw_primitive, push_layer_shadow, push_text_style_draws},
    scene::{
        BackdropLayer, CompositorScene, LayerRoundedClip, Placement as RunPlacement, RunDraw,
        SceneCapacityHint, ShadowDraw, SnapAnchor,
    },
};

const AFFINE_TOLERANCE: f32 = 1e-4;
const ROUNDED_CLIP_AA_MARGIN: f32 = 1.0;
const ANIMATED_RASTER_STEPS_PER_OCTAVE: f32 = 8.0;

#[derive(Default)]
pub(crate) struct LayerMotion {
    previous: HashMap<NodeId, Motion>,
    current: HashMap<NodeId, Motion>,
}

#[derive(Clone, Copy)]
struct Motion {
    content_hash: u64,
    raster: f32,
}

impl LayerMotion {
    pub(crate) fn raster_scale(
        &mut self,
        node_id: Option<NodeId>,
        scale: f32,
        content_hash: u64,
        cacheable: bool,
    ) -> f32 {
        let Some(node_id) = node_id else {
            return scale;
        };
        let scaling = self
            .previous
            .get(&node_id)
            .filter(|previous| previous.content_hash == content_hash);
        let raster = match scaling {
            Some(previous) if cacheable && scale.is_finite() && scale > 0.0 => {
                held_raster_scale(previous.raster, scale)
            }
            _ => scale,
        };
        self.current.insert(
            node_id,
            Motion {
                content_hash,
                raster,
            },
        );
        raster
    }

    pub(crate) fn end_frame(&mut self) {
        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
    }
}

fn held_raster_scale(held: f32, scale: f32) -> f32 {
    if held * (1.0 / ANIMATED_RASTER_STEPS_PER_OCTAVE).exp2() >= scale && held <= scale * 2.0 {
        held
    } else {
        animated_raster_scale(scale)
    }
}

pub(crate) fn animated_raster_scale(scale: f32) -> f32 {
    let step =
        (scale.log2() * ANIMATED_RASTER_STEPS_PER_OCTAVE).ceil() / ANIMATED_RASTER_STEPS_PER_OCTAVE;
    step.exp2().max(scale)
}

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
/// `blend_mode` and the optional rounded mask. A child `in_place` can do
/// without the texture: its content can draw straight into its parent's pass
/// at its raster scale, under the turn and move its transform leaves.
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
    pub(crate) in_place: bool,
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
    /// The corner radius `visual_clip` is rounded with; `0.0` for a rect.
    clip_radius: f32,
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
    layer.children.iter().any(|child| match child {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(_) => true,
            PrimitiveNode::Draw(draw) => {
                primitive_is_pixel_sensitive(&draw.primitive)
                    || (translated && primitive_is_drawn(&draw.primitive))
            }
        },
        RenderNode::DrawRun(run) => {
            run.summary.has_text
                || run.summary.has_pixel_sensitive
                || (translated && run.summary.has_non_shadow)
        }
        RenderNode::Layer(_) => false,
    })
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
            .visual_clip_rect()
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
                primitive_stays_clear(&draw.primitive, |rect| check(rect, draw.clip))
            }
            PrimitiveNode::Text(text) => check(text.rect, text.clip),
        },
        RenderNode::DrawRun(run) => {
            !run.summary.has_shadow && run.coverage_rects().all(|rect| check(rect, None))
        }
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

/// The scale of a `transform` that only scales evenly, turns and moves:
/// affine, with a linear part that is an orthonormal one times the scale.
/// Content drawn at that scale under the rest of the transform resamples
/// nothing.
pub(crate) fn similarity_scale(transform: ProjectiveTransform) -> Option<f32> {
    let [[a, b, _], [c, d, _], perspective] = transform.matrix();
    let near = |value: f32, target: f32| (value - target).abs() <= AFFINE_TOLERANCE;
    let squared = a * a + c * c;
    (near(perspective[0], 0.0)
        && near(perspective[1], 0.0)
        && near(perspective[2], 1.0)
        && squared.is_normal()
        && near((b * b + d * d) / squared, 1.0)
        && near((a * b + c * d) / squared, 0.0))
    .then(|| squared.sqrt())
}

/// The scale a layer whose transform scales by `scale` draws its content at
/// in place, against its parent's, for a layer rasterized at `raster_scale`:
/// one that keeps content's size draws it at the parent's scale, and one
/// that scales at the scale its surface would raster. `None` while its scale
/// moves within a raster it holds, which it keeps compositing.
pub(crate) fn in_place_content_scale(scale: f32, raster_scale: f32) -> Option<f32> {
    if (scale - 1.0).abs() <= AFFINE_TOLERANCE {
        Some(1.0)
    } else {
        ((scale - raster_scale).abs() <= AFFINE_TOLERANCE).then_some(raster_scale)
    }
}

/// Whether an isolated layer can draw its content straight into its parent's
/// pass: its transform only scales evenly at its raster scale, turns and
/// moves it, nothing else about it needs a surface, and its content draws
/// the same under a transform.
fn can_draw_in_place(
    layer: &LayerNode,
    transform: ProjectiveTransform,
    raster_scale: f32,
    content: &LayerScene,
) -> bool {
    similarity_scale(transform)
        .and_then(|scale| in_place_content_scale(scale, raster_scale))
        .is_some()
        && !child_needs_surface(layer)
        && layer.backdrop().is_none()
        && rounded_clip_for_layer(layer).is_none()
        && content_draws_in_place(content)
}

/// Whether every part of a layer's content draws the same straight into a
/// transformed pass as into a surface of its own: no backdrop, effect range
/// or shadow that resolves into a texture, nothing that blends other than
/// source-over (it would reach the pixels beneath the layer), no image (its
/// quad's edges are not anti-aliased, where a turned surface filters them),
/// shadow texts their clips leave whole (a clip turned off the pixel grid is
/// no scissor; a text's own glyphs are cut to its clip before the turn), and
/// children that draw in place unclipped.
fn content_draws_in_place(content: &LayerScene) -> bool {
    let scene = &content.scene;
    let whole =
        |rect: Rect, clip: Option<Rect>| clip.is_none_or(|clip| clip.intersect(rect) == Some(rect));
    let source_over = |run: &RunDraw| {
        run.segment_records()
            .all(|segment| segment.blend == BlendMode::SrcOver)
    };
    scene.backdrop_layers.is_empty()
        && scene.effect_layers.is_empty()
        && scene.runs.iter().all(source_over)
        && scene.shadow_draws.iter().all(|shadow| {
            !shadow.requires_surface()
                && shadow.texts.iter().all(|text| whole(text.rect, text.clip))
        })
        && scene.images.is_empty()
        && content
            .children
            .iter()
            .all(|child| child.in_place && child.clip.is_none())
}

#[derive(Clone, Copy)]
enum Placement {
    Direct(Point),
    /// Drawn in place under its own clip rounded at the radius, every
    /// shape in the clip's corners taking the clip's coverage.
    DirectRounded(Point, f32),
    Isolated,
}

fn child_placement(layer: &LayerNode) -> Placement {
    let Some(translation) = direct_translation(layer.transform_to_parent) else {
        return Placement::Isolated;
    };
    if child_needs_surface(layer) {
        return Placement::Isolated;
    }
    match rounded_clip_for_layer(layer) {
        Some(clip) if !content_admits_rounded_clip(layer, clip) => {
            content_takes_rounded_clip(layer, clip).map_or(Placement::Isolated, |radius| {
                Placement::DirectRounded(translation, radius)
            })
        }
        _ => Placement::Direct(translation),
    }
}

/// The radius the layer's clip keeps: the rounding it entered with, while
/// its own clip, `layer_clip`, holds the rounded one. A clip that cuts the
/// rounded one is a rect nothing under it may round; what reaches its
/// corners kept the rounded layer on a surface.
fn radius_within(layer_clip: Option<Rect>, context: &WalkContext) -> f32 {
    let keeps = match (layer_clip, context.visual_clip) {
        (None, _) => true,
        (Some(own), Some(rounded)) => clip_holds(own, rounded),
        (Some(_), None) => false,
    };
    if keeps { context.clip_radius } else { 0.0 }
}

/// Edges this close count as one: a clip met through other sums lands a few
/// ulps away from the rect it bounds.
const CLIP_HOLD_SLACK: f32 = 1e-3;

/// Whether `outer` holds `inner` whole, its edges taken within
/// [`CLIP_HOLD_SLACK`].
fn clip_holds(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - CLIP_HOLD_SLACK
        && inner.y >= outer.y - CLIP_HOLD_SLACK
        && inner.x + inner.width <= outer.x + outer.width + CLIP_HOLD_SLACK
        && inner.y + inner.height <= outer.y + outer.height + CLIP_HOLD_SLACK
}

/// How `child` places under `context`: a rounded clip draws in place only
/// where nothing above clips into it, and never inside another rounded
/// clip, whose radius a placement could not carry beside its own.
fn placement_in(child: &LayerNode, context: &WalkContext) -> Placement {
    match child_placement(child) {
        Placement::DirectRounded(translation, radius) => {
            let clip = child.visual_clip_rect().map(|clip| {
                clip.translate(
                    context.offset.x + translation.x,
                    context.offset.y + translation.y,
                )
            });
            let whole = context
                .visual_clip
                .is_none_or(|outer| clip.is_some_and(|clip| clip_holds(outer, clip)));
            if whole && context.clip_radius == 0.0 {
                Placement::DirectRounded(translation, radius)
            } else {
                Placement::Isolated
            }
        }
        placement => placement,
    }
}

/// The radius a layer whose rounded clip cuts into its content draws in
/// place with, its records taking the clip's coverage as its surface's
/// composite would: a uniform one, with nothing in the corners but shapes
/// drawn straight under that clip. Text, images, shadows, layers that
/// composite and anything under a clip of its own stay out of the corners,
/// which the rect clip alone then leaves whole.
fn content_takes_rounded_clip(layer: &LayerNode, clip: LayerRoundedClip) -> Option<f32> {
    let radius = clip.radii[0];
    let uniform = clip
        .radii
        .iter()
        .all(|corner| (corner - radius).abs() <= AFFINE_TOLERANCE);
    (uniform
        && layer.visual_clip_rect() == Some(clip.rect)
        && layer_takes_corners(layer, Point::default(), &RoundedClipCorners::of(clip)))
    .then_some(radius)
}

fn layer_takes_corners(layer: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    layer.children.iter().all(|node| match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) if draw.clip.is_none() && is_shape(&draw.primitive) => true,
            PrimitiveNode::Draw(draw) => {
                primitive_stays_clear(&draw.primitive, |rect| stays_clear(rect, offset, corners))
            }
            PrimitiveNode::Text(text) => stays_clear(text.rect, offset, corners),
        },
        RenderNode::DrawRun(run) => run_takes_corners(run, offset, corners),
        RenderNode::Layer(child) => child_takes_corners(child, offset, corners),
    })
}

fn child_takes_corners(child: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    let plain = child.visual_clip_rect().is_none()
        && rounded_clip_for_layer(child).is_none()
        && child.graphics_layer.shadow_elevation <= 0.0
        && !child_needs_surface(child);
    match direct_translation(child.transform_to_parent) {
        Some(translation) if plain => layer_takes_corners(
            child,
            Point::new(offset.x + translation.x, offset.y + translation.y),
            corners,
        ),
        _ => {
            child.draws_within_bounds
                && child.graphics_layer.shadow_elevation <= 0.0
                && stays_clear(
                    quad_bounds(child.transform_to_parent.map_rect(child.local_bounds)),
                    offset,
                    corners,
                )
        }
    }
}

/// Whether a run's shapes may take the corners: its other lanes (text,
/// images, shadows) stay out of them.
fn run_takes_corners(run: &DrawRunNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    let recording = &*run.recording;
    recording
        .segments_in(&run.segments)
        .all(|segment| match segment.lane {
            RecordLane::Shapes | RecordLane::Content => true,
            RecordLane::Others => recording.others()[segment.range()].iter().all(|primitive| {
                primitive_stays_clear(primitive, |rect| stays_clear(rect, offset, corners))
            }),
        })
}

/// Whether `primitive` stays out of a rounded clip's corner cuts, as
/// `clear` judges its coverage rect. A shadow has none: its blur reaches
/// past any rect it holds, and it draws under a rect clip, so it never
/// counts as clear. Only the content marker, which draws nothing, does.
fn primitive_stays_clear(primitive: &DrawPrimitive, clear: impl Fn(Rect) -> bool) -> bool {
    match primitive_coverage_rect(primitive) {
        Some(rect) => clear(rect),
        None => matches!(primitive, DrawPrimitive::Content),
    }
}

fn stays_clear(rect: Rect, offset: Point, corners: &RoundedClipCorners) -> bool {
    corners.admits(expand_rect(
        rect.translate(offset.x, offset.y),
        ROUNDED_CLIP_AA_MARGIN,
    ))
}

/// Whether a primitive draws as a shape record, which takes a rounded clip's
/// coverage in the shape shader.
fn is_shape(primitive: &DrawPrimitive) -> bool {
    matches!(
        primitive,
        DrawPrimitive::Rect { .. }
            | DrawPrimitive::RoundRect { .. }
            | DrawPrimitive::Arc { .. }
            | DrawPrimitive::Line { .. }
    )
}

pub(crate) fn collect_root(
    root: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    capacity: SceneCapacityHint,
) -> LayerScene {
    let mut out = LayerScene {
        scene: CompositorScene::with_capacity(capacity),
        children: Vec::new(),
    };
    let context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        clip_radius: 0.0,
        snap_anchor: None,
        translated: false,
    };
    collect_child(root, text_layout, motion, context, &mut out);
    out.scene.flush_loose();
    motion.end_frame();
    out
}

pub(crate) fn collect_overlay(
    root: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
) -> LayerScene {
    collect_root(
        root,
        text_layout,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
    )
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
            .visual_clip_rect()
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
        reach: context.visual_clip,
        rounded_clip,
        snap_anchor,
        effect: effect.clone(),
        z_index: 0,
    });
}

fn isolated_child(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    context: WalkContext,
    parent_scene: &mut CompositorScene,
) -> ChildLayer {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let content_context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        clip_radius: 0.0,
        snap_anchor: None,
        translated: context.translated || layer.translated_content_context,
    };
    let mut content = LayerScene {
        scene: CompositorScene::new(),
        children: Vec::new(),
    };
    collect_into(layer, text_layout, motion, content_context, &mut content);
    content.scene.flush_loose();
    let transform = layer
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let parent_bounds = quad_bounds(transform.map_rect(layer.local_bounds));
    let rigid = context.translated || layer_has_pixel_sensitive_subtree(layer);
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
    let content_hash = layer.target_content_hash();
    let cacheable = layer.cache_policy == CachePolicy::Auto && !content.contains_backdrop();
    let surface_scale = motion.raster_scale(
        layer.node_id,
        layer_uniform_scale(&layer.graphics_layer),
        content_hash,
        cacheable,
    );
    let in_place = can_draw_in_place(layer, transform, surface_scale, &content);
    ChildLayer {
        z_index: parent_scene.next_z(),
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
        surface_scale,
        content_hash,
        cache_policy: layer.cache_policy,
        in_place,
        content,
    }
}

fn with_backdrop_in_own_space(child: ChildLayer) -> ChildLayer {
    if child.backdrop.is_none() || uniform_scale_translation(child.transform).is_some() {
        return child;
    }
    let mut content = LayerScene {
        scene: CompositorScene::new(),
        children: Vec::new(),
    };
    let inner_z = content.scene.next_z();
    content.scene.next_z += 1;
    let mut outer = ChildLayer {
        z_index: child.z_index,
        node_id: child.node_id,
        local_bounds: child.local_bounds,
        transform: child.transform,
        clip: child.clip,
        rounded_clip: None,
        alpha: child.alpha,
        blend_mode: child.blend_mode,
        effect: None,
        backdrop: None,
        snap_anchor: child.snap_anchor,
        surface_scale: child.surface_scale,
        content_hash: child.content_hash,
        cache_policy: child.cache_policy,
        in_place: false,
        content,
    };
    outer.content.children.push(ChildLayer {
        z_index: inner_z,
        transform: ProjectiveTransform::identity(),
        clip: None,
        alpha: 1.0,
        blend_mode: BlendMode::SrcOver,
        snap_anchor: None,
        in_place: false,
        ..child
    });
    outer
}

fn detach_flat_backdrop(child: &mut ChildLayer, scene: &mut CompositorScene) {
    if child.alpha != 1.0
        || child.blend_mode != BlendMode::SrcOver
        || child.content.contains_backdrop()
    {
        return;
    }
    let Some(offset) = direct_translation(child.transform) else {
        return;
    };
    let Some(effect) = child.backdrop.take() else {
        return;
    };
    scene.push_backdrop_layer(BackdropLayer {
        node_id: child.node_id,
        rect: child.local_bounds.translate(offset.x, offset.y),
        clip: child.clip,
        reach: None,
        rounded_clip: child.rounded_clip.map(|clip| LayerRoundedClip {
            rect: clip.rect.translate(offset.x, offset.y),
            radii: clip.radii,
        }),
        snap_anchor: child.snap_anchor,
        effect,
        z_index: 0,
    });
    child.z_index = scene.next_z();
}

fn collect_into(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    context: WalkContext,
    out: &mut LayerScene,
) {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let layer_bounds = layer
        .local_bounds
        .translate(context.offset.x, context.offset.y);
    let layer_clip = layer
        .visual_clip_rect()
        .map(|clip| clip.translate(context.offset.x, context.offset.y));
    let visual_clip = resolve_clip(context.visual_clip, layer_clip);
    if visual_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }
    let clip_radius = radius_within(layer_clip, &context);
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
        // The node's own draws and text sit in its local space, whose origin
        // is the node's, even where its layer is bounded at a coordinator
        // away from it.
        layer_bounds: layer
            .node_rect()
            .translate(context.offset.x, context.offset.y),
        local_layer: &local_layer,
        visual_clip,
        clip_radius,
        anchor: layer_anchor,
        motion_context_animated: layer.motion_context_animated || translated,
    };
    let mut first_deferred = layer.children.len();

    for (index, child) in layer.children.iter().enumerate() {
        match child {
            RenderNode::Layer(child_layer) => {
                let child_context = WalkContext {
                    offset: context.offset,
                    visual_clip,
                    clip_radius,
                    snap_anchor: translated_anchor,
                    translated,
                };
                collect_child(child_layer, text_layout, motion, child_context, out);
            }
            _ if content_phase(child) == PrimitivePhase::AfterChildren => {
                first_deferred = first_deferred.min(index);
            }
            _ => push_content(out, text_layout, child, &content),
        }
    }

    for child in &layer.children[first_deferred..] {
        if content_phase(child) == PrimitivePhase::AfterChildren {
            push_content(out, text_layout, child, &content);
        }
    }
}

/// Where a layer's own primitives land: the layer's bounds and local
/// graphics layer, the clip and anchor they inherit, and whether their
/// motion context animates.
#[derive(Clone, Copy)]
struct ContentContext<'a> {
    layer_bounds: Rect,
    local_layer: &'a GraphicsLayer,
    visual_clip: Option<Rect>,
    clip_radius: f32,
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
        RenderNode::Primitive(entry) => push_primitive(out, text_layout, entry, content),
        RenderNode::DrawRun(run) => push_draw_run(out, run, content),
        RenderNode::Layer(_) => {}
    }
}

fn collect_child(
    child: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    context: WalkContext,
    out: &mut LayerScene,
) {
    match placement_in(child, &context) {
        placement @ (Placement::Direct(translation) | Placement::DirectRounded(translation, _)) => {
            let child_offset = Point::new(
                context.offset.x + translation.x,
                context.offset.y + translation.y,
            );
            let child_bounds = child.local_bounds.translate(child_offset.x, child_offset.y);
            if clipped_away(child, child_bounds, context.visual_clip) {
                return;
            }
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
            let (visual_clip, clip_radius) = match placement {
                Placement::DirectRounded(_, radius) => (
                    resolve_clip(
                        context.visual_clip,
                        child
                            .visual_clip_rect()
                            .map(|clip| clip.translate(child_offset.x, child_offset.y)),
                    ),
                    radius,
                ),
                _ => (context.visual_clip, context.clip_radius),
            };
            let child_context = WalkContext {
                offset: child_offset,
                visual_clip,
                clip_radius,
                snap_anchor: child_anchor,
                translated: context.translated,
            };
            if child.backdrop().is_some() {
                push_backdrop_layer(child, child_offset, child_context, &mut out.scene);
            }
            collect_into(child, text_layout, motion, child_context, out);
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
            let mut isolated = with_backdrop_in_own_space(isolated_child(
                child,
                text_layout,
                motion,
                context,
                &mut out.scene,
            ));
            assign_shadow_anchor(&mut out.scene, shadows_before, isolated.snap_anchor);
            detach_flat_backdrop(&mut isolated, &mut out.scene);
            out.children.push(isolated);
            out.scene.next_z += 1;
        }
    }
}

/// Whether nothing `child`, placed at `bounds`, draws can show inside
/// `clip`: its subtree draws within its bounds, it casts no shadow, and those
/// bounds miss the clip.
fn clipped_away(child: &LayerNode, bounds: Rect, clip: Option<Rect>) -> bool {
    let Some(clip) = clip else {
        return false;
    };
    let slack = cranpose_render_common::graph::CONTAINED_DRAW_SLACK;
    child.draws_within_bounds
        && child.graphics_layer.shadow_elevation <= 0.0
        && Rect {
            x: bounds.x - slack,
            y: bounds.y - slack,
            width: bounds.width + slack * 2.0,
            height: bounds.height + slack * 2.0,
        }
        .intersect(clip)
        .is_none()
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
        for run in shadow
            .shapes
            .iter_mut()
            .chain(&mut shadow.post_blur_cutouts)
        {
            run.placement.snap_anchor = Some(anchor);
        }
        for text in &mut shadow.texts {
            text.snap_anchor = Some(anchor);
        }
    }
}

#[derive(Clone, Copy)]
struct SceneCounts {
    images: usize,
    texts: usize,
    shadow_draws: usize,
    effect_layers: usize,
}

fn scene_counts(scene: &CompositorScene) -> SceneCounts {
    SceneCounts {
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

fn push_primitive(
    out: &mut LayerScene,
    text_layout: &mut impl TextLayoutResolver,
    entry: &PrimitiveEntry,
    content: &ContentContext<'_>,
) {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        clip_radius,
        anchor: snap_anchor,
        motion_context_animated,
    } = *content;
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
            if clip.is_some_and(|clip| clip.is_empty()) {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
                layer_bounds,
                local_layer,
                clip,
                if clip == visual_clip {
                    clip_radius
                } else {
                    0.0
                },
                snap_anchor,
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
            if text_clip.is_some_and(|clip| clip.is_empty()) {
                return;
            }
            push_text_style_draws(
                &mut out.scene,
                text_layout,
                text.node_id,
                layer_bounds,
                text_rect,
                local_layer,
                (&text.text, &text.render_text),
                &text.text_style,
                text.font_size,
                text.layout_options,
                text_clip,
                snap_anchor,
            );
        }
    }
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

/// Places a recorded run: every maximal stretch of shape segments becomes
/// one run draw under the layer's placement, and the primitives of the
/// other lane (text, images, shadows) take their own item paths between
/// them, in the order the command drew.
fn push_draw_run(out: &mut LayerScene, run: &DrawRunNode, content: &ContentContext<'_>) {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        clip_radius,
        anchor: snap_anchor,
        motion_context_animated,
    } = *content;
    let counts = scene_counts(&out.scene);
    let placement = RunPlacement::painted(
        Point::new(layer_bounds.x, layer_bounds.y),
        snap_anchor,
        visual_clip,
        local_layer,
    )
    .with_clip_radius(clip_radius);
    let recording = &*run.recording;
    let mut shapes_from: Option<u32> = None;
    let flush_shapes = |out: &mut LayerScene, end: u32, from: &mut Option<u32>| {
        if let Some(start) = from.take() {
            out.scene
                .push_run(RunDraw::of(recording, run.command, start..end, placement));
        }
    };
    for (index, segment) in recording.segments_in(&run.segments).enumerate() {
        let position = run.segments.start + index as u32;
        match segment.lane {
            RecordLane::Shapes => {
                shapes_from.get_or_insert(position);
            }
            RecordLane::Content => {}
            RecordLane::Others => {
                flush_shapes(out, position, &mut shapes_from);
                for primitive in &recording.others()[segment.range()] {
                    push_draw_primitive(
                        primitive,
                        layer_bounds,
                        local_layer,
                        visual_clip,
                        clip_radius,
                        snap_anchor,
                        &mut out.scene,
                        None,
                        motion_context_animated,
                    );
                }
            }
        }
    }
    flush_shapes(out, run.segments.end, &mut shapes_from);
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

#[cfg(test)]
#[path = "tests/collect_tests.rs"]
mod tests;
