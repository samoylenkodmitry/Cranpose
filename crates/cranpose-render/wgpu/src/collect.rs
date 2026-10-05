use std::hash::Hasher;

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::{
    geometry::{blur_reach, blur_reach_for_minimum_scale},
    graph::{
        CachePolicy, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderNode, quad_bounds,
    },
    layer_composition::{
        layer_composite_params, layer_requires_isolation, local_content_layer_for,
    },
    layer_shadow::ShadowLight,
    layer_transform::{apply_layer_affine_to_rect, layer_uniform_scale},
    primitive_emit::{PrimitiveClipSpace, resolve_clip, resolve_primitive_clip},
};
use cranpose_ui_graphics::{
    BlendMode, CompositingStrategy, DrawPrimitive, GraphicsLayer, LayerShape, Point, RecordLane,
    Rect, RenderEffect, ShadowPrimitive, expand_rect, primitive_coverage_rect,
};

use crate::{
    collect_cache::{CollectCache, Siblings, Visit},
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
    fn retained_scale(&self, node_id: Option<NodeId>, scale: f32, content_hash: u64) -> f32 {
        node_id
            .and_then(|id| self.previous.get(&id))
            .filter(|previous| previous.content_hash == content_hash)
            .filter(|_| scale.is_finite() && scale > 0.0)
            .map_or(scale, |previous| held_raster_scale(previous.raster, scale))
    }

    fn record_scale(&mut self, node_id: Option<NodeId>, raster: f32, content_hash: u64) {
        let Some(node_id) = node_id else {
            return;
        };
        self.current.insert(
            node_id,
            Motion {
                content_hash,
                raster,
            },
        );
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
    has_backdrop: bool,
}

#[derive(Default)]
pub(crate) struct LayerSceneRecycler {
    scenes: Vec<LayerScene>,
}

impl LayerSceneRecycler {
    pub(crate) fn take(&mut self, capacity: SceneCapacityHint) -> LayerScene {
        match self.scenes.pop() {
            Some(mut scene) => {
                scene.has_backdrop = false;
                scene
            }
            None => LayerScene::new(CompositorScene::with_capacity(capacity), Vec::new()),
        }
    }

    pub(crate) fn recycle(&mut self, mut scene: LayerScene) {
        while let Some(child) = scene.children.pop() {
            self.recycle(child.content);
        }
        scene.scene.clear();
        scene.has_backdrop = false;
        self.scenes.push(scene);
    }

    pub(crate) fn clear(&mut self) {
        self.scenes.clear();
    }

    pub(crate) fn returned_packet(&mut self) {
        if self.scenes.capacity() > self.scenes.len().saturating_mul(2) {
            self.scenes.shrink_to_fit();
        }
    }
}

impl LayerScene {
    pub(crate) fn new(scene: CompositorScene, children: Vec<ChildLayer>) -> Self {
        let mut layer = Self {
            scene,
            children,
            has_backdrop: false,
        };
        layer.refresh_backdrop_summary();
        layer
    }

    pub(crate) fn contains_backdrop(&self) -> bool {
        self.has_backdrop
    }

    fn refresh_backdrop_summary(&mut self) {
        self.has_backdrop = self
            .scene
            .backdrop_layers
            .iter()
            .any(|layer| layer.alpha != 0.0)
            || self.children.iter().any(ChildLayer::reads_backdrop);
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
    pub(crate) backdrop_alpha: f32,
    pub(crate) snap_anchor: Option<SnapAnchor>,
    pub(crate) surface_scale: f32,
    pub(crate) content_hash: u64,
    pub(crate) cache_policy: CachePolicy,
    pub(crate) in_place: bool,
    pub(crate) content: LayerScene,
}

impl ChildLayer {
    pub(crate) fn reads_backdrop(&self) -> bool {
        (self.backdrop.is_some() && self.backdrop_alpha != 0.0) || self.content.contains_backdrop()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WalkContext {
    offset: Point,
    visual_clip: Option<Rect>,
    /// The corner radius `visual_clip` is rounded with; `0.0` for a rect.
    clip_radius: f32,
    snap_anchor: Option<SnapAnchor>,
    translated: bool,
    raster_scale: RasterScale,
    /// The window's shadow light, in the coordinates this walk collects in.
    light: ShadowLight,
    /// Whether an isolated ancestor decides its snapping by whether this
    /// walk finds text or an image. Only then does a subtree that draws
    /// nothing have to be searched for them.
    wants_pixel_sensitive: bool,
    /// Whether children may reuse the draws they made in earlier frames.
    /// Off under a child that moved, since everything under it moved too.
    reuse_draws: bool,
}

impl WalkContext {
    /// Hashes everything the walk hands a child that its draws depend on,
    /// for [`crate::collect_cache::SegmentKey`].
    pub(crate) fn hash_placement(&self, state: &mut impl Hasher) {
        let Self {
            offset,
            visual_clip,
            clip_radius,
            snap_anchor,
            translated,
            raster_scale,
            light,
            wants_pixel_sensitive,
            reuse_draws,
        } = *self;
        let mut write = |value: f32| state.write_u32(value.to_bits());
        write(offset.x);
        write(offset.y);
        let clip = visual_clip.unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        [clip.x, clip.y, clip.width, clip.height, clip_radius]
            .into_iter()
            .for_each(&mut write);
        let anchor = snap_anchor.map_or([0.0; 3], |anchor| {
            [anchor.origin.x, anchor.origin.y, anchor.device_pixel_step]
        });
        anchor.into_iter().for_each(&mut write);
        let (scale_is_minimum, scale) = match raster_scale {
            RasterScale::Exact(scale) => (false, scale),
            RasterScale::Minimum(scale) => (true, scale),
        };
        let ShadowLight {
            x,
            y,
            z,
            radius,
            pixels_per_unit,
        } = light;
        [scale, x, y, z, radius, pixels_per_unit]
            .into_iter()
            .for_each(&mut write);
        let flags = [
            visual_clip.is_some(),
            snap_anchor.is_some(),
            scale_is_minimum,
            translated,
            wants_pixel_sensitive,
            reuse_draws,
        ]
        .into_iter()
        .enumerate()
        .fold(0u32, |flags, (bit, set)| flags | u32::from(set) << bit);
        state.write_u32(flags);
    }
}

#[derive(Clone, Copy)]
enum RasterScale {
    Exact(f32),
    Minimum(f32),
}

impl RasterScale {
    fn within(self, nominal: f32, retained: f32) -> Self {
        if nominal == 1.0 && retained == 1.0 {
            return self;
        }
        if !nominal.is_finite() || nominal <= 0.0 || !retained.is_finite() || retained <= 0.0 {
            return Self::Minimum(0.0);
        }
        let (Self::Exact(parent) | Self::Minimum(parent)) = self;
        Self::Minimum(parent * nominal.min(retained).min(1.0))
    }

    fn shadow_reach(self, radius: f32) -> f32 {
        match self {
            Self::Exact(scale) => blur_reach(radius, scale),
            Self::Minimum(scale) => blur_reach_for_minimum_scale(radius, scale),
        }
    }

    fn aa_margin(self) -> f32 {
        let scale = match self {
            Self::Exact(scale) if !scale.is_finite() || scale <= 0.0 => 1.0,
            Self::Exact(scale) | Self::Minimum(scale) => scale,
        };
        if scale.is_finite() && scale > 0.0 {
            ROUNDED_CLIP_AA_MARGIN.max(1.0 / scale)
        } else {
            f32::INFINITY
        }
    }
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
    layer.children.iter().any(node_is_pixel_sensitive)
}

fn node_is_pixel_sensitive(node: &RenderNode) -> bool {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(_) => true,
            PrimitiveNode::Draw(draw) => primitive_is_pixel_sensitive(&draw.primitive),
        },
        RenderNode::DrawRun(run) => run.summary.has_text || run.summary.has_pixel_sensitive,
        RenderNode::Layer(child) => {
            direct_translation(child.transform_to_parent).is_some()
                && layer_has_pixel_sensitive_subtree(child)
        }
    }
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
    raster_scale: RasterScale,
    aa_margin: f32,
}

impl RoundedClipCorners {
    fn of(clip: LayerRoundedClip, raster_scale: RasterScale) -> Self {
        Self {
            rect: clip.rect,
            radii: clip.radii,
            raster_scale,
            aa_margin: raster_scale.aa_margin(),
        }
    }

    /// Whether `region` lies inside the rounded rect: for every corner square
    /// it enters, its point farthest from that corner's circle centre is still
    /// within the circle.
    pub(crate) fn admits(&self, region: Rect) -> bool {
        if ![region.x, region.y, region.width, region.height]
            .into_iter()
            .all(f32::is_finite)
        {
            return false;
        }
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

fn content_admits_rounded_clip(
    layer: &LayerNode,
    clip: LayerRoundedClip,
    raster_scale: RasterScale,
) -> bool {
    let corners = RoundedClipCorners::of(clip, raster_scale);
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
        visible.is_none_or(|visible| corners.admits(expand_rect(visible, corners.aa_margin)))
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
                primitive_stays_clear(&draw.primitive, corners.raster_scale, |rect| {
                    check(rect, draw.clip)
                })
            }
            PrimitiveNode::Text(text) => text.draw_bounds().is_some_and(|rect| check(rect, None)),
        },
        RenderNode::DrawRun(run) => {
            run.coverage_rects().all(|rect| check(rect, None))
                && (!run.summary.has_shadow
                    || run_others_stay_clear(run, corners.raster_scale, |rect| check(rect, None)))
        }
        RenderNode::Layer(child) => {
            if child.graphics_layer.shadow_elevation > 0.0 {
                return false;
            }
            let Some(translation) = direct_translation(child.transform_to_parent) else {
                return child_stays_clear(child, offset, corners);
            };
            if child_needs_surface(child) {
                return child_stays_clear(child, offset, corners);
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

fn child_placement(layer: &LayerNode, raster_scale: RasterScale) -> Placement {
    let Some(translation) = direct_translation(layer.transform_to_parent) else {
        return Placement::Isolated;
    };
    if child_needs_surface(layer) {
        return Placement::Isolated;
    }
    match rounded_clip_for_layer(layer) {
        Some(clip) if !content_admits_rounded_clip(layer, clip, raster_scale) => {
            content_takes_rounded_clip(layer, clip, raster_scale)
                .map_or(Placement::Isolated, |radius| {
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
    match child_placement(child, context.raster_scale) {
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
fn content_takes_rounded_clip(
    layer: &LayerNode,
    clip: LayerRoundedClip,
    raster_scale: RasterScale,
) -> Option<f32> {
    let radius = clip.radii[0];
    let uniform = clip
        .radii
        .iter()
        .all(|corner| (corner - radius).abs() <= AFFINE_TOLERANCE);
    (uniform
        && layer.visual_clip_rect() == Some(clip.rect)
        && layer_takes_corners(
            layer,
            Point::default(),
            &RoundedClipCorners::of(clip, raster_scale),
        ))
    .then_some(radius)
}

fn layer_takes_corners(layer: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    layer.children.iter().all(|node| match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) if draw.clip.is_none() && is_shape(&draw.primitive) => true,
            PrimitiveNode::Draw(draw) => {
                primitive_stays_clear(&draw.primitive, corners.raster_scale, |rect| {
                    stays_clear(rect, offset, corners)
                })
            }
            PrimitiveNode::Text(text) => text
                .draw_bounds()
                .is_some_and(|rect| stays_clear(rect, offset, corners)),
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
        _ => child_stays_clear(child, offset, corners),
    }
}

fn child_stays_clear(child: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    if !child.draws_within_bounds || child.graphics_layer.shadow_elevation > 0.0 {
        return false;
    }
    let padding = cranpose_render_common::graph::CONTAINED_DRAW_SLACK
        + child.effect().map_or(0.0, RenderEffect::output_padding);
    stays_clear(
        quad_bounds(
            child
                .transform_to_parent
                .map_rect(expand_rect(child.local_bounds, padding)),
        ),
        offset,
        corners,
    )
}

/// Whether a run's shapes may take the corners: its other lanes (text,
/// images, shadows) stay out of them.
fn run_takes_corners(run: &DrawRunNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    run_others_stay_clear(run, corners.raster_scale, |rect| {
        stays_clear(rect, offset, corners)
    })
}

fn run_others_stay_clear(
    run: &DrawRunNode,
    raster_scale: RasterScale,
    clear: impl Fn(Rect) -> bool,
) -> bool {
    let recording = &*run.recording;
    recording
        .segments_in(&run.segments)
        .all(|segment| match segment.lane {
            RecordLane::Shapes | RecordLane::Content => true,
            RecordLane::Others => recording.others()[segment.range()]
                .iter()
                .all(|primitive| primitive_stays_clear(primitive, raster_scale, &clear)),
        })
}

fn primitive_stays_clear(
    primitive: &DrawPrimitive,
    raster_scale: RasterScale,
    clear: impl Fn(Rect) -> bool,
) -> bool {
    let mut primitive = primitive;
    while let DrawPrimitive::Blend {
        primitive: inner, ..
    } = primitive
    {
        primitive = inner;
    }
    let bounds = match primitive {
        DrawPrimitive::Shadow(ShadowPrimitive::Drop {
            shape, blur_radius, ..
        }) => {
            let reach = raster_scale.shadow_reach(*blur_radius);
            primitive_coverage_rect(shape).map(|rect| expand_rect(rect, reach))
        }
        DrawPrimitive::Shadow(ShadowPrimitive::Inner { clip_rect, .. }) => Some(*clip_rect),
        DrawPrimitive::Content => return true,
        _ => primitive_coverage_rect(primitive),
    };
    match bounds {
        Some(rect) => {
            [rect.x, rect.y, rect.width, rect.height]
                .into_iter()
                .all(f32::is_finite)
                && clear(rect)
        }
        None => false,
    }
}

fn stays_clear(rect: Rect, offset: Point, corners: &RoundedClipCorners) -> bool {
    corners.admits(expand_rect(
        rect.translate(offset.x, offset.y),
        corners.aa_margin,
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
    segments: &mut CollectCache,
    capacity: SceneCapacityHint,
    root_scale: f32,
    recycler: &mut LayerSceneRecycler,
) -> LayerScene {
    let mut out = recycler.take(capacity);
    let context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        clip_radius: 0.0,
        snap_anchor: None,
        translated: false,
        raster_scale: RasterScale::Exact(root_scale),
        light: ShadowLight::for_window(
            root.local_bounds.width,
            root.local_bounds.height,
            1.0,
            root_scale,
        ),
        wants_pixel_sensitive: false,
        reuse_draws: true,
    };
    segments.begin_frame();
    collect_child(
        root,
        text_layout,
        motion,
        &mut Siblings::new(segments),
        context,
        &mut out,
        recycler,
    );
    segments.end_frame();
    out.scene.flush_loose();
    out.refresh_backdrop_summary();
    motion.end_frame();
    out
}

pub(crate) fn collect_overlay(
    root: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    root_scale: f32,
    recycler: &mut LayerSceneRecycler,
) -> LayerScene {
    collect_root(
        root,
        text_layout,
        &mut LayerMotion::default(),
        &mut CollectCache::default(),
        SceneCapacityHint::default(),
        root_scale,
        recycler,
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
    let local_layer = local_content_layer_for(&layer.graphics_layer);
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
        .or_else(|| rigid_snap_anchor(rect, &local_layer));
    scene.push_backdrop_layer(BackdropLayer {
        node_id: layer.node_id,
        alpha: local_layer.alpha,
        rect,
        clip,
        reach: context.visual_clip,
        rounded_clip,
        snap_anchor,
        effect: effect.clone(),
        z_index: 0,
    });
}

/// The walk's light seen from `layer`'s own space, which an isolated
/// layer's content is collected in.
fn light_in_layer(layer: &LayerNode, context: &WalkContext) -> ShadowLight {
    let to_parent = layer
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let scale = uniform_scale_translation(to_parent).map_or_else(
        || layer_uniform_scale(&layer.graphics_layer),
        |(scale, _)| scale,
    );
    let origin = to_parent.map_point(Point::default());
    context.light.in_space(origin.x, origin.y, scale)
}

fn isolated_child(
    layer: &LayerNode,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    segments: &mut CollectCache,
    context: WalkContext,
    parent_scene: &mut CompositorScene,
    recycler: &mut LayerSceneRecycler,
) -> (ChildLayer, bool) {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let content_hash = layer.target_content_hash();
    let nominal_scale = layer_uniform_scale(&layer.graphics_layer);
    let retained_scale = if layer.cache_policy == CachePolicy::Auto {
        motion.retained_scale(layer.node_id, nominal_scale, content_hash)
    } else {
        nominal_scale
    };
    let mut raster_scale = context.raster_scale.within(nominal_scale, retained_scale);
    if layer.backdrop().is_some() && uniform_scale_translation(layer.transform_to_parent).is_none()
    {
        raster_scale = raster_scale.within(nominal_scale, retained_scale);
    }
    let content_context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        clip_radius: 0.0,
        snap_anchor: None,
        translated: context.translated || layer.translated_content_context,
        raster_scale,
        light: light_in_layer(layer, &context),
        wants_pixel_sensitive: context.wants_pixel_sensitive
            || (!context.translated && context.snap_anchor.is_none()),
        reuse_draws: context.reuse_draws,
    };
    let mut content = recycler.take(SceneCapacityHint::default());
    let has_pixel_sensitive_subtree = collect_into(
        layer,
        text_layout,
        motion,
        segments,
        content_context,
        &mut content,
        recycler,
    );
    content.scene.flush_loose();
    content.refresh_backdrop_summary();
    let transform = layer
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let parent_bounds = quad_bounds(transform.map_rect(layer.local_bounds));
    let rigid = context.translated || has_pixel_sensitive_subtree;
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
    let cacheable = layer.cache_policy == CachePolicy::Auto && !content.contains_backdrop();
    let surface_scale = if cacheable {
        retained_scale
    } else {
        nominal_scale
    };
    motion.record_scale(layer.node_id, surface_scale, content_hash);
    let in_place = can_draw_in_place(layer, transform, surface_scale, &content);
    (
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
            backdrop_alpha: local_layer.alpha,
            snap_anchor,
            surface_scale,
            content_hash,
            cache_policy: layer.cache_policy,
            in_place,
            content,
        },
        has_pixel_sensitive_subtree,
    )
}

fn with_backdrop_in_own_space(child: ChildLayer, recycler: &mut LayerSceneRecycler) -> ChildLayer {
    if child.backdrop.is_none()
        || (uniform_scale_translation(child.transform).is_some()
            && child.alpha == 1.0
            && child.blend_mode == BlendMode::SrcOver)
    {
        return child;
    }
    let content = recycler.take(SceneCapacityHint::default());
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
        backdrop_alpha: 1.0,
        snap_anchor: child.snap_anchor,
        surface_scale: child.surface_scale,
        content_hash: child.content_hash,
        cache_policy: child.cache_policy,
        in_place: false,
        content,
    };
    let mut inner = ChildLayer {
        z_index: outer.content.scene.next_z(),
        transform: ProjectiveTransform::identity(),
        clip: None,
        alpha: 1.0,
        blend_mode: BlendMode::SrcOver,
        snap_anchor: None,
        surface_scale: 1.0,
        in_place: false,
        ..child
    };
    detach_flat_backdrop(&mut inner, &mut outer.content.scene);
    outer.content.scene.next_z += 1;
    outer.content.children.push(inner);
    outer.content.refresh_backdrop_summary();
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
        alpha: child.backdrop_alpha,
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
    segments: &mut CollectCache,
    context: WalkContext,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let layer_bounds = layer
        .local_bounds
        .translate(context.offset.x, context.offset.y);
    let layer_clip = layer
        .visual_clip_rect()
        .map(|clip| clip.translate(context.offset.x, context.offset.y));
    let visual_clip = resolve_clip(context.visual_clip, layer_clip);
    if visual_clip.is_some_and(|clip| clip.is_empty()) {
        return context.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(layer);
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
    let mut has_pixel_sensitive_subtree = false;
    let mut siblings = Siblings::new(segments);

    for (index, child) in layer.children.iter().enumerate() {
        match child {
            RenderNode::Layer(child_layer) => {
                let child_context = WalkContext {
                    offset: context.offset,
                    visual_clip,
                    clip_radius,
                    snap_anchor: translated_anchor,
                    translated,
                    raster_scale: context.raster_scale,
                    light: context.light,
                    wants_pixel_sensitive: context.wants_pixel_sensitive,
                    reuse_draws: context.reuse_draws,
                };
                has_pixel_sensitive_subtree |= collect_child(
                    child_layer,
                    text_layout,
                    motion,
                    &mut siblings,
                    child_context,
                    out,
                    recycler,
                );
            }
            _ => {
                has_pixel_sensitive_subtree |= node_is_pixel_sensitive(child);
                if content_phase(child) == PrimitivePhase::AfterChildren {
                    first_deferred = first_deferred.min(index);
                } else {
                    push_content(out, text_layout, child, &content);
                }
            }
        }
    }

    for child in &layer.children[first_deferred..] {
        if content_phase(child) == PrimitivePhase::AfterChildren {
            push_content(out, text_layout, child, &content);
        }
    }
    has_pixel_sensitive_subtree
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
    segments: &mut Siblings<'_>,
    context: WalkContext,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    let composite = layer_composite_params(&child.graphics_layer);
    if composite.is_some_and(|(alpha, blend)| alpha == 0.0 && blend == BlendMode::SrcOver) {
        return context.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(child);
    }
    match placement_in(child, &context) {
        placement @ (Placement::Direct(translation) | Placement::DirectRounded(translation, _)) => {
            let direct = DirectChild {
                layer: child,
                placement,
                translation,
            };
            let (context, keep) = match plan_direct_child(child, context, segments, &mut out.scene)
            {
                #[cfg(not(debug_assertions))]
                DirectPlan::Reused(pixel_sensitive) => return pixel_sensitive,
                DirectPlan::Collect { context, keep } => (context, keep),
                #[cfg(debug_assertions)]
                DirectPlan::Check {
                    node,
                    draws,
                    pixel_sensitive,
                } => {
                    let mark = out.scene.mark();
                    let fresh_pixel_sensitive = collect_direct_child(
                        direct,
                        text_layout,
                        motion,
                        segments.cache(),
                        context,
                        out,
                        recycler,
                    );
                    let fresh = out.scene.segment_since(mark);
                    assert!(
                        draws.draws_as(&fresh) && pixel_sensitive == fresh_pixel_sensitive,
                        "layer {node} would reuse draws its subtree no longer makes"
                    );
                    out.scene.rewind(mark);
                    out.scene.append_segment(&draws);
                    return pixel_sensitive;
                }
            };
            let mark = keep.map(|_| (out.scene.mark(), out.children.len()));
            let pixel_sensitive = collect_direct_child(
                direct,
                text_layout,
                motion,
                segments.cache(),
                context,
                out,
                recycler,
            );
            if let (Some(node), Some((mark, children))) = (keep, mark)
                && out.children.len() == children
                && out.scene.only_draws_since(mark)
            {
                segments
                    .cache()
                    .keep(node, out.scene.segment_since(mark), pixel_sensitive);
            }
            pixel_sensitive
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
                context.light,
            );
            let (isolated, has_pixel_sensitive_subtree) = isolated_child(
                child,
                text_layout,
                motion,
                segments.cache(),
                context,
                &mut out.scene,
                recycler,
            );
            let mut isolated = with_backdrop_in_own_space(isolated, recycler);
            assign_shadow_anchor(&mut out.scene, shadows_before, isolated.snap_anchor);
            detach_flat_backdrop(&mut isolated, &mut out.scene);
            out.children.push(isolated);
            out.scene.next_z += 1;
            direct_translation(child.transform_to_parent).is_some() && has_pixel_sensitive_subtree
        }
    }
}

/// What the collect cache decided for a direct child before it is collected.
enum DirectPlan {
    /// Its draws from an earlier frame are appended; whether it holds text
    /// or an image.
    #[cfg(not(debug_assertions))]
    Reused(bool),
    /// Collect it under `context`, keeping a copy of its draws for `keep`.
    Collect {
        context: WalkContext,
        keep: Option<NodeId>,
    },
    /// Debug builds collect a reusable child anyway and check its draws.
    #[cfg(debug_assertions)]
    Check {
        node: NodeId,
        draws: crate::scene::SceneSegment,
        pixel_sensitive: bool,
    },
}

#[inline(always)]
fn plan_direct_child(
    child: &LayerNode,
    context: WalkContext,
    segments: &mut Siblings<'_>,
    scene: &mut CompositorScene,
) -> DirectPlan {
    let Some((node, key)) = context
        .reuse_draws
        .then(|| segments.key(child, &context))
        .flatten()
    else {
        return DirectPlan::Collect {
            context,
            keep: None,
        };
    };
    match segments.cache().visit(node, &key) {
        Visit::Collect { keep } => DirectPlan::Collect {
            context,
            keep: keep.then_some(node),
        },
        Visit::Moved => DirectPlan::Collect {
            context: WalkContext {
                reuse_draws: false,
                ..context
            },
            keep: None,
        },
        #[cfg(not(debug_assertions))]
        Visit::Reuse(draws, pixel_sensitive) => {
            scene.append_segment(draws);
            DirectPlan::Reused(pixel_sensitive)
        }
        #[cfg(debug_assertions)]
        Visit::Reuse(draws, pixel_sensitive) => {
            let _ = scene;
            DirectPlan::Check {
                node,
                draws: draws.clone(),
                pixel_sensitive,
            }
        }
    }
}

/// A child drawn in place under its parent: the layer and where it goes.
#[derive(Clone, Copy)]
struct DirectChild<'a> {
    layer: &'a LayerNode,
    placement: Placement,
    translation: Point,
}

#[inline(always)]
fn collect_direct_child(
    direct: DirectChild<'_>,
    text_layout: &mut impl TextLayoutResolver,
    motion: &mut LayerMotion,
    segments: &mut CollectCache,
    context: WalkContext,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    let DirectChild {
        layer: child,
        placement,
        translation,
    } = direct;
    let child_offset = Point::new(
        context.offset.x + translation.x,
        context.offset.y + translation.y,
    );
    let child_bounds = child.local_bounds.translate(child_offset.x, child_offset.y);
    if clipped_away(child, child_bounds, context.visual_clip) {
        return context.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(child);
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
        context.light,
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
        raster_scale: context.raster_scale,
        light: context.light,
        wants_pixel_sensitive: context.wants_pixel_sensitive,
        reuse_draws: context.reuse_draws,
    };
    if child.backdrop().is_some() {
        push_backdrop_layer(child, child_offset, child_context, &mut out.scene);
    }
    collect_into(
        child,
        text_layout,
        motion,
        segments,
        child_context,
        out,
        recycler,
    )
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
