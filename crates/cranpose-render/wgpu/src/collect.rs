use std::{borrow::Cow, sync::Arc};

use cranpose_core::NodeId;
use cranpose_render_common::{
    geometry::{blur_reach, blur_reach_for_minimum_scale},
    graph::{
        CachePolicy, InlineNode, LayerNode, PrimitiveNode, ProjectiveTransform, RenderNode,
        for_each_in_draw_order,
    },
    layer_composition::{
        layer_composite_params, layer_requires_isolation, local_content_layer_for,
    },
    layer_shadow::ShadowLight,
    layer_transform::layer_uniform_scale,
    primitive_emit::resolve_clip,
    space::SeenDoor,
};
use cranpose_ui_graphics::{
    BlendMode, CompositingStrategy, GraphicsLayer, Point, Rect, RenderEffect,
    space::{Camera, Light, MeshSurface},
};

use crate::{
    pipeline::{layer_casts_shadow, push_layer_shadow},
    scene::{
        BackdropLayer, ClipRounding, CompositorScene, ContentContext, LayerRoundedClip, RunDraw,
        SceneCapacityHint, SnapAnchor, clip_holds,
    },
};

mod backdrop_space;
mod layer_motion;
mod push_content;
mod rigid_snap;
mod rounded_clip_corners;
mod space_children;
use backdrop_space::{detach_flat_backdrop, with_backdrop_in_own_space};
pub(crate) use layer_motion::LayerMotion;
use push_content::{assign_shadow_anchor, push_content};
pub(crate) use push_content::{assign_snap_anchor_since, scene_counts};
use rigid_snap::{
    layer_has_pixel_sensitive_subtree, node_is_pixel_sensitive, primitive_is_drawn,
    primitive_is_pixel_sensitive, rigid_snap_anchor,
};
use rounded_clip_corners::{
    content_admits_rounded_clip, content_takes_rounded_clip, rounded_clip_for_layer,
};
use space_children::collect_space_children;

const AFFINE_TOLERANCE: f32 = 1e-4;
const ROUNDED_CLIP_AA_MARGIN: f32 = 1.0;

/// One isolated layer's content in that layer's own coordinate space: the flat
/// z-ordered ops, the isolated children composited at their z, and the
/// backdrop effects that read what lies beneath them at their z.
pub(crate) struct LayerScene {
    pub(crate) scene: CompositorScene,
    pub(crate) children: Vec<ChildLayer>,
    has_backdrop: bool,
}

/// The layer scenes of returned packets, kept with their buffers for the
/// next collect. Boxed, as a layer's content is: a scene is some 540 bytes,
/// which every recycle, take and child push moved inline.
#[derive(Default)]
pub(crate) struct LayerSceneRecycler {
    #[expect(clippy::vec_box, reason = "scenes are recycled by box, so none moves")]
    scenes: Vec<Box<LayerScene>>,
}

impl LayerSceneRecycler {
    pub(crate) fn take(&mut self, capacity: SceneCapacityHint) -> Box<LayerScene> {
        match self.scenes.pop() {
            Some(mut scene) => {
                scene.has_backdrop = false;
                scene
            }
            None => Box::new(LayerScene::new(
                CompositorScene::with_capacity(capacity),
                Vec::new(),
            )),
        }
    }

    pub(crate) fn recycle(&mut self, mut scene: Box<LayerScene>) {
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
    pub(crate) content: Box<LayerScene>,
    /// For a mesh surface in a scene: the mesh its surface is drawn on in
    /// the scene's 3D pass, in place of a composite.
    pub(crate) mesh: Option<Arc<MeshChild>>,
}

/// A scene the window shows, as the 3D pass of its surfaces draws it, all in
/// the space its eye stands in.
pub(crate) struct SceneDraw {
    pub(crate) camera: Camera,
    /// The light of the space the eye stands in.
    pub(crate) light: Option<Light>,
    /// The scene node's rect in the coordinates its parent's content is
    /// collected in. The camera looks through its middle.
    pub(crate) rect: Rect,
    /// What cuts the scene, in the same coordinates.
    pub(crate) clip: Option<Rect>,
    /// The spaces the eye sees: its own first, each before the spaces seen
    /// through it.
    pub(crate) seen: Vec<SeenDraw>,
}

/// A space a scene's eye sees, as its pass draws it.
pub(crate) struct SeenDraw {
    /// The seen space whose doorway it shows through; `None` for the eye's
    /// own.
    pub(crate) through: Option<usize>,
    /// The space's light, in the eye's space.
    pub(crate) light: Option<Light>,
    /// The doorway it shows through; `None` for the eye's own space.
    pub(crate) door: Option<SeenDoor>,
}

/// A mesh surface the eye of a scene sees.
pub(crate) struct MeshChild {
    pub(crate) scene: Arc<SceneDraw>,
    /// The surface, its mesh placed in the space the eye stands in.
    pub(crate) surface: MeshSurface,
    /// The surface node's own rect, which the mesh's texture coordinates
    /// span.
    pub(crate) content: Rect,
    /// The seen space the surface lies in, by its number in the scene's.
    pub(crate) seen: usize,
}

impl ChildLayer {
    pub(crate) fn reads_backdrop(&self) -> bool {
        (self.backdrop.is_some() && self.backdrop_alpha != 0.0) || self.content.contains_backdrop()
    }
}

/// Where a walk stands at a layer. It is copied for every layer the walk
/// enters; what stays the same through a walk is in [`WalkSettings`], which
/// it borrows.
#[derive(Clone, Copy)]
struct WalkContext<'s> {
    offset: Point,
    visual_clip: Option<Rect>,
    /// The rounded clip `visual_clip` cuts from, whole.
    rounding: Option<ClipRounding>,
    snap_anchor: Option<SnapAnchor>,
    translated: bool,
    settings: &'s WalkSettings,
}

/// What one walk keeps for every layer it collects in place. An isolated
/// layer's content is walked with settings of its own.
#[derive(Clone, Copy)]
struct WalkSettings {
    raster_scale: RasterScale,
    /// The window's shadow light, in the coordinates this walk collects in.
    light: ShadowLight,
    /// Whether an isolated ancestor decides its snapping by whether this
    /// walk finds text or an image. Only then does a subtree that draws
    /// nothing have to be searched for them.
    wants_pixel_sensitive: bool,
    /// The graph's latest scene update, which tells an isolated layer whose
    /// content changes every frame from one a raster cache can keep.
    update: u64,
    /// Whether the walk lies in a scene's space, whose scene collects the
    /// layers there that are scenes, surfaces or 3D transforms: the walk
    /// leaves them out.
    in_space: bool,
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
/// shadow texts and elevation shadows their clips leave whole (a clip turned
/// off the pixel grid is no scissor; a text's own glyphs are cut to its clip
/// before the turn), no rounding a clip cuts into (only a scissor cuts it),
/// and children that draw in place unclipped.
fn content_draws_in_place(content: &LayerScene) -> bool {
    let scene = &content.scene;
    let whole =
        |rect: Rect, clip: Option<Rect>| clip.is_none_or(|clip| clip.intersect(rect) == Some(rect));
    let run_draws_alike = |run: &RunDraw| {
        !run.placement.rounding_cut()
            && run
                .segment_records()
                .all(|segment| segment.blend == BlendMode::SrcOver)
    };
    scene.backdrop_layers.is_empty()
        && scene.effect_layers.is_empty()
        && scene.runs.iter().all(run_draws_alike)
        && scene.shadow_draws.iter().all(|shadow| {
            !shadow.requires_surface()
                && shadow.texts.iter().all(|text| whole(text.rect, text.clip))
        })
        && scene
            .rrect_shadows
            .iter()
            .all(|shadow| whole(shadow.shadow.bounds, shadow.clip))
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

/// The rounding the layer's clip keeps: the one it entered with, while its
/// own clip, `layer_clip`, holds what is left of the rounded one. A clip
/// that cuts further is a rect nothing under it may round; what reaches
/// its corners kept the rounded layer on a surface.
fn rounding_within(layer_clip: Option<Rect>, context: &WalkContext<'_>) -> Option<ClipRounding> {
    let keeps = match (layer_clip, context.visual_clip) {
        (None, _) => true,
        (Some(own), Some(rounded)) => clip_holds(own, rounded),
        (Some(_), None) => false,
    };
    context.rounding.filter(|_| keeps)
}

pub(crate) fn collect_root(
    root: &LayerNode,
    update: u64,
    motion: &mut LayerMotion,
    capacity: SceneCapacityHint,
    root_scale: f32,
    recycler: &mut LayerSceneRecycler,
) -> Box<LayerScene> {
    let mut out = recycler.take(capacity);
    let settings = WalkSettings {
        raster_scale: RasterScale::Exact(root_scale),
        light: ShadowLight::for_window(
            root.local_bounds.width,
            root.local_bounds.height,
            1.0,
            root_scale,
        ),
        wants_pixel_sensitive: false,
        update,
        in_space: false,
    };
    let context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        rounding: None,
        snap_anchor: None,
        translated: false,
        settings: &settings,
    };
    collect_child(root, motion, context, &mut out, recycler);
    out.scene.flush_loose();
    out.refresh_backdrop_summary();
    motion.end_frame();
    out
}

pub(crate) fn collect_overlay(
    root: &LayerNode,
    root_scale: f32,
    recycler: &mut LayerSceneRecycler,
) -> Box<LayerScene> {
    collect_root(
        root,
        0,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        root_scale,
        recycler,
    )
}

#[inline(never)]
fn push_backdrop_layer(
    layer: &LayerNode,
    offset: Point,
    context: WalkContext<'_>,
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
fn light_in_layer(layer: &LayerNode, context: &WalkContext<'_>) -> ShadowLight {
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
    context.settings.light.in_space(origin.x, origin.y, scale)
}

/// The key of what `layer` draws. A layer whose content changed in update
/// `update` and in the one before gets a key of this update alone instead
/// of a hash of its content: the content is new, so the cache treats it as
/// it treats content that changes, without the hashing.
fn content_key(layer: &LayerNode, update: u64) -> u64 {
    const CHURN_SALT: u64 = 0x6a09_e667_f3bc_c909;
    if layer.content_churns(update) {
        return update.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ CHURN_SALT;
    }
    layer.target_content_hash()
}

fn isolated_child(
    layer: &LayerNode,
    motion: &mut LayerMotion,
    context: WalkContext<'_>,
    mesh: Option<Arc<MeshChild>>,
    parent_scene: &mut CompositorScene,
    recycler: &mut LayerSceneRecycler,
) -> (ChildLayer, bool) {
    let local_layer = local_content_layer_for(&layer.graphics_layer);
    let content_hash = content_key(layer, context.settings.update);
    let nominal_scale = layer_uniform_scale(&layer.graphics_layer);
    let retained_scale = if layer.cache_policy == CachePolicy::Auto {
        motion.retained_scale(layer.node_id, nominal_scale, content_hash)
    } else {
        nominal_scale
    };
    let mut raster_scale = context
        .settings
        .raster_scale
        .within(nominal_scale, retained_scale);
    if layer.backdrop().is_some() && uniform_scale_translation(layer.transform_to_parent).is_none()
    {
        raster_scale = raster_scale.within(nominal_scale, retained_scale);
    }
    let content_settings = WalkSettings {
        raster_scale,
        light: light_in_layer(layer, &context),
        wants_pixel_sensitive: context.settings.wants_pixel_sensitive
            || (!context.translated && context.snap_anchor.is_none()),
        update: context.settings.update,
        in_space: context.settings.in_space || mesh.is_some(),
    };
    let content_context = WalkContext {
        offset: Point::default(),
        visual_clip: None,
        rounding: None,
        snap_anchor: None,
        translated: context.translated || layer.translated_content_context,
        settings: &content_settings,
    };
    let mut content = recycler.take(SceneCapacityHint::default());
    let has_pixel_sensitive_subtree =
        collect_into(layer, motion, content_context, &mut content, recycler);
    content.scene.flush_loose();
    content.refresh_backdrop_summary();
    let transform = layer
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let parent_bounds = transform.bounds_for_rect(layer.local_bounds);
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
    let in_place = mesh.is_none() && can_draw_in_place(layer, transform, surface_scale, &content);
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
            mesh,
        },
        has_pixel_sensitive_subtree,
    )
}

fn collect_into(
    layer: &LayerNode,
    motion: &mut LayerMotion,
    context: WalkContext<'_>,
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
        return context.settings.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(layer);
    }
    let rounding = rounding_within(layer_clip, &context);
    let translated = context.translated || layer.translated_content_context;
    let allow_rigid_snap = translated || !layer.motion_context_animated;
    let boundary_anchor = boundary_anchor(layer, &context, (layer_bounds, &local_layer));
    let translated_anchor = context.snap_anchor.or(boundary_anchor);
    let layer_anchor = translated_anchor.or_else(|| {
        (allow_rigid_snap && content_needs_rigid_snap(&layer.children, translated))
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
        rounding,
        anchor: layer_anchor,
        motion_context_animated: layer.motion_context_animated || translated,
    };
    let child_context = WalkContext {
        offset: context.offset,
        visual_clip,
        rounding,
        snap_anchor: translated_anchor,
        translated,
        settings: context.settings,
    };
    if context.settings.in_space || layer.graphics_layer.space.is_some() {
        return collect_space_children(layer, motion, (child_context, &content), out, recycler);
    }
    let mut has_pixel_sensitive_subtree = false;
    for_each_in_draw_order(&layer.children, |child| {
        has_pixel_sensitive_subtree |= match child {
            RenderNode::Layer(child_layer) => {
                collect_child(child_layer, motion, child_context, out, recycler)
            }
            RenderNode::Inline(inline) => collect_inline(inline, child_context, out),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {
                push_content(out, child, &content);
                node_is_pixel_sensitive(child)
            }
        };
    });
    has_pixel_sensitive_subtree
}

/// The anchor a layer that starts moving its content as one translated
/// block, outside any such block, snaps that content to; `None` for any
/// other layer. The local layer is read only for such a layer.
fn boundary_anchor(
    layer: &LayerNode,
    context: &WalkContext<'_>,
    (layer_bounds, local_layer): (Rect, &Cow<'_, GraphicsLayer>),
) -> Option<SnapAnchor> {
    if context.translated || !layer.translated_content_context {
        return None;
    }
    rigid_snap_anchor(
        layer_bounds.translate(
            layer.translated_content_offset.x,
            layer.translated_content_offset.y,
        ),
        local_layer,
    )
}

fn collect_child(
    child: &LayerNode,
    motion: &mut LayerMotion,
    context: WalkContext<'_>,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    let composite = layer_composite_params(&child.graphics_layer);
    if composite.is_some_and(|(alpha, blend)| alpha == 0.0 && blend == BlendMode::SrcOver) {
        return context.settings.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(child);
    }
    // A rounded clip draws in place inside another one too: the outer one
    // drew in place only with every rounded layer under it clear of its
    // corners, where it cuts as its rect does, so the inner radius replaces it.
    let (translation, radius) = match child_placement(child, context.settings.raster_scale) {
        Placement::Direct(translation) => (translation, None),
        Placement::DirectRounded(translation, radius) => (translation, Some(radius)),
        Placement::Isolated => {
            return collect_isolated_child(child, motion, context, None, out, recycler);
        }
    };
    let child_offset = Point::new(
        context.offset.x + translation.x,
        context.offset.y + translation.y,
    );
    let child_bounds = child.local_bounds.translate(child_offset.x, child_offset.y);
    if clipped_away(child, child_bounds, context.visual_clip) {
        return context.settings.wants_pixel_sensitive && layer_has_pixel_sensitive_subtree(child);
    }
    let child_anchor = context.snap_anchor.or_else(|| {
        context
            .translated
            .then(|| {
                rigid_snap_anchor(
                    child_bounds,
                    &local_content_layer_for(&child.graphics_layer),
                )
            })
            .flatten()
    });
    if layer_casts_shadow(&child.graphics_layer) {
        push_direct_layer_shadow(
            child,
            (child_offset, child_bounds),
            child_anchor,
            &context,
            &mut out.scene,
        );
    }
    let (visual_clip, rounding) = match radius {
        Some(radius) => {
            let clip = child
                .visual_clip_rect()
                .map(|clip| clip.translate(child_offset.x, child_offset.y));
            (
                resolve_clip(context.visual_clip, clip),
                clip.map(|rect| ClipRounding { rect, radius }),
            )
        }
        None => (context.visual_clip, context.rounding),
    };
    let child_context = WalkContext {
        offset: child_offset,
        visual_clip,
        rounding,
        snap_anchor: child_anchor,
        ..context
    };
    if child.backdrop().is_some() {
        push_backdrop_layer(child, child_offset, child_context, &mut out.scene);
    }
    collect_into(child, motion, child_context, out, recycler)
}

// Collects `inline`, a node drawn in its parent's layer, under `context`,
// the walk of its parent's children: its content takes the place, clip and
// snap anchor a child layer that only moves would give it. Returns whether
// it draws pixel-sensitive content.
fn collect_inline(inline: &InlineNode, context: WalkContext<'_>, out: &mut LayerScene) -> bool {
    let offset = Point::new(
        context.offset.x + inline.origin.x,
        context.offset.y + inline.origin.y,
    );
    let bounds = inline.node_rect().translate(offset.x, offset.y);
    if context
        .visual_clip
        .is_some_and(|clip| inline.draws_within_bounds() && outside_clip(bounds, clip))
    {
        return context.settings.wants_pixel_sensitive
            && inline.content.iter().any(node_is_pixel_sensitive);
    }
    let translated = context.translated;
    let rigid = || rigid_snap_anchor(bounds, &GraphicsLayer::DEFAULT);
    let inherited = context
        .snap_anchor
        .or_else(|| translated.then(rigid).flatten());
    let anchor = inherited.or_else(|| {
        ((translated || !inline.motion_context_animated)
            && content_needs_rigid_snap(&inline.content, translated))
        .then(rigid)
        .flatten()
    });
    let content = ContentContext {
        layer_bounds: bounds,
        local_layer: &GraphicsLayer::DEFAULT,
        visual_clip: context.visual_clip,
        rounding: context.rounding,
        anchor,
        motion_context_animated: inline.motion_context_animated || translated,
    };
    let mut pixel_sensitive = false;
    for_each_in_draw_order(&inline.content, |node| {
        push_content(out, node, &content);
        pixel_sensitive |= node_is_pixel_sensitive(node);
    });
    pixel_sensitive
}

#[inline(never)]
fn push_direct_layer_shadow(
    child: &LayerNode,
    (child_offset, child_bounds): (Point, Rect),
    child_anchor: Option<SnapAnchor>,
    context: &WalkContext<'_>,
    scene: &mut CompositorScene,
) {
    let shadow_clip = resolve_clip(
        context.visual_clip,
        child
            .shadow_clip()
            .map(|clip| clip.translate(child_offset.x, child_offset.y)),
    );
    let shadows_before = scene.rrect_shadows.len();
    push_layer_shadow(
        scene,
        &child.graphics_layer,
        child_bounds,
        child_bounds,
        shadow_clip,
        context.settings.light,
    );
    assign_shadow_anchor(scene, shadows_before, child_anchor);
}

#[inline(never)]
fn collect_isolated_child(
    child: &LayerNode,
    motion: &mut LayerMotion,
    context: WalkContext<'_>,
    mesh: Option<Arc<MeshChild>>,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    let transform = child
        .transform_to_parent
        .then(ProjectiveTransform::translation(
            context.offset.x,
            context.offset.y,
        ));
    let child_bounds = transform.bounds_for_rect(child.local_bounds);
    let shadow_clip = resolve_clip(
        context.visual_clip,
        child
            .shadow_clip()
            .map(|clip| transform.bounds_for_rect(clip)),
    );
    let shadows_before = out.scene.rrect_shadows.len();
    push_layer_shadow(
        &mut out.scene,
        &child.graphics_layer,
        child.local_bounds,
        child_bounds,
        shadow_clip,
        context.settings.light,
    );
    let (isolated, has_pixel_sensitive_subtree) =
        isolated_child(child, motion, context, mesh, &mut out.scene, recycler);
    let mut isolated = with_backdrop_in_own_space(isolated, recycler);
    assign_shadow_anchor(&mut out.scene, shadows_before, isolated.snap_anchor);
    detach_flat_backdrop(&mut isolated, &mut out.scene);
    out.children.push(isolated);
    out.scene.next_z += 1;
    direct_translation(child.transform_to_parent).is_some() && has_pixel_sensitive_subtree
}

/// Whether nothing `child`, placed at `bounds`, draws can show inside
/// `clip`: its subtree draws within its bounds, it casts no shadow, and those
/// bounds miss the clip.
fn clipped_away(child: &LayerNode, bounds: Rect, clip: Option<Rect>) -> bool {
    clip.is_some_and(|clip| {
        child.draws_within_bounds()
            && child.graphics_layer.shadow_elevation <= 0.0
            && outside_clip(bounds, clip)
    })
}

// Whether `bounds`, widened by the slack a contained draw may take, miss
// `clip`.
fn outside_clip(bounds: Rect, clip: Rect) -> bool {
    let slack = cranpose_render_common::graph::CONTAINED_DRAW_SLACK;
    Rect {
        x: bounds.x - slack,
        y: bounds.y - slack,
        width: bounds.width + slack * 2.0,
        height: bounds.height + slack * 2.0,
    }
    .intersect(clip)
    .is_none()
}

fn content_needs_rigid_snap(children: &[RenderNode], translated: bool) -> bool {
    children.iter().any(|child| match child {
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
        RenderNode::Layer(_) | RenderNode::Inline(_) => false,
    })
}

#[cfg(test)]
#[path = "tests/collect_tests.rs"]
mod tests;
