use std::{
    borrow::Cow,
    collections::hash_map::Entry,
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{CachePolicy, ProjectiveTransform, quad_bounds},
    raster_cache::{LayerRasterCacheKey, RasterScale},
};
use cranpose_ui_graphics::{
    BlendMode, Color, MAX_SUBSTRATES, Point, Rect, RenderEffect, RenderHash, RuntimeShader,
    ShaderPlaceholder, SubstrateSpec, TileMode,
};
use smallvec::SmallVec;

use crate::{
    ablation::Ablation,
    capture_hash::{CaptureWindow, capture_hasher, hash_capture_composites, hash_capture_ops},
    collect::{
        ChildLayer, LayerScene, in_place_content_scale, similarity_scale, uniform_scale_translation,
    },
    debug_toggles::DebugToggle,
    draw_pass::{
        PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind, SourceContent,
        op_draw_bounds, scissor_in_target, segment_draws_anything,
    },
    effect_renderer::{
        AtlasSideWork, BlurRegion, CompositeSampleMode, EffectReads, EffectScratchTargetProvider,
        RoundedCompositeMask, SubstrateAverage, SubstrateRegion, SubstrateRegions,
        blur_scratch_size, substrate_scratch_size,
    },
    frame_graph::{
        FrameCommandRecorder, FrameTextureDescriptor, TextureRegionCopy, copy_compatible,
    },
    geometry::{SegmentTransform, snap_delta_for_anchor},
    layer_cache::{Retained, RetainedContent},
    offscreen::OffscreenTarget,
    opaque_prefix::{OpaquePrefix, PrefixContext, opaque_prefix, page_fill_color},
    render::GpuRenderer,
    scene::{BackdropLayer, CompositorScene, DrawOp, DrawOpKind, EffectLayer, LayerRoundedClip},
};

const MAX_SURFACE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_RESOLVE_DEPTH: usize = 128;

/// Device-space rectangle: origin and size in pixels of some scene's device
/// space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DeviceRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl DeviceRect {
    fn from_logical(rect: Rect, scale: f32) -> Self {
        let pixel = |value: f32| {
            let whole = value.round();
            if (value - whole).abs() <= value.abs().max(1.0) * f32::EPSILON * 2.0 {
                whole
            } else {
                value
            }
        };
        Self {
            x: pixel(rect.x * scale),
            y: pixel(rect.y * scale),
            width: pixel(rect.width * scale),
            height: pixel(rect.height * scale),
        }
    }

    fn tuple(self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.height)
    }

    fn size(self) -> [f32; 2] {
        [self.width, self.height]
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }

    fn expand(self, margin: f32) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + margin * 2.0,
            height: self.height + margin * 2.0,
        }
    }

    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y,
        }
    }

    /// Snaps to whole pixels, growing outward.
    fn translated(self, delta: Point) -> Self {
        Self {
            x: self.x + delta.x,
            y: self.y + delta.y,
            ..self
        }
    }

    fn snap_out(self) -> Self {
        let left = self.x.floor();
        let top = self.y.floor();
        let right = (self.x + self.width).ceil();
        let bottom = (self.y + self.height).ceil();
        Self {
            x: left,
            y: top,
            width: (right - left).max(1.0),
            height: (bottom - top).max(1.0),
        }
    }

    fn pixel_size(self) -> (u32, u32) {
        (
            (self.width.ceil().max(1.0)) as u32,
            (self.height.ceil().max(1.0)) as u32,
        )
    }
}

struct Beneath<'a> {
    base: wgpu::LoadOp<wgpu::Color>,
    page: Option<PageBase>,
    source: Option<BeneathSegment<'a>>,
}

impl Beneath<'_> {
    fn over(base: wgpu::LoadOp<wgpu::Color>) -> Self {
        Self {
            base,
            page: None,
            source: None,
        }
    }
}

struct BeneathSegment<'a> {
    base: wgpu::LoadOp<wgpu::Color>,
    scene: &'a CompositorScene,
    z_end: usize,
    drawn: &'a [ResolvedComposite],
    pending: &'a [ResolvedComposite],
    placement: [f32; 2],
}

/// The parent's page under an isolated child that reads its backdrop, and
/// how the child's device space maps into it.
#[derive(Clone)]
struct PageBase {
    source: Rc<OffscreenTarget>,
    origin: [f32; 2],
    placement: PagePlacement,
}

#[derive(Clone)]
enum PagePlacement {
    /// A child device point plus `shift` is the parent device point.
    Translated { shift: [f32; 2] },
    /// The parent page's pixels under the child, projected into the child's
    /// device space: `inverse` maps a child device point to a page pixel.
    Projected {
        bounds: Option<DeviceRect>,
        inverse: [[f32; 3]; 3],
    },
}

impl PageBase {
    fn child_rect(&self) -> Option<DeviceRect> {
        match self.placement {
            PagePlacement::Translated { shift } => {
                Some(self.rect().translated(Point::new(-shift[0], -shift[1])))
            }
            PagePlacement::Projected { bounds, .. } => bounds,
        }
    }

    fn rect(&self) -> DeviceRect {
        DeviceRect {
            x: self.origin[0],
            y: self.origin[1],
            width: self.source.width as f32,
            height: self.source.height as f32,
        }
    }

    /// The parent pixels under `region` of the child, drawn into the child's
    /// device space.
    fn under(&self, region: DeviceRect) -> Option<ResolvedComposite> {
        match self.placement {
            PagePlacement::Translated { shift } => {
                let parent = region
                    .translated(Point::new(shift[0], shift[1]))
                    .intersect(self.rect())?;
                Some(page_blit(
                    &self.source,
                    self.origin,
                    parent,
                    parent.translated(Point::new(-shift[0], -shift[1])),
                ))
            }
            PagePlacement::Projected { inverse, .. } => Some(ResolvedComposite {
                z_index: 0,
                source: Rc::clone(&self.source),
                content: SourceContent::Transient,
                dest: region.tuple(),
                scissor: None,
                kind: ResolvedCompositeKind::Projective {
                    dest_quad: [
                        [region.x, region.y],
                        [region.x + region.width, region.y],
                        [region.x, region.y + region.height],
                        [region.x + region.width, region.y + region.height],
                    ],
                    inverse,
                    alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    sample_mode: CompositeSampleMode::Linear,
                    source_region: None,
                },
            }),
        }
    }
}

/// The pixels of `source`, whose origin sits at `origin` in device space,
/// within `parent`, drawn at `dest`.
fn prefix_blit(prefix: &OpaquePrefix, texture: Rc<OffscreenTarget>) -> ResolvedComposite {
    ResolvedComposite {
        z_index: prefix.z_index,
        source: texture,
        content: SourceContent::retained(&prefix.key),
        dest: prefix.device_rect,
        scissor: None,
        kind: ResolvedCompositeKind::Blit {
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_mask: None,
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: None,
        },
    }
}

fn page_blit(
    source: &Rc<OffscreenTarget>,
    origin: [f32; 2],
    parent: DeviceRect,
    dest: DeviceRect,
) -> ResolvedComposite {
    ResolvedComposite {
        z_index: 0,
        source: Rc::clone(source),
        content: SourceContent::Transient,
        dest: dest.tuple(),
        scissor: None,
        kind: ResolvedCompositeKind::Blit {
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_mask: None,
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: Some((
                parent.x - origin[0],
                parent.y - origin[1],
                parent.width,
                parent.height,
            )),
        },
    }
}

/// The texture a layer draws its strata into and its captures read back:
/// the frame's root image, or an isolated child's surface, with the device
/// offset its origin sits at.
#[derive(Clone)]
struct Page {
    texture: Rc<OffscreenTarget>,
    offset: [f32; 2],
}

impl Page {
    fn pass_target(&self) -> PassTarget<'_> {
        PassTarget {
            view: &self.texture.view,
            width: self.texture.width,
            height: self.texture.height,
        }
    }

    fn rect(&self) -> DeviceRect {
        DeviceRect {
            x: self.offset[0],
            y: self.offset[1],
            width: self.texture.width as f32,
            height: self.texture.height as f32,
        }
    }

    /// The page's pixels within `rect`, drawn back in place.
    fn blit(&self, rect: DeviceRect) -> Option<ResolvedComposite> {
        let rect = rect.intersect(self.rect())?;
        Some(page_blit(&self.texture, self.offset, rect, rect))
    }

    /// The page's texels within `rect` copied to `origin` of `dest`, when
    /// the rect lies on whole texels inside the page and the copy fits.
    fn copy<'a>(
        &'a self,
        rect: DeviceRect,
        dest: &'a OffscreenTarget,
        origin: [f32; 2],
    ) -> Option<TextureRegionCopy<'a>> {
        let source = [rect.x - self.offset[0], rect.y - self.offset[1]];
        grid_copy(&self.texture, source, dest, origin, rect.size())
    }
}

fn grid_copy<'a>(
    source: &'a OffscreenTarget,
    source_origin: [f32; 2],
    dest: &'a OffscreenTarget,
    dest_origin: [f32; 2],
    size: [f32; 2],
) -> Option<TextureRegionCopy<'a>> {
    let coords = [
        source_origin[0],
        source_origin[1],
        size[0],
        size[1],
        dest_origin[0],
        dest_origin[1],
    ];
    if coords
        .iter()
        .any(|value| value.fract() != 0.0 || *value < 0.0)
    {
        return None;
    }
    let size = [size[0] as u32, size[1] as u32];
    let source_origin = [source_origin[0] as u32, source_origin[1] as u32];
    let dest_origin = [dest_origin[0] as u32, dest_origin[1] as u32];
    let fits = |origin: [u32; 2], target: &OffscreenTarget| {
        origin[0] + size[0] <= target.width && origin[1] + size[1] <= target.height
    };
    (fits(source_origin, source) && fits(dest_origin, dest)).then_some(TextureRegionCopy {
        source,
        source_origin,
        dest,
        dest_origin,
        size,
    })
}

/// One layer's render in progress: the page it draws into, the strata it has
/// drawn (every op below `drawn_z` except the deferred ones, and every
/// composite in `drawn`), the composites still to draw, the ops held back
/// behind a captured glass, the glasses of the running stage whose
/// composites are not on the page yet, the backdrops waiting for their
/// stage, and the children waiting to draw in place at their z. Children
/// draw in place only in a layer nothing reads back and no effect range
/// renders apart: what they draw is on no composite a capture or effect
/// could account for.
struct LayerPass<'a> {
    layer: &'a LayerScene,
    page: Page,
    scale: f32,
    beneath: &'a Beneath<'a>,
    drawn: Vec<ResolvedComposite>,
    pending: Vec<ResolvedComposite>,
    deferred: Vec<DrawOp>,
    blockers: Vec<Blocker>,
    excluded: Vec<(usize, usize)>,
    stages: ResolveStages<'a>,
    drawn_z: usize,
    load_op: Option<wgpu::LoadOp<wgpu::Color>>,
    segments: usize,
    resolved: Vec<Option<Resolved>>,
    in_place_allowed: bool,
    in_place: Vec<usize>,
}

const LAYER_PASS_LABELS: [&str; 6] = [
    "Layer Pass 0",
    "Layer Pass 1",
    "Layer Pass 2",
    "Layer Pass 3",
    "Layer Pass 4",
    "Layer Pass 5+",
];

/// Pixels that something not yet on the page will claim: a glass of the
/// running stage (its capture rect, since its capture must not see what is
/// above it) or a deferred op or composite part. Anything above it in z
/// that touches the rect waits behind it.
#[derive(Clone, Copy)]
struct Blocker {
    z: usize,
    rect: DeviceRect,
}

fn blocked_coverage(holes: &mut Vec<Blocker>, z: usize, coverage: DeviceRect) -> bool {
    let blocked = holes
        .iter()
        .filter(|hole| hole.z < z)
        .find_map(|hole| hole.rect.intersect(coverage).map(|part| part == coverage));
    let Some(fully_covered) = blocked else {
        return false;
    };
    if !fully_covered {
        holes.push(Blocker { z, rect: coverage });
    }
    true
}

fn release_op(
    op: DrawOp,
    scene: &CompositorScene,
    scale: f32,
    holes: &mut Vec<Blocker>,
    deferred: &mut Vec<DrawOp>,
    now_ops: &mut Vec<DrawOp>,
) {
    let blocked = op_draw_bounds(scene, &op, scale).is_some_and(|bounds| {
        blocked_coverage(holes, op.z_index, DeviceRect::from_logical(bounds, scale))
    });
    if blocked {
        deferred.push(op);
    } else {
        now_ops.push(op);
    }
}

fn release_composite(
    composite: ResolvedComposite,
    holes: &mut Vec<Blocker>,
    now: &mut Vec<ResolvedComposite>,
    pending: &mut Vec<ResolvedComposite>,
) {
    let Some(coverage) = composite_coverage(&composite) else {
        return;
    };
    if blocked_coverage(holes, composite.z_index, coverage) {
        pending.push(composite);
    } else {
        now.push(composite);
    }
}

fn ensure_sorted_by_key<T, K: Ord>(values: &mut [T], key: impl Fn(&T) -> K) {
    if !values.is_sorted_by_key(&key) {
        values.sort_by_key(key);
    }
}

fn draw_op_z(op: &DrawOp) -> usize {
    op.z_index
}

fn composite_z(composite: &ResolvedComposite) -> usize {
    composite.z_index
}

fn backdrop_cache_keys_use_pending(pass: &LayerPass<'_>) -> bool {
    !NO_BACKDROP_CACHE.flag()
        && matches!(pass.beneath.base, wgpu::LoadOp::Clear(_))
        && pass
            .beneath
            .source
            .as_ref()
            .is_none_or(|source| matches!(source.base, wgpu::LoadOp::Clear(_)))
        && !matches!(
            pass.beneath.page,
            Some(PageBase {
                placement: PagePlacement::Projected { .. },
                ..
            })
        )
}

impl<'scene> LayerPass<'scene> {
    fn target_rect(&self) -> DeviceRect {
        self.page.rect()
    }

    fn capture_bounds(&self) -> Option<DeviceRect> {
        let target = self.target_rect();
        self.beneath.page.as_ref().map_or(Some(target), |base| {
            base.child_rect().map(|rect| target.union(rect))
        })
    }

    fn can_draw_in_place(&self, child: &ChildLayer) -> bool {
        self.in_place_allowed && child.in_place
    }

    /// The children left to draw in place below `z`, in z order.
    fn take_in_place_below(&mut self, z: usize) -> Vec<usize> {
        let children = &self.layer.children;
        let end = self
            .in_place
            .partition_point(|&index| children[index].z_index < z);
        self.in_place.drain(..end).collect()
    }

    /// Records how the child at `index` draws, resolved ahead of its z.
    fn resolve(&mut self, index: usize, resolved: Resolved) {
        if self.resolved.len() <= index {
            self.resolved
                .resize_with(self.layer.children.len(), || None);
        }
        self.resolved[index] = Some(resolved);
    }

    /// Whether the child at `index` was resolved ahead of its z to draw in
    /// place, or to a surface; `None` when it was not resolved ahead.
    fn resolved_in_place(&self, index: usize) -> Option<bool> {
        self.resolved
            .get(index)
            .and_then(Option::as_ref)
            .map(|resolved| matches!(resolved, Resolved::InPlace))
    }

    /// The ops between the page's drawn z and `z` outside the excluded
    /// ranges, with the deferred ops below `z`, in z order.
    /// Whether the page still awaits its transparent clear: nothing has been
    /// drawn on it, so a blit of it is the identity and it holds nothing to
    /// copy.
    fn page_untouched(&self) -> bool {
        is_transparent_clear(self.load_op)
    }

    fn ops_below(&self, z: usize) -> Cow<'_, [DrawOp]> {
        pending_draw_ops(
            &self.layer.scene.draw_ops,
            self.drawn_z,
            z,
            &self.excluded,
            &self.deferred,
        )
    }

    fn take_ops_below(&mut self, z: usize) -> Cow<'scene, [DrawOp]> {
        let ops =
            filtered_ops_in_range(&self.layer.scene.draw_ops, self.drawn_z, z, &self.excluded);
        let deferred_end = self.deferred.partition_point(|op| op.z_index < z);
        if deferred_end == 0 {
            return ops;
        }
        merge_draw_ops(ops, self.deferred.drain(..deferred_end))
    }

    fn release<'ops>(
        &mut self,
        ops: Cow<'ops, [DrawOp]>,
        mut composites: Vec<ResolvedComposite>,
    ) -> (Cow<'ops, [DrawOp]>, Vec<ResolvedComposite>) {
        if self.blockers.is_empty() {
            composites.retain(|composite| composite_coverage(composite).is_some());
            ensure_sorted_by_key(&mut composites, composite_z);
            ensure_sorted_by_key(&mut self.deferred, draw_op_z);
            return (ops, composites);
        }
        let scene = &self.layer.scene;
        let scale = self.scale;
        let op_count = ops.len();
        let composite_count = composites.len();
        let mut ops = ops.iter().copied().peekable();
        let mut composites = composites.into_iter().peekable();
        let blocker_count = self.blockers.len();
        let mut now_ops = Vec::with_capacity(op_count);
        let mut now = Vec::with_capacity(composite_count);
        loop {
            let take_composite = match (composites.peek(), ops.peek()) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(composite), Some(op)) => composite.z_index <= op.z_index,
            };
            if take_composite {
                if let Some(composite) = composites.next() {
                    release_composite(composite, &mut self.blockers, &mut now, &mut self.pending);
                }
            } else if let Some(op) = ops.next() {
                release_op(
                    op,
                    scene,
                    scale,
                    &mut self.blockers,
                    &mut self.deferred,
                    &mut now_ops,
                );
            }
        }
        self.blockers.truncate(blocker_count);
        self.deferred.sort_by_key(draw_op_z);
        (Cow::Owned(now_ops), now)
    }

    /// The pending composites below `z`, in z order.
    fn pending_below(&mut self, z: usize) -> &[ResolvedComposite] {
        ensure_sorted_by_key(&mut self.pending, composite_z);
        let end = self
            .pending
            .partition_point(|composite| composite.z_index < z);
        &self.pending[..end]
    }

    fn drawn_below(&self, z: usize) -> &[ResolvedComposite] {
        let end = self
            .drawn
            .partition_point(|composite| composite.z_index < z);
        &self.drawn[..end]
    }
}

const MAX_ATLAS_DIM: u32 = 4096;
const ATLAS_SIZE_STEP: u32 = 16;

/// A region of a capture atlas: the device rect it holds and where its
/// origin sits in the atlas.
struct CaptureRegion {
    z: usize,
    rect: DeviceRect,
    origin: [f32; 2],
}

/// What maps the page's pixels onto `region`'s place in its capture.
fn region_offset(region: &CaptureRegion) -> [f32; 2] {
    [
        region.rect.x - region.origin[0],
        region.rect.y - region.origin[1],
    ]
}

#[derive(Clone, Copy)]
struct BlurSpec {
    radius_x: f32,
    radius_y: f32,
    tile_mode: TileMode,
}

/// A backdrop effect the renderer can resolve from a shared atlas: a blur,
/// a shader that reads its source region, or the two chained.
#[derive(Clone, Copy)]
enum BatchedEffect<'a> {
    Blur(BlurSpec),
    Shader(&'a Arc<RuntimeShader>),
    BlurThenShader(BlurSpec, &'a Arc<RuntimeShader>),
}

impl<'a> BatchedEffect<'a> {
    fn shader(self) -> Option<&'a Arc<RuntimeShader>> {
        match self {
            Self::Shader(shader) | Self::BlurThenShader(_, shader) => Some(shader),
            Self::Blur(_) => None,
        }
    }

    fn input_hash(self) -> u64 {
        let mut hasher = capture_hasher();
        self.shader().is_some().hash(&mut hasher);
        self.blur().is_some().hash(&mut hasher);
        if let Some(blur) = self.blur() {
            blur.radius_x.to_bits().hash(&mut hasher);
            blur.radius_y.to_bits().hash(&mut hasher);
            blur.tile_mode.hash(&mut hasher);
        }
        if let Some(shader) = self.shader() {
            shader.hash_substrates(&mut hasher);
        }
        hasher.finish()
    }

    fn blur(self) -> Option<BlurSpec> {
        match self {
            Self::Blur(blur) | Self::BlurThenShader(blur, _) => Some(blur),
            Self::Shader(_) => None,
        }
    }

    /// The substrates the member's shader declared, in slot order.
    fn substrates(self) -> &'a [SubstrateSpec] {
        match self {
            Self::Shader(shader) | Self::BlurThenShader(_, shader) => shader.substrates(),
            Self::Blur(_) => &[],
        }
    }
}

fn blur_spec(effect: &RenderEffect) -> Option<BlurSpec> {
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } if *radius_x > 0.0 || *radius_y > 0.0 => Some(BlurSpec {
            radius_x: *radius_x,
            radius_y: *radius_y,
            tile_mode: *edge_treatment,
        }),
        _ => None,
    }
}

/// A child's surface after its effect.
enum ThroughEffect<'l> {
    /// The effect is a shader drawn in the final pass over the surface.
    Tail(ResolvedComposite),
    /// The surface to composite, and the effect it is drawn without while
    /// the effect's pipelines compile.
    Source {
        source: CompositeSource,
        waiting: Option<&'l RenderEffect>,
    },
}

impl ThroughEffect<'_> {
    fn plain(surface: &SurfaceRender) -> Self {
        Self::Source {
            source: surface.source.clone(),
            waiting: None,
        }
    }
}

/// The rounded mask of `placeholder`'s shape over a layer at `layer`, drawn
/// at `scale`; none when it fills the layer's own shape.
fn placeholder_mask(
    placeholder: ShaderPlaceholder,
    layer: DeviceRect,
    scale: f32,
) -> Option<RoundedCompositeMask> {
    let shape = placeholder.shape?;
    let (width, height) = (
        shape.bounds.width * layer.width,
        shape.bounds.height * layer.height,
    );
    let radius = (shape.corner_radius * scale).clamp(0.0, 0.5 * width.min(height));
    Some(RoundedCompositeMask {
        rect: [
            layer.x + shape.bounds.x * layer.width,
            layer.y + shape.bounds.y * layer.height,
            width,
            height,
        ],
        radii: [radius; 4],
    })
}

/// Clips `composite`, a layer's content drawn without the `waiting` effect
/// while that effect compiles, to the shape of its placeholder over a layer
/// at `layer`, drawn at `scale`: a glass that masks its content keeps the
/// content in its shape.
fn clip_to_placeholder(
    composite: &mut ResolvedComposite,
    waiting: Option<&RenderEffect>,
    layer: DeviceRect,
    scale: f32,
) {
    if let Some(mask) = waiting
        .and_then(RenderEffect::placeholder)
        .and_then(|placeholder| placeholder_mask(placeholder, layer, scale))
        && let ResolvedCompositeKind::Blit { rounded_mask, .. } = &mut composite.kind
    {
        *rounded_mask = Some(mask);
    }
}

fn batched_effect(effect: &RenderEffect) -> Option<BatchedEffect<'_>> {
    match effect {
        RenderEffect::Blur { .. } => blur_spec(effect).map(BatchedEffect::Blur),
        RenderEffect::Shader { shader } if shader.batched_source() => {
            Some(BatchedEffect::Shader(shader))
        }
        RenderEffect::Chain { first, second } => match second.as_ref() {
            RenderEffect::Shader { shader } if shader.batched_source() => {
                blur_spec(first).map(|blur| BatchedEffect::BlurThenShader(blur, shader))
            }
            _ => None,
        },
        _ => None,
    }
}

/// A backdrop effect waiting for its stage: the device rect it captures,
/// the effect rect inside it, the pixels its composite may touch, and how it
/// resolves.
struct PendingBackdrop<'a> {
    z: usize,
    alpha: f32,
    node_id: Option<NodeId>,
    key: Option<LayerRasterCacheKey>,
    capture_rect: DeviceRect,
    layer_rect: DeviceRect,
    visible: DeviceRect,
    effect: &'a RenderEffect,
    rounded_mask: Option<RoundedCompositeMask>,
    batched: Option<BatchedEffect<'a>>,
    stage: usize,
    support: Option<DeviceRect>,
}

impl PendingBackdrop<'_> {
    /// Whether `shader`, drawing last in the effect, draws in the final pass
    /// composited over what is beneath rather than into a texture first.
    fn composites_tail(&self, shader: &RuntimeShader) -> bool {
        (self.rounded_mask.is_none() && self.alpha >= 1.0) || shader.batched_source()
    }

    /// Whether the effect's last shader draws composited.
    fn composites_shader(&self) -> bool {
        self.batched.is_some()
            || shader_tail(self.effect).is_some_and(|(_, shader)| self.composites_tail(shader))
    }

    fn layer_pixel_rect(&self) -> [f32; 4] {
        [
            self.layer_rect.x - self.capture_rect.x,
            self.layer_rect.y - self.capture_rect.y,
            self.layer_rect.width,
            self.layer_rect.height,
        ]
    }
}

static STAGE_DIAG: DebugToggle = DebugToggle::new("CRANPOSE_GPU_STAGE_DIAG");
static NO_EFFECT_DOMAINS: DebugToggle = DebugToggle::new("CRANPOSE_NO_EFFECT_DOMAINS");
static NO_FILL_CACHE: DebugToggle = DebugToggle::new("CRANPOSE_NO_FILL_CACHE");
const ABLATION_LOG_PERIOD: u32 = 600;
static NO_BACKDROP_CACHE: DebugToggle = DebugToggle::new("CRANPOSE_NO_BACKDROP_CACHE");
static PROBE_PASSES: DebugToggle = DebugToggle::new("CRANPOSE_PROBE_PASSES");
static PROBE_DRAW_PASSES: DebugToggle = DebugToggle::new("CRANPOSE_PROBE_DRAW_PASSES");

fn declared_support(support: Option<Rect>) -> Option<Rect> {
    if NO_EFFECT_DOMAINS.flag() {
        return None;
    }
    support
}

fn declared_domain(domain: Option<Rect>) -> Option<Rect> {
    if NO_EFFECT_DOMAINS.flag() {
        return None;
    }
    domain
}

fn output_support(effect: &RenderEffect) -> Option<Rect> {
    declared_support(effect.output_support())
}

fn child_composite_support(
    child: &ChildLayer,
    support: Option<Rect>,
    snap: Point,
    scale: f32,
    visible: DeviceRect,
) -> Option<DeviceRect> {
    let Some(support) = declared_support(support) else {
        return Some(visible);
    };
    let local = support.translate(child.local_bounds.x, child.local_bounds.y);
    let logical = quad_bounds(child.transform.map_rect(local)).translate(snap.x, snap.y);
    visible.intersect(DeviceRect::from_logical(logical, scale))
}

fn stage_diagnostics_enabled() -> bool {
    STAGE_DIAG.flag()
}

fn log_stage(stage: usize, items: &[&PendingBackdrop<'_>]) {
    for item in items {
        let capture = item.capture_rect;
        let visible = item.visible;
        let (blur, substrates) = match item.batched {
            Some(batched) => (batched.blur().is_some(), batched.substrates().len()),
            None => (false, 0),
        };
        let folds: Vec<&str> = match item.batched {
            Some(BatchedEffect::Shader(shader) | BatchedEffect::BlurThenShader(_, shader)) => {
                shader.overrides().iter().map(|(name, _)| *name).collect()
            }
            _ => Vec::new(),
        };
        log::warn!(
            "[stage-diag] stage={stage} z={} capture=({:.0},{:.0},{:.0},{:.0}) visible=({:.0},{:.0},{:.0},{:.0}) batched={} blur={blur} substrates={substrates} folds={folds:?} key={:?} node={:?} effect_rect={:?}",
            item.z,
            capture.x,
            capture.y,
            capture.width,
            capture.height,
            visible.x,
            visible.y,
            visible.width,
            visible.height,
            item.batched.is_some(),
            item.key,
            item.node_id,
            item.layer_pixel_rect(),
        );
    }
}

/// Everything a layer scene resolves before its final pass, in the order
/// it resolves: by z, and at one z a backdrop before the shadow before the
/// effect range before the child.
fn layer_events(layer: &LayerScene) -> Vec<(usize, Event)> {
    let scene = &layer.scene;
    let mut events: Vec<(usize, Event)> = Vec::new();
    for (index, child) in layer.children.iter().enumerate() {
        events.push((child.z_index, Event::Child(index)));
    }
    for (index, backdrop) in scene.backdrop_layers.iter().enumerate() {
        events.push((backdrop.z_index, Event::Backdrop(index)));
    }
    for (index, effect) in scene.effect_layers.iter().enumerate() {
        events.push((effect.z_start, Event::Effect(index)));
    }
    for (index, shadow) in scene.shadow_draws.iter().enumerate() {
        if shadow.requires_surface() {
            events.push((shadow.z_index, Event::Shadow(index)));
        }
    }
    events.sort_by_key(|(z, event)| {
        let order = match event {
            Event::Backdrop(_) => 0,
            Event::Shadow(_) => 1,
            Event::Effect(_) => 2,
            Event::Child(_) => 3,
        };
        (*z, order)
    });
    events
}

fn plan_backdrop(
    backdrop: &BackdropLayer,
    z: usize,
    scale: f32,
    target_rect: DeviceRect,
    capture_bounds: Option<DeviceRect>,
) -> Option<PendingBackdrop<'_>> {
    if backdrop.alpha == 0.0 {
        return None;
    }
    let snap = backdrop
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default();
    let rect = backdrop.rect.translate(snap.x, snap.y);
    let clip = backdrop.clip.map(|clip| clip.translate(snap.x, snap.y));
    let visible = match clip {
        Some(clip) => rect.intersect(clip)?,
        None => rect,
    };
    let visible = DeviceRect::from_logical(visible, scale).intersect(target_rect)?;
    let support = match output_support(&backdrop.effect) {
        Some(support) => Some(
            DeviceRect::from_logical(support.translate(rect.x, rect.y), scale)
                .intersect(visible)?,
        ),
        None => None,
    };
    let padding = (backdrop.effect.input_padding() + backdrop.effect.output_padding()) * scale;
    let expanded = visible.expand(padding.ceil());
    let capture_bounds = capture_bounds.unwrap_or(expanded);
    let reach = match backdrop.reach {
        Some(reach) => DeviceRect::from_logical(reach.translate(snap.x, snap.y), scale)
            .intersect(capture_bounds)?,
        None => capture_bounds,
    };
    let capture_rect = expanded.intersect(reach).unwrap_or(visible).snap_out();
    Some(PendingBackdrop {
        z,
        alpha: backdrop.alpha,
        node_id: backdrop.node_id,
        key: None,
        capture_rect,
        layer_rect: DeviceRect::from_logical(rect, scale),
        visible,
        effect: &backdrop.effect,
        rounded_mask: backdrop
            .rounded_clip
            .map(|clip| rounded_mask(clip, snap, scale)),
        batched: batched_effect(&backdrop.effect),
        stage: 0,
        support,
    })
}

fn is_transparent_clear(load_op: Option<wgpu::LoadOp<wgpu::Color>>) -> bool {
    matches!(load_op, Some(wgpu::LoadOp::Clear(color)) if color == wgpu::Color::TRANSPARENT)
}

fn texel_rect_in(rect: DeviceRect, within: DeviceRect) -> TexelRect {
    let x = (rect.x - within.x).max(0.0) as u32;
    let y = (rect.y - within.y).max(0.0) as u32;
    let (width, height) = within.pixel_size();
    (
        x,
        y,
        (rect.width as u32).min(width.saturating_sub(x)),
        (rect.height as u32).min(height.saturating_sub(y)),
    )
}

fn blit_read_rect(
    scissor: DeviceRect,
    capture_rect: DeviceRect,
    linear: bool,
) -> Option<DeviceRect> {
    scissor
        .expand(if linear { 1.0 } else { 0.0 })
        .intersect(capture_rect)
        .map(DeviceRect::snap_out)
}

fn domain_read_rect(
    effect: &RenderEffect,
    layer_rect: DeviceRect,
    capture_rect: DeviceRect,
    scale: f32,
) -> Option<DeviceRect> {
    let domain = declared_domain(effect.sample_domain())?;
    let read = DeviceRect {
        x: layer_rect.x + domain.x * scale,
        y: layer_rect.y + domain.y * scale,
        width: domain.width * scale,
        height: domain.height * scale,
    };
    read.expand(1.0)
        .intersect(capture_rect)
        .map(DeviceRect::snap_out)
}

fn effect_reads(
    effect: &RenderEffect,
    output: Option<DeviceRect>,
    layer_rect: DeviceRect,
    capture_rect: DeviceRect,
    scale: f32,
) -> EffectReads {
    EffectReads {
        output: output.map(|read| texel_rect_in(read, capture_rect)),
        shader_input: domain_read_rect(effect, layer_rect, capture_rect, scale)
            .map(|read| texel_rect_in(read, capture_rect)),
    }
}

fn member_read_texels(
    item: &PendingBackdrop<'_>,
    placement: AtlasPlacement,
    scale: f32,
) -> Option<TexelRect> {
    let read = match item.batched? {
        BatchedEffect::Blur(_) => blit_read_rect(
            item.support.unwrap_or(item.visible),
            item.capture_rect,
            true,
        )?,
        BatchedEffect::Shader(_) | BatchedEffect::BlurThenShader(..) => {
            domain_read_rect(item.effect, item.layer_rect, item.capture_rect, scale)?
        }
    };
    let (x, y, width, height) = texel_rect_in(read, item.capture_rect);
    Some((placement.x + x, placement.y + y, width, height))
}

/// The backdrop effects of one layer scene grouped into stages: an effect
/// joins the stage after every queued effect below it whose composite lies
/// under its capture, so every capture in a stage reads only composites of
/// earlier stages and the stage's captures share one pass.
#[derive(Default)]
struct ResolveStages<'a> {
    pending: Vec<PendingBackdrop<'a>>,
}

impl<'a> ResolveStages<'a> {
    fn push(&mut self, mut item: PendingBackdrop<'a>) {
        item.stage = self
            .pending
            .iter()
            .filter(|other| {
                other.z < item.z
                    && other
                        .support
                        .unwrap_or(other.visible)
                        .intersect(item.capture_rect)
                        .is_some()
            })
            .map(|other| other.stage + 1)
            .max()
            .unwrap_or(0);
        self.pending.push(item);
    }
}

/// Where a child lands in its parent: its z, the device pixels it may
/// touch, its device bounds and its snap.
#[derive(Clone, Copy)]
struct ChildPlacement {
    z: usize,
    visible: DeviceRect,
    support: DeviceRect,
    dest: DeviceRect,
    snap: Point,
}

/// A child's device bounds in its parent, and the part of them its clip and
/// the target leave visible.
fn child_device_placement(
    child: &ChildLayer,
    snap: Point,
    scale: f32,
    target_rect: DeviceRect,
) -> (DeviceRect, Option<DeviceRect>) {
    let dest_bounds_logical =
        quad_bounds(child.transform.map_rect(child.local_bounds)).translate(snap.x, snap.y);
    let dest = DeviceRect::from_logical(dest_bounds_logical, scale);
    let clipped = match child.clip {
        Some(clip) => dest.intersect(DeviceRect::from_logical(
            clip.translate(snap.x, snap.y),
            scale,
        )),
        None => Some(dest),
    };
    (dest, clipped.and_then(|rect| rect.intersect(target_rect)))
}

/// What the page shows of a child's rendered surface: the child's clip
/// within the target, and nothing narrower.
///
/// Not the child's own box. A surface holds what the child draws outside
/// itself as well -- the shadow it casts -- and [`child_surface_rect`] sizes
/// it to cover that. Rendering or compositing only the box's part of it
/// instead drops the ring of shadow around the child, which is a rectangle
/// of missing shadow exactly where the child sits.
fn child_surface_bound(
    child: &ChildLayer,
    snap: Point,
    scale: f32,
    target_rect: DeviceRect,
) -> Option<DeviceRect> {
    match child.clip {
        Some(clip) => {
            DeviceRect::from_logical(clip.translate(snap.x, snap.y), scale).intersect(target_rect)
        }
        None => Some(target_rect),
    }
}

/// The part of a backdrop-reading child's surface `whole` worth rendering:
/// what the page shows of it, `shown`, grown by `reach`, the pixels its
/// glasses read past what they cover.
///
/// `shown` is the child's clip within the target, never its box. The shadow
/// a child casts lies outside its box and inside its surface, and a press
/// that promotes the child to a surface must render that shadow whole:
/// trimmed to the box and the glass reach, it ends in a straight edge a few
/// pixels out from every side of the control.
fn rendered_surface(whole: DeviceRect, shown: DeviceRect, reach: f32) -> DeviceRect {
    shown
        .expand(reach)
        .intersect(whole)
        .map_or(whole, DeviceRect::snap_out)
}

/// Whether a child's runtime shader can draw in the final pass over the
/// child's content: the shader must apply the child's clip and alpha itself
/// unless the child has neither.
/// Whether the layer's content puts nothing on its surface.
fn draws_nothing(content: &LayerScene) -> bool {
    content.scene.draw_ops.is_empty()
        && content.scene.shadow_draws.is_empty()
        && content.children.is_empty()
        && content.scene.backdrop_layers.is_empty()
        && content.scene.effect_layers.is_empty()
}

fn composites_nothing(child: &ChildLayer) -> bool {
    child.blend_mode == BlendMode::SrcOver
        && (child.alpha == 0.0
            || (draws_nothing(&child.content)
                && child
                    .effect
                    .as_ref()
                    .is_none_or(RenderEffect::preserves_transparency)))
}

/// The child's layer bounds in the pixels of a surface at `surface_rect`.
fn layer_pixel_rect(child: &ChildLayer, surface_rect: DeviceRect, scale: f32) -> [f32; 4] {
    let bounds = DeviceRect::from_logical(child.local_bounds, scale);
    [
        bounds.x - surface_rect.x,
        bounds.y - surface_rect.y,
        bounds.width,
        bounds.height,
    ]
}

/// A runtime shader drawn in the final pass over `source`, the child's
/// content, at `dest`.
#[expect(clippy::too_many_arguments)]
fn shader_tail_composite(
    child: &ChildLayer,
    shader: &Arc<RuntimeShader>,
    z: usize,
    source: CompositeSource,
    dest: DeviceRect,
    layer_pixel_rect: [f32; 4],
    rounded_mask: Option<RoundedCompositeMask>,
    visible: DeviceRect,
) -> ResolvedComposite {
    ResolvedComposite {
        z_index: z,
        source: source.texture,
        content: source.content,
        dest: dest.tuple(),
        scissor: Some(visible.tuple()),
        kind: ResolvedCompositeKind::Shader {
            shader: Arc::clone(shader),
            layer_pixel_rect,
            source_region: None,
            source_logical_size: None,
            substrate_regions: [None; MAX_SUBSTRATES],
            rounded_mask,
            alpha: child.alpha,
        },
    }
}

const TRANSPARENT_SOURCE: &str = "transparent source";

fn capture_window(rect: DeviceRect) -> CaptureWindow {
    CaptureWindow {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

/// Whether a composite or a child drawn in place lies at or below `z`.
fn drawn_beneath(z: usize, composites: &[ResolvedComposite], first_in_place_z: usize) -> bool {
    first_in_place_z <= z || composites.iter().any(|composite| composite.z_index <= z)
}

/// The clear that replaces the flush's first op when it fills the whole
/// page with a solid color and nothing lies beneath it.
fn page_fill_clear(
    context: &PrefixContext<'_>,
    ops: &[DrawOp],
    composites: &[ResolvedComposite],
    first_in_place_z: usize,
) -> Option<wgpu::LoadOp<wgpu::Color>> {
    let (color, z) = page_fill_color(context, ops)?;
    (!drawn_beneath(z, composites, first_in_place_z)).then_some(wgpu::LoadOp::Clear(color))
}

fn hash_base<H: Hasher>(base: wgpu::LoadOp<wgpu::Color>, state: &mut H) {
    match base {
        wgpu::LoadOp::Clear(color) => {
            1u8.hash(state);
            for channel in [color.r, color.g, color.b, color.a] {
                channel.to_bits().hash(state);
            }
        }
        wgpu::LoadOp::Load => 0u8.hash(state),
        wgpu::LoadOp::DontCare(_) => 2u8.hash(state),
    }
}

/// The pixels a composite may touch: its destination within its scissor.
fn composite_coverage(composite: &ResolvedComposite) -> Option<DeviceRect> {
    let (x, y, width, height) = composite.dest;
    let dest = DeviceRect {
        x,
        y,
        width,
        height,
    };
    match composite.scissor {
        Some((sx, sy, sw, sh)) => dest.intersect(DeviceRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
        }),
        None => Some(dest),
    }
}

/// A backdrop's result blitted where the backdrop sits, through its rounded
/// mask and scissored to what is visible.
fn backdrop_blit(item: &PendingBackdrop<'_>, source: CompositeSource) -> ResolvedComposite {
    ResolvedComposite {
        z_index: item.z,
        source: source.texture,
        content: source.content,
        dest: item.capture_rect.tuple(),
        scissor: Some(item.support.unwrap_or(item.visible).tuple()),
        kind: ResolvedCompositeKind::Blit {
            alpha: item.alpha,
            blend_mode: BlendMode::SrcOver,
            rounded_mask: item.rounded_mask,
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: None,
        },
    }
}

/// A child whose surface lies on the parent's pixel grid, blitted one to one
/// at `dest`.
#[expect(clippy::too_many_arguments)]
fn grid_child_composite(
    child: &ChildLayer,
    z: usize,
    source: CompositeSource,
    region: Option<DeviceRect>,
    dest: DeviceRect,
    snap: Point,
    scale: f32,
    visible: DeviceRect,
) -> ResolvedComposite {
    ResolvedComposite {
        z_index: z,
        source: source.texture,
        content: source.content,
        dest: dest.tuple(),
        scissor: Some(visible.tuple()),
        kind: ResolvedCompositeKind::Blit {
            alpha: child.alpha,
            blend_mode: child.blend_mode,
            rounded_mask: grid_rounded_mask(child, snap, scale),
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: region.map(DeviceRect::tuple),
        },
    }
}

/// A transformed child's surface projected through its transform; none
/// when the transform cannot be inverted.
fn projected_child_composite(
    child: &ChildLayer,
    z: usize,
    source: CompositeSource,
    surface: &SurfaceRender,
    snap: Point,
    scale: f32,
    visible: DeviceRect,
) -> Option<ResolvedComposite> {
    let source_to_parent = surface_to_parent_device(surface, child.transform, snap, scale);
    let inverse = source_to_parent.inverse()?;
    let dest_quad = source_to_parent.map_rect(Rect {
        x: 0.0,
        y: 0.0,
        width: surface.rect.width,
        height: surface.rect.height,
    });
    Some(ResolvedComposite {
        z_index: z,
        source: source.texture,
        content: source.content,
        dest: quad_device_bounds(dest_quad).tuple(),
        scissor: Some(visible.tuple()),
        kind: ResolvedCompositeKind::Projective {
            dest_quad,
            inverse: inverse.matrix(),
            alpha: child.alpha,
            blend_mode: child.blend_mode,
            sample_mode: if renders_flat(child) {
                CompositeSampleMode::Texels
            } else {
                CompositeSampleMode::Linear
            },
            source_region: surface.region.map(DeviceRect::tuple),
        },
    })
}

fn replayed_kind(
    kind: &ResolvedCompositeKind,
    item: &PendingBackdrop<'_>,
) -> ResolvedCompositeKind {
    match kind {
        ResolvedCompositeKind::Blit {
            blend_mode,
            sample_mode,
            source_viewport,
            ..
        } => ResolvedCompositeKind::Blit {
            alpha: item.alpha,
            blend_mode: *blend_mode,
            rounded_mask: item.rounded_mask,
            sample_mode: *sample_mode,
            source_viewport: *source_viewport,
        },
        ResolvedCompositeKind::Shader {
            source_region,
            source_logical_size,
            substrate_regions,
            ..
        } => ResolvedCompositeKind::Shader {
            shader: Arc::clone(
                item.batched
                    .and_then(BatchedEffect::shader)
                    .expect("cached shader inputs"),
            ),
            layer_pixel_rect: item.layer_pixel_rect(),
            source_region: *source_region,
            source_logical_size: *source_logical_size,
            substrate_regions: *substrate_regions,
            rounded_mask: item.rounded_mask,
            alpha: item.alpha,
        },
        projective @ ResolvedCompositeKind::Projective { .. } => projective.clone(),
    }
}

/// The composite of every member of one capture atlas: a blur reads its
/// blurred region, downscaled by the blur and standing for the capture's
/// pixels, a shader reads its capture region.
fn stage_composites(
    texture: &Rc<OffscreenTarget>,
    side: Option<&StageSideRegions>,
    items: &[&PendingBackdrop<'_>],
    members: &[(usize, AtlasPlacement)],
    glass_as_blit: bool,
) -> Vec<ResolvedComposite> {
    members
        .iter()
        .enumerate()
        .map(|(member, (index, placement))| {
            let item = items[*index];
            let (capture_width, capture_height) = item.capture_rect.pixel_size();
            let capture_size = (capture_width as f32, capture_height as f32);
            let (source, region, logical_size) =
                match side.and_then(|side| side.blurred_slot(member)) {
                    Some((blurred, slot)) => (blurred, region_tuple(slot), Some(capture_size)),
                    None => (
                        texture,
                        (
                            placement.x as f32,
                            placement.y as f32,
                            capture_size.0,
                            capture_size.1,
                        ),
                        None,
                    ),
                };
            let substrate_regions =
                side.map_or([None; MAX_SUBSTRATES], |side| side.substrates[member]);
            let blit = ResolvedCompositeKind::Blit {
                alpha: item.alpha,
                blend_mode: BlendMode::SrcOver,
                rounded_mask: item.rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: Some(region),
            };
            let kind = match item.batched.expect("packed items are batched") {
                _ if glass_as_blit => blit,
                BatchedEffect::Blur(_) => ResolvedCompositeKind::Blit {
                    alpha: item.alpha,
                    blend_mode: BlendMode::SrcOver,
                    rounded_mask: item.rounded_mask,
                    sample_mode: CompositeSampleMode::Linear,
                    source_viewport: Some(region),
                },
                BatchedEffect::Shader(shader) | BatchedEffect::BlurThenShader(_, shader) => {
                    ResolvedCompositeKind::Shader {
                        shader: Arc::clone(shader),
                        layer_pixel_rect: item.layer_pixel_rect(),
                        source_region: Some(region),
                        source_logical_size: logical_size,
                        substrate_regions,
                        rounded_mask: item.rounded_mask,
                        alpha: item.alpha,
                    }
                }
            };
            ResolvedComposite {
                z_index: item.z,
                source: Rc::clone(source),
                content: SourceContent::Transient,
                dest: item.capture_rect.tuple(),
                scissor: Some(item.support.unwrap_or(item.visible).tuple()),
                kind,
            }
        })
        .collect()
}

/// A rectangle of texels: x, y, width, height.
type TexelRect = (u32, u32, u32, u32);

/// The three region lists `stage_substrate_regions` appends to while it walks
/// a stage's members.
/// Where a stage's side work is collected: its blur regions and averaged
/// regions, each with the atlas slot its result is wanted in, if any.
struct SideRegionSinks<'a> {
    regions: &'a mut Vec<BlurRegion>,
    region_slots: &'a mut Vec<Option<[u32; 2]>>,
    averaged: &'a mut Vec<SubstrateRegion>,
    average_slots: &'a mut Vec<Option<[u32; 2]>>,
}

fn stage_blur_regions(
    blurred: &[(usize, BlurSpec)],
    members: &[(usize, AtlasPlacement)],
    items: &[&PendingBackdrop<'_>],
    view: &AtlasView<'_>,
    scale: f32,
    sinks: &mut SideRegionSinks<'_>,
    slots: &mut Vec<Option<TexelRect>>,
) -> Result<(), String> {
    slots.resize(members.len(), None);
    for (member, blur) in blurred {
        let (index, placement) = members[*member];
        let (width, height) = items[index].capture_rect.pixel_size();
        let Some(scratch) = view.side(index).blur else {
            return Err("a blurred region outgrew the atlas that held it".into());
        };
        slots[*member] = Some(scratch);
        sinks.region_slots.push(None);
        sinks.regions.push(BlurRegion {
            source: (placement.x, placement.y, width, height),
            scratch,
            dest: scratch,
            radius_x: blur.radius_x,
            radius_y: blur.radius_y,
            tile_mode: blur.tile_mode,
            read: member_read_texels(items[index], placement, scale),
        });
    }
    Ok(())
}

fn mean_capture_rect(item: &PendingBackdrop<'_>) -> DeviceRect {
    item.layer_rect
        .intersect(item.capture_rect)
        .unwrap_or(item.capture_rect)
        .snap_out()
}

fn mean_source_region(item: &PendingBackdrop<'_>, placement: AtlasPlacement) -> TexelRect {
    let rect = mean_capture_rect(item);
    let (width, height) = rect.pixel_size();
    (
        placement.x + (rect.x - item.capture_rect.x).max(0.0) as u32,
        placement.y + (rect.y - item.capture_rect.y).max(0.0) as u32,
        width,
        height,
    )
}

fn stage_substrate_regions(
    members: &[(usize, AtlasPlacement)],
    items: &[&PendingBackdrop<'_>],
    view: &AtlasView<'_>,
    scale: f32,
    ablate: bool,
    sinks: SideRegionSinks<'_>,
    member_regions: &mut Vec<SubstrateRegions>,
) -> Result<(), String> {
    let SideRegionSinks {
        regions,
        region_slots,
        averaged,
        average_slots,
    } = sinks;
    member_regions.resize(members.len(), [None; MAX_SUBSTRATES]);
    for (member, (index, placement)) in members.iter().enumerate() {
        let (source_width, source_height) = items[*index].capture_rect.pixel_size();
        let source = (placement.x, placement.y, source_width, source_height);
        for (order, planned) in view.substrates(*index).iter().enumerate() {
            if ablate {
                member_regions[member][order] = Some(region_tuple(source));
                continue;
            }
            let (width, height) = planned.size;
            let Some(scratch) = view.side(*index).substrates.get(order).copied().flatten() else {
                return Err("a substrate outgrew the atlas that held it".into());
            };
            let read = member_read_texels(items[*index], *placement, scale);
            let slot = planned.atlas_slot.map(|(x, y, _, _)| [x, y]);
            match planned.spec {
                SubstrateSpec::Mean => {
                    averaged.push(SubstrateRegion {
                        source: mean_source_region(items[*index], *placement),
                        scratch,
                        dest: (scratch.0, scratch.1, 1, 1),
                        average: SubstrateAverage::Mean,
                        read: None,
                    });
                    average_slots.push(slot);
                }
                SubstrateSpec::Average { block } => {
                    averaged.push(SubstrateRegion {
                        source,
                        scratch,
                        dest: scratch,
                        average: SubstrateAverage::Block(block),
                        read,
                    });
                    average_slots.push(slot);
                }
                SubstrateSpec::Blur { radius_px } => {
                    regions.push(BlurRegion {
                        source,
                        scratch,
                        dest: scratch,
                        radius_x: radius_px,
                        radius_y: radius_px,
                        tile_mode: TileMode::Clamp,
                        read,
                    });
                    region_slots.push(slot);
                }
            }
            member_regions[member][order] = Some(region_tuple(match slot {
                Some([x, y]) => (x, y, width, height),
                None => (scratch.0, scratch.1, width, height),
            }));
        }
    }
    Ok(())
}

/// Points every blur and mean at its atlas slot when each has one and no
/// block average is among them, so the side passes draw into the atlas;
/// reports whether they do.
fn direct_side_slots(
    regions: &mut [BlurRegion],
    region_slots: &[Option<[u32; 2]>],
    averaged: &mut [SubstrateRegion],
    average_slots: &[Option<[u32; 2]>],
) -> bool {
    let direct = region_slots.iter().all(Option::is_some)
        && average_slots.iter().all(Option::is_some)
        && averaged
            .iter()
            .all(|substrate| matches!(substrate.average, SubstrateAverage::Mean));
    if direct {
        for (region, slot) in regions.iter_mut().zip(region_slots) {
            let [x, y] = slot.expect("every blur region has its atlas slot");
            region.dest = (x, y, region.scratch.2, region.scratch.3);
        }
        for (mean, slot) in averaged.iter_mut().zip(average_slots) {
            let [x, y] = slot.expect("every mean has its atlas slot");
            mean.dest = (x, y, 1, 1);
        }
    }
    direct
}

fn record_side_result_copies(
    recorder: &mut impl FrameCommandRecorder,
    result: &OffscreenTarget,
    atlas: &OffscreenTarget,
    regions: &[BlurRegion],
    region_slots: &[Option<[u32; 2]>],
    averaged: &[SubstrateRegion],
    average_slots: &[Option<[u32; 2]>],
) {
    for (region, slot) in regions.iter().zip(region_slots) {
        if let Some(dest_origin) = slot {
            recorder.copy_texture_region(TextureRegionCopy {
                source: result,
                source_origin: [region.scratch.0, region.scratch.1],
                dest: atlas,
                dest_origin: *dest_origin,
                size: [region.scratch.2, region.scratch.3],
            });
        }
    }
    for (substrate, slot) in averaged.iter().zip(average_slots) {
        if let Some(dest_origin) = slot {
            let size = match substrate.average {
                SubstrateAverage::Mean => [1, 1],
                SubstrateAverage::Block(_) => [substrate.scratch.2, substrate.scratch.3],
            };
            recorder.copy_texture_region(TextureRegionCopy {
                source: result,
                source_origin: [substrate.scratch.0, substrate.scratch.1],
                dest: atlas,
                dest_origin: *dest_origin,
                size,
            });
        }
    }
}

fn region_tuple((x, y, width, height): TexelRect) -> (f32, f32, f32, f32) {
    (x as f32, y as f32, width as f32, height as f32)
}

/// The texels a substrate of a capture occupies: a block average keeps one
/// texel per block, a blur its scratch size.
fn substrate_size(spec: SubstrateSpec, (width, height): (u32, u32)) -> (u32, u32) {
    match spec {
        SubstrateSpec::Mean => (1, 1),
        SubstrateSpec::Average { block } => {
            (width.div_ceil(block).max(1), height.div_ceil(block).max(1))
        }
        SubstrateSpec::Blur { radius_px } => substrate_scratch_size(radius_px, width, height),
    }
}

/// A substrate a stage packs for one of its shader members: its size, and
/// the slot of the atlas that receives it when the shader reads the atlas;
/// a shader after a blur reads its input in the result texture and finds
/// the substrate there.
#[derive(Clone, Copy)]
struct PlannedSubstrate {
    spec: SubstrateSpec,
    size: (u32, u32),
    work_size: (u32, u32),
    atlas_slot: Option<TexelRect>,
}

type PlannedSubstrates = SmallVec<[PlannedSubstrate; MAX_SUBSTRATES]>;

#[derive(Clone, Default)]
struct SideSlots {
    blur: Option<TexelRect>,
    substrates: SmallVec<[Option<TexelRect>; MAX_SUBSTRATES]>,
}

struct AtlasView<'a> {
    layout: &'a StageLayout,
    atlas: usize,
    members: Vec<(usize, AtlasPlacement)>,
}

impl AtlasView<'_> {
    fn size(&self) -> (u32, u32) {
        self.layout.atlas_sizes[self.atlas]
    }

    fn side_size(&self) -> (u32, u32) {
        self.layout.side_sizes[self.atlas]
    }

    fn substrates(&self, index: usize) -> &[PlannedSubstrate] {
        &self.layout.substrates[index]
    }

    fn side(&self, index: usize) -> &SideSlots {
        &self.layout.side[index]
    }
}

struct StageLayout {
    atlas_sizes: Vec<(u32, u32)>,
    placements: Vec<Option<AtlasPlacement>>,
    substrates: Vec<PlannedSubstrates>,
    side_sizes: Vec<(u32, u32)>,
    side: Vec<SideSlots>,
}

impl StageLayout {
    fn signature(&self, index: usize) -> u64 {
        let mut hasher = capture_hasher();
        match self.placements[index] {
            Some(placement) => {
                1u8.hash(&mut hasher);
                self.atlas_sizes[placement.atlas].hash(&mut hasher);
                (placement.x, placement.y).hash(&mut hasher);
                self.side_sizes[placement.atlas].hash(&mut hasher);
            }
            None => 0u8.hash(&mut hasher),
        }
        for planned in &self.substrates[index] {
            match planned.spec {
                SubstrateSpec::Mean => 2u8.hash(&mut hasher),
                SubstrateSpec::Average { block } => {
                    0u8.hash(&mut hasher);
                    block.hash(&mut hasher);
                }
                SubstrateSpec::Blur { radius_px } => {
                    1u8.hash(&mut hasher);
                    radius_px.to_bits().hash(&mut hasher);
                }
            }
            planned.size.hash(&mut hasher);
            planned.atlas_slot.hash(&mut hasher);
        }
        self.side[index].blur.hash(&mut hasher);
        self.side[index].substrates.hash(&mut hasher);
        hasher.finish()
    }

    fn atlas_views(&self) -> impl Iterator<Item = AtlasView<'_>> {
        (0..self.atlas_sizes.len()).map(|atlas| AtlasView {
            layout: self,
            atlas,
            members: self
                .placements
                .iter()
                .enumerate()
                .filter_map(|(index, placement)| {
                    placement
                        .filter(|placement| placement.atlas == atlas)
                        .map(|placement| (index, placement))
                })
                .collect(),
        })
    }
}

/// What a stage renders beside its capture atlas: the texture holding the
/// blurred regions, per atlas member the downscaled slot its blur wrote,
/// and per member the regions of its substrates in the texture it reads.
struct StageSideRegions<'a> {
    result: Rc<OffscreenTarget>,
    blurred: &'a [Option<TexelRect>],
    substrates: &'a [SubstrateRegions],
}

impl StageSideRegions<'_> {
    fn blurred_slot(&self, member: usize) -> Option<(&Rc<OffscreenTarget>, TexelRect)> {
        self.blurred[member].map(|slot| (&self.result, slot))
    }
}

#[derive(Default)]
pub(crate) struct StageSideScratch {
    blurred: Vec<(usize, BlurSpec)>,
    regions: Vec<BlurRegion>,
    region_slots: Vec<Option<[u32; 2]>>,
    averaged: Vec<SubstrateRegion>,
    average_slots: Vec<Option<[u32; 2]>>,
    slots: Vec<Option<TexelRect>>,
    member_regions: Vec<SubstrateRegions>,
}

impl StageSideScratch {
    fn clear(&mut self) {
        self.blurred.clear();
        self.regions.clear();
        self.region_slots.clear();
        self.averaged.clear();
        self.average_slots.clear();
        self.slots.clear();
        self.member_regions.clear();
    }
}

#[derive(Clone, Copy)]
struct AtlasPlacement {
    atlas: usize,
    x: u32,
    y: u32,
}

struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

#[derive(Default)]
struct Atlas {
    width: u32,
    height: u32,
    shelves: Vec<Shelf>,
}

impl Atlas {
    fn padded_size(&self, limit: u32) -> (u32, u32) {
        (
            padded_dimension(self.width, limit),
            padded_dimension(self.height, limit),
        )
    }
}

fn padded_dimension(value: u32, limit: u32) -> u32 {
    let step = (value.max(ATLAS_SIZE_STEP).next_power_of_two() / 8).max(ATLAS_SIZE_STEP);
    value.max(1).div_ceil(step).saturating_mul(step).min(limit)
}

/// Shelf packing of regions edge to edge into as few atlases as the
/// dimension limit allows. Nothing separates neighbours: every reader of a
/// region holds its samples to the region's own texel centers, so a
/// neighbour's texels are never read and an atlas needs no clearing.
struct AtlasPacker {
    limit: u32,
    shelf_width: u32,
    atlases: Vec<Atlas>,
}

impl AtlasPacker {
    fn new(limit: u32) -> Self {
        Self {
            limit,
            shelf_width: limit,
            atlases: Vec::new(),
        }
    }

    /// A packer whose shelves end at `width`, or at the limit when that is
    /// narrower, so its atlases grow down rather than out to the limit.
    fn with_shelf_width(self, width: u32) -> Self {
        Self {
            shelf_width: width.clamp(1, self.limit),
            ..self
        }
    }

    fn place(&mut self, width: u32, height: u32) -> Option<AtlasPlacement> {
        if width > self.limit || height > self.limit {
            return None;
        }
        for (atlas_index, atlas) in self.atlases.iter_mut().enumerate() {
            for shelf in &mut atlas.shelves {
                if shelf.height >= height && shelf.x + width <= self.shelf_width.max(width) {
                    let placement = AtlasPlacement {
                        atlas: atlas_index,
                        x: shelf.x,
                        y: shelf.y,
                    };
                    shelf.x += width;
                    atlas.width = atlas.width.max(shelf.x);
                    return Some(placement);
                }
            }
            if atlas.height + height <= self.limit {
                let placement = AtlasPlacement {
                    atlas: atlas_index,
                    x: 0,
                    y: atlas.height,
                };
                atlas.shelves.push(Shelf {
                    y: atlas.height,
                    height,
                    x: width,
                });
                atlas.height += height;
                atlas.width = atlas.width.max(width);
                return Some(placement);
            }
        }
        self.atlases.push(Atlas {
            width,
            height,
            shelves: vec![Shelf {
                y: 0,
                height,
                x: width,
            }],
        });
        Some(AtlasPlacement {
            atlas: self.atlases.len() - 1,
            x: 0,
            y: 0,
        })
    }
}

pub(crate) struct FrameExecutor<'r, 'c, C: FrameCommandRecorder> {
    renderer: &'r mut GpuRenderer,
    recorder: &'c mut C,
    transients: Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
    empty_scene: CompositorScene,
    depth: usize,
    prefix_admitted_pixels: u64,
}

const MAX_ADMISSION_PATIENCE: u32 = 16;
/// The frames a layer that can draw in place holds its content still before
/// its surface is kept.
const IN_PLACE_PATIENCE: u32 = MAX_ADMISSION_PATIENCE;
/// The longest that wait grows after surfaces kept for such a layer went
/// unread: about four seconds at 60 Hz.
const IN_PLACE_MAX_PATIENCE: u32 = 16 * IN_PLACE_PATIENCE;

/// The reads a kept surface needs to pay for itself. Keeping it costs a
/// copy or a draw into a surface of its own, and the texture it holds; read
/// once and then replaced -- a cell whose size holds two or three frames of
/// a relayout -- it cost about what drawing it again would have, and its
/// texture churned the pool.
const READS_TO_PAY: u32 = 2;

enum AdmissionCost {
    Pin,
    /// A kept surface costs a copy. `patience` is the frames the content
    /// holds still before its surface is kept: it doubles, up to `ceiling`,
    /// whenever a kept surface was replaced before it was read
    /// [`READS_TO_PAY`] times, and halves, down to `floor`, when one was.
    Copy {
        patience: u32,
        floor: u32,
        ceiling: u32,
    },
}

pub(crate) struct AdmissionGate {
    key: LayerRasterCacheKey,
    run: u32,
    cost: AdmissionCost,
    admitted: bool,
    /// Frames the kept surface of the current key has been read.
    reads: u32,
    seen: bool,
}

impl AdmissionGate {
    fn pinned(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(key, AdmissionCost::Pin)
    }

    /// A gate for a surface kept by copying it out of the pass it was drawn
    /// in: kept once its content has held still for a second frame.
    fn copied(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: 1,
                floor: 1,
                ceiling: MAX_ADMISSION_PATIENCE,
            },
        )
    }

    /// A gate for a surface rendered into a texture of its own, kept from
    /// its first sight: still content reads it back the next frame.
    fn rendered(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: 0,
                floor: 0,
                ceiling: MAX_ADMISSION_PATIENCE,
            },
        )
    }

    /// A gate for a surface its layer can do without by drawing in place:
    /// the surface is kept only once its content has held still for
    /// `IN_PLACE_PATIENCE` frames. Drawing in place meanwhile costs nothing
    /// a surface would save, and a shorter hold -- a relayout pausing at the
    /// turn of its motion -- would keep surfaces read for a frame or two.
    /// A pause longer than that at every turn keeps surfaces nothing reads,
    /// so each one that goes unread doubles the wait, up to
    /// `IN_PLACE_MAX_PATIENCE`.
    fn drawn_in_place(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: IN_PLACE_PATIENCE,
                floor: IN_PLACE_PATIENCE,
                ceiling: IN_PLACE_MAX_PATIENCE,
            },
        )
    }

    fn with_cost(key: LayerRasterCacheKey, cost: AdmissionCost) -> Self {
        Self {
            key,
            run: 1,
            cost,
            admitted: false,
            reads: 0,
            seen: true,
        }
    }

    fn observe(&mut self, key: LayerRasterCacheKey) -> Option<LayerRasterCacheKey> {
        self.seen = true;
        if self.key == key {
            self.run = self.run.saturating_add(1);
            return None;
        }
        if let AdmissionCost::Copy {
            patience, ceiling, ..
        } = &mut self.cost
            && self.admitted
            && self.reads < READS_TO_PAY
        {
            *patience = (*patience * 2).clamp(1, *ceiling);
        }
        let dead = self.dead_entry();
        self.admitted = false;
        self.reads = 0;
        self.key = key;
        self.run = 1;
        dead
    }

    pub(crate) fn dead_entry(&self) -> Option<LayerRasterCacheKey> {
        let dead = match self.cost {
            AdmissionCost::Pin => self.admitted,
            AdmissionCost::Copy { .. } => self.admitted && self.reads == 0,
        };
        dead.then_some(self.key)
    }

    fn admits(&self) -> bool {
        match self.cost {
            AdmissionCost::Pin => true,
            AdmissionCost::Copy { patience, .. } => self.run > patience,
        }
    }

    fn admitted(&mut self) {
        self.admitted = true;
        self.reads = 0;
    }

    fn hit(&mut self, key: LayerRasterCacheKey) {
        self.observe(key);
        self.reads = self.reads.saturating_add(1);
        // A kept surface that paid for itself halves the wait, once: content
        // that holds still at the turns of a motion and churns between them
        // keeps a wait the churn does not get past.
        if let AdmissionCost::Copy {
            patience, floor, ..
        } = &mut self.cost
            && self.reads == READS_TO_PAY
        {
            *patience = (*patience / 2).max(*floor);
        }
    }

    pub(crate) fn end_frame(&mut self) -> bool {
        std::mem::take(&mut self.seen)
    }
}

/// A texture a composite draws and what it holds.
#[derive(Clone)]
struct CompositeSource {
    texture: Rc<OffscreenTarget>,
    content: SourceContent,
}

struct SurfaceRender {
    source: CompositeSource,
    rect: DeviceRect,
    scale: f32,
    grid_dest: Option<DeviceRect>,
    region: Option<DeviceRect>,
}

#[derive(Clone, Copy)]
struct ChildFrame {
    snap: Point,
    grid: Option<Point>,
    translation: Option<Point>,
    dest: DeviceRect,
    visible: Option<DeviceRect>,
}

/// The logical delta that lands a child on its snap anchor at `scale`.
fn child_snap(child: &ChildLayer, scale: f32) -> Point {
    child
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default()
}

/// Where a flush draws the children it draws in place: its page's scale,
/// device rect, size in pixels and offset.
struct InPlaceTarget {
    scale: f32,
    rect: DeviceRect,
    size: (u32, u32),
    offset: [f32; 2],
}

/// A stretch of a flush drawn as one segment: the page's own ops and
/// composites in a z range, or ops of a child drawn in place at its scale,
/// under the transform composed down to it and its clip's scissor.
enum FlushPart<'s> {
    Page {
        ops: Range<usize>,
        composites: Range<usize>,
    },
    InPlace {
        scene: &'s CompositorScene,
        ops: &'s [DrawOp],
        transform: SegmentTransform,
        scale: f32,
        scissor: Option<(u32, u32, u32, u32)>,
    },
}

/// The parts of a flush of `ops` and `composites` with the children at
/// `in_place` drawn between them at their z, and whether every nested child
/// was within the resolve depth.
fn flush_parts<'s>(
    layer: &'s LayerScene,
    ops: &[DrawOp],
    composites: &[ResolvedComposite],
    in_place: &[usize],
    target: &InPlaceTarget,
    depth: usize,
) -> (Vec<FlushPart<'s>>, bool) {
    let mut parts = Vec::with_capacity(in_place.len() * 2 + 1);
    let mut complete = true;
    let (mut op_start, mut composite_start) = (0, 0);
    for &index in in_place {
        let child = &layer.children[index];
        let op_end = ops
            .partition_point(|op| op.z_index < child.z_index)
            .max(op_start);
        let composite_end = composites
            .partition_point(|composite| composite.z_index < child.z_index)
            .max(composite_start);
        push_page_part(&mut parts, op_start..op_end, composite_start..composite_end);
        (op_start, composite_start) = (op_end, composite_end);
        let snap = child_snap(child, target.scale);
        let Some(shown) = child_surface_bound(child, snap, target.scale, target.rect) else {
            continue;
        };
        let scissor = match child.clip {
            Some(_) => match scissor_in_target(shown.tuple(), target.size, target.offset) {
                Some(scissor) => Some(scissor),
                None => continue,
            },
            None => None,
        };
        complete &= push_in_place(
            &mut parts,
            child,
            SegmentTransform::IDENTITY,
            target.scale,
            scissor,
            depth,
        );
    }
    push_page_part(
        &mut parts,
        op_start..ops.len(),
        composite_start..composites.len(),
    );
    (parts, complete)
}

/// What one flush draws: the page's ops and composites, the children it
/// draws in place, and the window of the first run a replayed prefix left.
struct Flush<'a> {
    ops: &'a [DrawOp],
    composites: &'a [ResolvedComposite],
    in_place: &'a [usize],
    first_run_window: Option<Range<u32>>,
}

/// The segments a flush's parts draw as, in order, at the page's offset.
fn flush_segments<'a>(
    scene: &'a CompositorScene,
    parts: &[FlushPart<'a>],
    flush: &Flush<'a>,
    offset: [f32; 2],
    scale: f32,
) -> Vec<PassSegment<'a>> {
    parts
        .iter()
        .map(|part| match part {
            FlushPart::Page { ops, composites } => PassSegment {
                scene,
                ops: &flush.ops[ops.clone()],
                composites: &flush.composites[composites.clone()],
                offset,
                scissor: None,
                first_run_window: (ops.start == 0)
                    .then(|| flush.first_run_window.clone())
                    .flatten(),
                transform: SegmentTransform::IDENTITY,
                scale,
            },
            FlushPart::InPlace {
                scene,
                ops,
                transform,
                scale,
                scissor,
            } => PassSegment {
                scene,
                ops,
                composites: &[],
                offset,
                scissor: *scissor,
                first_run_window: None,
                transform: *transform,
                scale: *scale,
            },
        })
        .collect()
}

fn push_page_part(parts: &mut Vec<FlushPart<'_>>, ops: Range<usize>, composites: Range<usize>) {
    if !ops.is_empty() || !composites.is_empty() {
        parts.push(FlushPart::Page { ops, composites });
    }
}

/// Adds a child drawn in place into a target at `scale` to a flush: its ops,
/// split around its children (which draw in place too), at its own scale
/// under its transform composed onto `outer`. `false` when the nesting
/// passes the resolve depth and the layers below it are left out.
fn push_in_place<'s>(
    parts: &mut Vec<FlushPart<'s>>,
    child: &'s ChildLayer,
    outer: SegmentTransform,
    scale: f32,
    scissor: Option<(u32, u32, u32, u32)>,
    depth: usize,
) -> bool {
    if depth >= MAX_RESOLVE_DEPTH {
        return false;
    }
    let Some((transform, content_scale)) = in_place_transform(child, scale) else {
        return true;
    };
    let transform = transform.then(outer);
    let scene = &child.content.scene;
    let ops = scene.draw_ops.as_slice();
    let push_ops = |parts: &mut Vec<FlushPart<'s>>, ops: &'s [DrawOp]| {
        if !ops.is_empty() {
            parts.push(FlushPart::InPlace {
                scene,
                ops,
                transform,
                scale: content_scale,
                scissor,
            });
        }
    };
    let mut complete = true;
    let mut start = 0;
    for grandchild in &child.content.children {
        let end = ops
            .partition_point(|op| op.z_index < grandchild.z_index)
            .max(start);
        push_ops(parts, &ops[start..end]);
        complete &= push_in_place(
            parts,
            grandchild,
            transform,
            content_scale,
            scissor,
            depth + 1,
        );
        start = end;
    }
    push_ops(parts, &ops[start..]);
    complete
}

/// The scale a child drawn in place into a target at `scale` draws its
/// content at, and the map from that content's device space into the
/// target's: the turn and move a projective composite of its surface
/// applies, its even scale taken into the content's scale, as Skia redraws a
/// scaled render node, so glyphs and edges raster at the size they show.
/// `None` for a transform that does more than scale evenly, turn and move.
fn in_place_transform(child: &ChildLayer, scale: f32) -> Option<(SegmentTransform, f32)> {
    let snap = child_snap(child, scale);
    let content = similarity_scale(child.transform)
        .and_then(|uniform| in_place_content_scale(uniform, child.surface_scale))?;
    let [[a, b, x], [c, d, y], _] = child.transform.matrix();
    let turn = [a / content, b / content, c / content, d / content];
    SegmentTransform::affine(turn, [(x + snap.x) * scale, (y + snap.y) * scale])
        .map(|transform| (transform, scale * content))
}

impl ChildFrame {
    fn of(child: &ChildLayer, scale: f32, target: DeviceRect) -> Self {
        let snap = child_snap(child, scale);
        let grid = uniform_scale_translation(child.transform)
            .filter(|(uniform, _)| (uniform - child.surface_scale).abs() <= 1e-4)
            .map(|(_, translation)| Point::new(translation.x + snap.x, translation.y + snap.y));
        let translation = grid.filter(|_| (child.surface_scale - 1.0).abs() <= 1e-4);
        let (dest, visible) = child_device_placement(child, snap, scale, target);
        Self {
            snap,
            grid,
            translation,
            dest,
            visible,
        }
    }
}

struct SurfacePlan {
    surface_logical: Rect,
    surface_rect: DeviceRect,
    grid_dest: Option<DeviceRect>,
    grid_offset: Option<Point>,
    device_phase: Point,
    surface_scale: f32,
    translated: bool,
    width: u32,
    height: u32,
}

impl SurfacePlan {
    fn of(child: &ChildLayer, scale: f32, grid: Option<Point>, shown: DeviceRect) -> Option<Self> {
        let surface_scale = scale * child.surface_scale;
        let translated = (child.surface_scale - 1.0).abs() <= 1e-4;
        let surface_logical = child_surface_rect(child, scale)?;
        let child_rect = DeviceRect::from_logical(surface_logical, surface_scale).snap_out();
        let grid_offset = grid.map(|grid| {
            let offset = Point::new(grid.x * scale, grid.y * scale);
            if translated {
                Point::new(offset.x.round(), offset.y.round())
            } else {
                offset
            }
        });
        let (surface_rect, grid_dest, device_phase) = match grid_offset {
            Some(offset) => {
                let whole = child_rect.translated(offset).snap_out();
                let dest = if child.content.contains_backdrop() && child.effect.is_none() {
                    let reach = (backdrop_reach(&child.content) * surface_scale).ceil() + 1.0;
                    rendered_surface(whole, shown, reach)
                } else {
                    whole
                };
                (
                    dest.translated(Point::new(-offset.x, -offset.y)),
                    Some(dest),
                    Point::new(offset.x - offset.x.floor(), offset.y - offset.y.floor()),
                )
            }
            None => (child_rect, None, Point::default()),
        };
        let (width, height) = surface_rect.pixel_size();
        if u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS {
            log::error!(
                "[layer] dropping a layer whole: {width}x{height} is past the {MAX_SURFACE_PIXELS} pixel budget, \
                 and its own {:.0}x{:.0} box does not fit either. Nothing it draws reaches the frame.",
                child.local_bounds.width,
                child.local_bounds.height,
            );
            return None;
        }
        Some(Self {
            surface_logical,
            surface_rect,
            grid_dest,
            grid_offset,
            device_phase,
            surface_scale,
            translated,
            width,
            height,
        })
    }

    fn cache_key(&self, child: &ChildLayer) -> Option<LayerRasterCacheKey> {
        (!child.content.contains_backdrop() && child.cache_policy == CachePolicy::Auto).then(|| {
            LayerRasterCacheKey::source_content(
                child.node_id,
                child.content_hash,
                self.surface_logical,
                (self.width, self.height),
                RasterScale::from_scale(self.surface_scale),
                self.device_phase,
            )
        })
    }

    fn surface(&self, source: CompositeSource, region: Option<DeviceRect>) -> SurfaceRender {
        SurfaceRender {
            source,
            rect: self.surface_rect,
            scale: self.surface_scale,
            grid_dest: self.grid_dest,
            region,
        }
    }
}

/// How a child resolved ahead of its z draws: in place, or through a
/// surface, none when it has nothing to show.
enum Resolved {
    InPlace,
    Surface(Option<SurfaceRender>),
}

enum SourceDecision {
    Cached(SurfaceRender),
    Render(Option<LayerRasterCacheKey>),
}

struct BatchMember {
    index: usize,
    plan: SurfacePlan,
    retain: Option<LayerRasterCacheKey>,
}

fn renders_flat(child: &ChildLayer) -> bool {
    child.effect.is_none()
        && !child.reads_backdrop()
        && child.content.children.is_empty()
        && child.content.scene.backdrop_layers.is_empty()
        && child.content.scene.effect_layers.is_empty()
        && child.content.scene.shadow_draws.is_empty()
}

fn z_ordered_ops(ops: &[DrawOp]) -> Cow<'_, [DrawOp]> {
    let ops = filtered_ops(ops, usize::MAX, &[]);
    if ops.is_sorted_by_key(draw_op_z) {
        return ops;
    }
    let mut ops = ops.into_owned();
    ensure_sorted_by_key(&mut ops, draw_op_z);
    Cow::Owned(ops)
}

enum Event {
    Child(usize),
    Backdrop(usize),
    Effect(usize),
    Shadow(usize),
}

impl<'r, 'c, C: FrameCommandRecorder> FrameExecutor<'r, 'c, C> {
    pub(crate) fn new(renderer: &'r mut GpuRenderer, recorder: &'c mut C) -> Self {
        let ablation = Ablation::current();
        let changed = ablation != renderer.ablation;
        renderer.ablation_frames = if changed {
            0
        } else {
            renderer.ablation_frames.wrapping_add(1)
        };
        if ablation != Ablation::default()
            && renderer.ablation_frames.is_multiple_of(ABLATION_LOG_PERIOD)
            || changed
        {
            log::warn!("[ablation] CRANPOSE_ABLATE switches: {ablation:?}");
        }
        renderer.ablation = ablation;
        renderer
            .effect_renderer
            .shader_cache
            .set_forced_flags(ablation.glass_flags.forced_flags());
        Self {
            renderer,
            recorder,
            transients: Vec::new(),
            empty_scene: CompositorScene::new(),
            depth: 0,
            prefix_admitted_pixels: 0,
        }
    }

    /// Renders the root scene into the frame's page, then the overlay on top
    /// of it.
    pub(crate) fn render_frame(
        mut self,
        root: &LayerScene,
        overlay: Option<&LayerScene>,
        page: Rc<OffscreenTarget>,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        let page = Page {
            texture: page,
            offset: [0.0, 0.0],
        };
        let beneath = Beneath::over(load_op);
        self.render_layer(root, page.clone(), root_scale, load_op, &beneath)?;
        if let Some(overlay) = overlay {
            let beneath = Beneath::over(wgpu::LoadOp::Load);
            self.render_layer(
                overlay,
                page.clone(),
                root_scale,
                wgpu::LoadOp::Load,
                &beneath,
            )?;
        }
        for _ in 0..PROBE_PASSES.parse::<u32>().unwrap_or(0) {
            self.renderer.empty_pass(
                self.recorder,
                "Probe Pass",
                page.pass_target().view,
                wgpu::LoadOp::Load,
            );
        }
        self.probe_draw_passes(&page, root_scale)?;
        self.release_transients();
        Ok(())
    }

    fn probe_draw_passes(&mut self, page: &Page, root_scale: f32) -> Result<(), String> {
        let count = PROBE_DRAW_PASSES.parse::<u32>().unwrap_or(0);
        if count == 0 {
            return Ok(());
        }
        let blank = self.acquire_transient("Probe Blank", 1, 1);
        self.renderer.clear_target(
            self.recorder,
            &blank.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        let texel = DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let blit = page_blit(&blank, [0.0, 0.0], texel, page.rect());
        let segment = PassSegment {
            scene: &self.empty_scene,
            ops: &[],
            composites: std::slice::from_ref(&blit),
            offset: page.offset,
            scissor: None,
            first_run_window: None,
            transform: SegmentTransform::IDENTITY,
            scale: root_scale,
        };
        for _ in 0..count {
            self.renderer.encode_pass(
                self.recorder,
                page.pass_target(),
                std::slice::from_ref(&segment),
                wgpu::LoadOp::Load,
                "Probe Draw Pass",
            )?;
        }
        Ok(())
    }

    /// Whether `effect` over a `source_size` source draws a layer at
    /// `layer_pixel_rect` without waiting for a pipeline to compile, its
    /// last shader `composited` over what is beneath or drawn into a
    /// texture. When it does not, what it misses is queued and the frame
    /// draws without it.
    fn effect_draws_now(
        &mut self,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
        source_size: (u32, u32),
        composited: bool,
    ) -> bool {
        let renderer = &mut *self.renderer;
        let ready = renderer.effect_renderer.effect_ready(
            &renderer.device,
            effect,
            crate::effect_renderer::layer_pixels(layer_pixel_rect),
            source_size,
            composited,
        );
        if !ready {
            renderer.note_placeholder();
        }
        ready
    }

    fn backdrop_draws_now(&mut self, item: &PendingBackdrop<'_>) -> bool {
        self.effect_draws_now(
            item.effect,
            item.layer_pixel_rect(),
            item.capture_rect.pixel_size(),
            item.composites_shader(),
        )
    }

    /// The backdrop's placeholder, drawn at `scale`, while its effect's
    /// pipelines compile; nothing for an effect without one.
    fn backdrop_placeholder(
        &mut self,
        item: &PendingBackdrop<'_>,
        scale: f32,
    ) -> Option<ResolvedComposite> {
        let placeholder = item.effect.placeholder()?;
        Some(self.placeholder_fill(
            placeholder.color,
            item.z,
            item.layer_rect,
            item.support.unwrap_or(item.visible),
            item.alpha,
            placeholder_mask(placeholder, item.layer_rect, scale).or(item.rounded_mask),
        ))
    }

    /// The placeholder of `effect` over a child at `layer` that draws nothing
    /// but the effect, while the effect's pipelines compile; none when the
    /// effect has none, or the child's transform turns its rectangle.
    fn empty_layer_placeholder(
        &mut self,
        child: &ChildLayer,
        effect: &RenderEffect,
        layer: DeviceRect,
        visible: DeviceRect,
        snap: Point,
        scale: f32,
    ) -> Option<ResolvedComposite> {
        let placeholder = effect.placeholder()?;
        uniform_scale_translation(child.transform)?;
        let mask = placeholder_mask(placeholder, layer, scale)
            .or_else(|| grid_rounded_mask(child, snap, scale));
        Some(self.placeholder_fill(
            placeholder.color,
            child.z_index,
            layer,
            visible,
            child.alpha,
            mask,
        ))
    }

    /// `dest` filled with `color` within `rounded_mask`, in place of an
    /// effect whose pipelines compile.
    fn placeholder_fill(
        &mut self,
        Color(r, g, b, a): Color,
        z: usize,
        dest: DeviceRect,
        visible: DeviceRect,
        alpha: f32,
        rounded_mask: Option<RoundedCompositeMask>,
    ) -> ResolvedComposite {
        let source = self.acquire_transient("Placeholder", 1, 1);
        self.renderer.clear_target(
            self.recorder,
            &source.view,
            wgpu::LoadOp::Clear(wgpu::Color {
                r: f64::from(r),
                g: f64::from(g),
                b: f64::from(b),
                a: f64::from(a),
            }),
        );
        ResolvedComposite {
            z_index: z,
            source,
            content: SourceContent::Transient,
            dest: dest.tuple(),
            scissor: Some(visible.tuple()),
            kind: ResolvedCompositeKind::Blit {
                alpha,
                blend_mode: BlendMode::SrcOver,
                rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: None,
            },
        }
    }

    fn release_transients(&mut self) {
        for (descriptor, target) in self.transients.drain(..) {
            if let Ok(target) = Rc::try_unwrap(target) {
                self.recorder
                    .release_transient_offscreen(descriptor, target);
            }
        }
    }

    fn acquire_transient(
        &mut self,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> Rc<OffscreenTarget> {
        let max = self.renderer.max_texture_dim();
        self.acquire_described(FrameTextureDescriptor::render_attachment(
            label,
            width.min(max),
            height.min(max),
            self.renderer.composition_format,
        ))
    }

    fn acquire_described(&mut self, descriptor: FrameTextureDescriptor) -> Rc<OffscreenTarget> {
        let target = self
            .recorder
            .acquire_transient_offscreen(&self.renderer.device, descriptor);
        let target = Rc::new(target);
        self.transients.push((descriptor, Rc::clone(&target)));
        target
    }

    /// Draws the layer into its page in strata: every backdrop, isolated
    /// child, effect range and blurred shadow resolves into a texture, and
    /// the page is drawn up to the lowest backdrop of each stage before that
    /// stage captures, so a capture reads pixels the page already holds and
    /// draws nothing beneath its glass twice.
    fn render_layer(
        &mut self,
        layer: &LayerScene,
        page: Page,
        scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        beneath: &Beneath<'_>,
    ) -> Result<(), String> {
        if self.depth >= MAX_RESOLVE_DEPTH {
            self.report_nesting_overflow();
            if matches!(load_op, wgpu::LoadOp::Clear(_)) {
                self.renderer
                    .clear_target(self.recorder, &page.texture.view, load_op);
            }
            return Ok(());
        }
        self.depth += 1;
        let result = self.render_layer_inner(layer, page, scale, load_op, beneath);
        self.depth -= 1;
        result
    }

    fn report_nesting_overflow(&mut self) {
        if !self.renderer.nesting_overflow_reported {
            self.renderer.nesting_overflow_reported = true;
            log::error!(
                "[layer] isolated layers nest deeper than {MAX_RESOLVE_DEPTH}: the layers below \
                 that depth draw nothing"
            );
        }
    }

    fn render_layer_inner(
        &mut self,
        layer: &LayerScene,
        page: Page,
        scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        beneath: &Beneath<'_>,
    ) -> Result<(), String> {
        let scene = &layer.scene;
        let mut pass = LayerPass {
            layer,
            page,
            scale,
            beneath,
            drawn: Vec::new(),
            pending: Vec::new(),
            deferred: Vec::new(),
            blockers: Vec::new(),
            excluded: Vec::new(),
            stages: ResolveStages::default(),
            drawn_z: 0,
            load_op: Some(load_op),
            segments: 0,
            resolved: Vec::new(),
            in_place_allowed: !layer.contains_backdrop() && scene.effect_layers.is_empty(),
            in_place: Vec::new(),
        };
        let target_rect = pass.target_rect();
        let capture_bounds = pass.capture_bounds();
        self.resolve_flat_children(&mut pass)?;

        for (z, event) in layer_events(layer) {
            match event {
                Event::Backdrop(index) => {
                    let backdrop = &scene.backdrop_layers[index];
                    if !self.renderer.ablation.stages
                        && let Some(item) =
                            plan_backdrop(backdrop, z, scale, target_rect, capture_bounds)
                    {
                        if self.backdrop_draws_now(&item) {
                            pass.stages.push(item);
                        } else if let Some(placeholder) = self.backdrop_placeholder(&item, scale) {
                            pass.pending.push(placeholder);
                        }
                    }
                }
                Event::Shadow(index) => {
                    let shadow = &scene.shadow_draws[index];
                    self.renderer.resolve_blurred_shadow(
                        self.recorder,
                        shadow,
                        z,
                        scale,
                        target_rect.tuple(),
                        &mut self.transients,
                        &mut pass.pending,
                    );
                }
                Event::Effect(index) => {
                    self.run_stages(&mut pass)?;
                    let effect = &scene.effect_layers[index];
                    pass.excluded.push((effect.z_start, effect.z_end));
                    if let Some(composite) = self.resolve_effect_range(&mut pass, effect)? {
                        pass.pending.push(composite);
                    }
                }
                Event::Child(index) => self.child_event(&mut pass, index, z)?,
            }
        }
        self.run_stages(&mut pass)?;
        self.flush_page(&mut pass, usize::MAX)
    }

    /// A child at its z: left to draw in place with the flush that reaches
    /// it, or resolved into a composite now, once the page beneath a child
    /// that reads it back is drawn.
    fn child_event(
        &mut self,
        pass: &mut LayerPass<'_>,
        index: usize,
        z: usize,
    ) -> Result<(), String> {
        let layer = pass.layer;
        let child = &layer.children[index];
        let in_place = match pass.resolved_in_place(index) {
            Some(in_place) => in_place,
            None => pass.can_draw_in_place(child) && self.draws_in_place(pass, index, child)?,
        };
        if in_place {
            pass.in_place.push(index);
            return Ok(());
        }
        if child.reads_backdrop() {
            self.run_stages(pass)?;
            self.flush_page(pass, z + 1)?;
        }
        self.resolve_child(pass, index, child)
    }

    /// Applies the page's pending clear, so a child reading the page through
    /// its base finds it cleared.
    fn start_page(&mut self, pass: &mut LayerPass<'_>) {
        if let Some(load_op) = pass.load_op.take() {
            self.renderer
                .clear_target(self.recorder, pass.page.pass_target().view, load_op);
        }
    }

    /// Draws the next stratum: the ops from the last flush up to `z` outside
    /// the excluded ranges, the deferred ops below `z`, and every pending
    /// composite below `z`, except what still waits behind a blocker.
    fn flush_page(&mut self, pass: &mut LayerPass<'_>, z: usize) -> Result<(), String> {
        let ops = pass.take_ops_below(z);
        ensure_sorted_by_key(&mut pass.pending, composite_z);
        let end = pass
            .pending
            .partition_point(|composite| composite.z_index < z);
        let composites: Vec<ResolvedComposite> = pass.pending.drain(..end).collect();
        let (ops, mut composites) = pass.release(ops, composites);
        let in_place = pass.take_in_place_below(z);
        let mut load_op = pass.load_op.take();
        if ops.is_empty() && composites.is_empty() && in_place.is_empty() {
            if load_op.is_none() {
                pass.drawn_z = pass.drawn_z.max(z);
                return Ok(());
            }
            if z < usize::MAX && pass.beneath.page.is_some() && is_transparent_clear(load_op) {
                pass.load_op = load_op;
                pass.drawn_z = pass.drawn_z.max(z);
                return Ok(());
            }
        }
        let first_in_place_z = in_place
            .first()
            .map_or(usize::MAX, |&index| pass.layer.children[index].z_index);
        let first_run_window = match load_op {
            Some(base) => self.reuse_opaque_prefix(
                pass,
                &ops,
                base,
                &mut composites,
                &mut load_op,
                first_in_place_z,
            )?,
            None => None,
        };
        let flush = Flush {
            ops: &ops,
            composites: &composites,
            in_place: &in_place,
            first_run_window,
        };
        self.encode_flush(pass, flush, load_op.unwrap_or(wgpu::LoadOp::Load))?;
        pass.drawn.extend(composites);
        ensure_sorted_by_key(&mut pass.drawn, composite_z);
        pass.drawn_z = pass.drawn_z.max(z);
        Ok(())
    }

    /// Encodes one flush as one pass: the page's ops and composites, with the
    /// children drawn in place between them at their z.
    fn encode_flush(
        &mut self,
        pass: &mut LayerPass<'_>,
        flush: Flush<'_>,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        let target = pass.page.pass_target();
        let place = InPlaceTarget {
            scale: pass.scale,
            rect: pass.target_rect(),
            size: (target.width, target.height),
            offset: pass.page.offset,
        };
        let (parts, complete) = flush_parts(
            pass.layer,
            flush.ops,
            flush.composites,
            flush.in_place,
            &place,
            self.depth,
        );
        if !complete {
            self.report_nesting_overflow();
        }
        let segments = flush_segments(&pass.layer.scene, &parts, &flush, place.offset, pass.scale);
        let label = LAYER_PASS_LABELS[pass.segments.min(LAYER_PASS_LABELS.len() - 1)];
        pass.segments += 1;
        self.renderer
            .encode_pass(self.recorder, target, &segments, load_op, label)
            .map(drop)
    }

    /// Clears the pass to the flush's first op when it is a solid fill of
    /// the whole page, or else replays that opaque op from the layer cache
    /// or admits it there, when nothing composited or drawn in place lies
    /// beneath it.
    fn reuse_opaque_prefix(
        &mut self,
        pass: &mut LayerPass<'_>,
        ops: &[DrawOp],
        base: wgpu::LoadOp<wgpu::Color>,
        composites: &mut Vec<ResolvedComposite>,
        load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
        first_in_place_z: usize,
    ) -> Result<Option<Range<u32>>, String> {
        if NO_FILL_CACHE.flag() {
            return Ok(None);
        }
        let page_size = (pass.page.texture.width, pass.page.texture.height);
        let context = PrefixContext {
            scene: &pass.layer.scene,
            base,
            page_offset: pass.page.offset,
            page_size,
            scale: pass.scale,
        };
        if let Some(clear) = page_fill_clear(&context, ops, composites, first_in_place_z) {
            *load_op = Some(clear);
            return Ok(Some(1..u32::MAX));
        }
        let Some(prefix) = opaque_prefix(&context, ops) else {
            return Ok(None);
        };
        if drawn_beneath(prefix.z_index, composites, first_in_place_z) {
            return Ok(None);
        }
        let (x, y, width, height) = prefix.device_rect;
        let (pixel_width, pixel_height) = (width as u32, height as u32);
        if let Some(retained) = self.renderer.layer_cache.get(&prefix.key) {
            self.renderer.frame_stats.record_layer_cache_hit(
                &prefix.key,
                pixel_width,
                pixel_height,
            );
            if let Some(gate) = self.renderer.fill_gates.get_mut(&prefix.command) {
                gate.hit(prefix.key);
            }
            composites.push(prefix_blit(&prefix, retained.texture));
            ensure_sorted_by_key(composites, composite_z);
            return Ok(Some(1..u32::MAX));
        }
        self.renderer
            .frame_stats
            .record_layer_cache_miss(&prefix.key, pixel_width, pixel_height);
        let admits = match self.renderer.fill_gates.entry(prefix.command) {
            Entry::Occupied(mut gate) => {
                if let Some(dead) = gate.get_mut().observe(prefix.key) {
                    self.renderer.layer_cache.remove(&dead);
                }
                gate.get().admits()
            }
            Entry::Vacant(slot) => slot.insert(AdmissionGate::copied(prefix.key)).admits(),
        };
        let pixels = u64::from(pixel_width) * u64::from(pixel_height);
        let budget = u64::from(page_size.0) * u64::from(page_size.1);
        if !admits
            || self.prefix_admitted_pixels + pixels > budget
            || !self.renderer.layer_cache.fits(pixel_width, pixel_height)
        {
            return Ok(None);
        }
        let segment = PassSegment {
            scene: &pass.layer.scene,
            ops: &ops[..1],
            composites: &[],
            offset: pass.page.offset,
            scissor: None,
            first_run_window: Some(0..1),
            transform: SegmentTransform::IDENTITY,
            scale: pass.scale,
        };
        self.renderer.encode_pass(
            self.recorder,
            pass.page.pass_target(),
            std::slice::from_ref(&segment),
            base,
            "Layer Pass Prefix",
        )?;
        let texture = Rc::new(
            self.renderer
                .acquire_retained_surface(pixel_width, pixel_height),
        );
        let copy = pass
            .page
            .copy(
                DeviceRect {
                    x,
                    y,
                    width,
                    height,
                },
                &texture,
                [0.0, 0.0],
            )
            .ok_or_else(|| "an opaque prefix off the page's texel grid".to_string())?;
        self.recorder.copy_texture_region(copy);
        self.prefix_admitted_pixels += pixels;
        if self
            .renderer
            .layer_cache
            .insert(prefix.key, Retained::surface(texture), None)
        {
            self.renderer.frame_stats.record_prefix_admission();
            if let Some(gate) = self.renderer.fill_gates.get_mut(&prefix.command) {
                gate.admitted();
            }
        }
        *load_op = Some(wgpu::LoadOp::Load);
        Ok(Some(1..u32::MAX))
    }

    fn run_stages(&mut self, pass: &mut LayerPass<'_>) -> Result<(), String> {
        let mut pending = std::mem::take(&mut pass.stages.pending);
        pending.sort_by_key(|item| (item.stage, item.z));
        let stage_count = pending.last().map_or(0, |item| item.stage + 1);
        self.renderer.frame_stats.record_stages(stage_count as u32);
        let diagnose = stage_diagnostics_enabled();
        let mut start = 0;
        while start < pending.len() {
            let stage = pending[start].stage;
            let end = start + pending[start..].partition_point(|item| item.stage == stage);
            pass.blockers.clear();
            pass.blockers
                .extend(pending[start..].iter().map(|item| Blocker {
                    z: item.z,
                    rect: item.capture_rect,
                }));
            let mut layout =
                self.plan_stage(&pending[start..end], pass.target_rect().pixel_size().0);
            let items = self.take_uncached(pass, &mut pending[start..end], &mut layout);
            if !items.is_empty() {
                if diagnose {
                    log_stage(stage, &items);
                }
                let mut outputs = self.run_stage(pass, &items, &layout)?;
                self.admit_backdrops(&items, &mut outputs);
                pass.pending.extend(outputs);
            }
            start = end;
        }
        pass.blockers.clear();
        Ok(())
    }

    fn take_uncached<'a, 'scene>(
        &mut self,
        pass: &mut LayerPass<'_>,
        items: &'a mut [PendingBackdrop<'scene>],
        layout: &mut StageLayout,
    ) -> Vec<&'a PendingBackdrop<'scene>> {
        let cache_keys_use_pending = backdrop_cache_keys_use_pending(pass);
        if cache_keys_use_pending
            && items
                .iter()
                .any(|item| item.node_id.is_some() && item.batched.is_some())
        {
            ensure_sorted_by_key(&mut pass.pending, composite_z);
        }
        for (index, item) in items.iter_mut().enumerate() {
            item.key = self.backdrop_cache_key(
                pass,
                item,
                layout.signature(index),
                cache_keys_use_pending,
            );
        }
        let mut kept = Vec::new();
        for (index, item) in items.iter().enumerate() {
            match self.cached_backdrop(item) {
                Some(composite) => pass.pending.push(composite),
                None => {
                    let destination = kept.len();
                    if destination == 0 {
                        kept.reserve(items.len() - index);
                    }
                    layout.placements.swap(destination, index);
                    layout.substrates.swap(destination, index);
                    layout.side.swap(destination, index);
                    kept.push(item);
                }
            }
        }
        layout.placements.truncate(kept.len());
        layout.substrates.truncate(kept.len());
        layout.side.truncate(kept.len());
        kept
    }

    fn backdrop_cache_key(
        &self,
        pass: &LayerPass<'_>,
        item: &PendingBackdrop<'_>,
        layout: u64,
        cache_keys_use_pending: bool,
    ) -> Option<LayerRasterCacheKey> {
        let node_id = item.node_id?;
        let effect = item.batched?;
        if !cache_keys_use_pending {
            return None;
        }
        let scale = pass.scale;
        let mut hasher = capture_hasher();
        hash_base(pass.beneath.base, &mut hasher);
        if let Some(segment) = pass.beneath.source.as_ref() {
            hash_base(segment.base, &mut hasher);
            let ops = filtered_ops(&segment.scene.draw_ops, segment.z_end, &[]);
            let window = capture_window(
                item.capture_rect
                    .translated(Point::new(-segment.placement[0], -segment.placement[1])),
            );
            hash_capture_ops(segment.scene, &ops, window, scale, &mut hasher);
            let drawn = &segment.drawn[..segment
                .drawn
                .partition_point(|composite| composite.z_index < segment.z_end)];
            let pending = &segment.pending[..segment
                .pending
                .partition_point(|composite| composite.z_index < segment.z_end)];
            if !hash_capture_composites(drawn, pending, window, &mut hasher) {
                return None;
            }
        }
        let window = capture_window(item.capture_rect);
        let ops = filtered_ops(&pass.layer.scene.draw_ops, item.z, &[]);
        hash_capture_ops(&pass.layer.scene, &ops, window, scale, &mut hasher);
        let pending_end = pass
            .pending
            .partition_point(|composite| composite.z_index < item.z);
        if !hash_capture_composites(
            pass.drawn_below(item.z),
            &pass.pending[..pending_end],
            window,
            &mut hasher,
        ) {
            return None;
        }
        layout.hash(&mut hasher);
        let [x, y, width, height] = if layout == 0
            || effect.shader().is_none()
            || effect.substrates().contains(&SubstrateSpec::Mean)
        {
            item.layer_pixel_rect()
        } else {
            [0.0, 0.0, item.capture_rect.width, item.capture_rect.height]
        };
        Some(LayerRasterCacheKey::backdrop_effect(
            Some(node_id),
            hasher.finish(),
            effect.input_hash(),
            Rect {
                x,
                y,
                width,
                height,
            },
            item.capture_rect.pixel_size(),
            RasterScale::from_scale(scale),
        ))
    }

    fn cached_backdrop(&mut self, item: &PendingBackdrop<'_>) -> Option<ResolvedComposite> {
        let key = item.key?;
        let retained = self.renderer.layer_cache.get(&key)?;
        let RetainedContent::Composite(kind) = &retained.content else {
            return None;
        };
        if let Some(gate) = item
            .node_id
            .and_then(|node_id| self.renderer.backdrop_gates.get_mut(&node_id))
        {
            gate.hit(key);
        }
        let (width, height) = item.capture_rect.pixel_size();
        self.renderer
            .frame_stats
            .record_layer_cache_hit(&key, width, height);
        Some(ResolvedComposite {
            z_index: item.z,
            source: Rc::clone(&retained.texture),
            content: SourceContent::retained(&key),
            dest: item.capture_rect.tuple(),
            scissor: Some(item.support.unwrap_or(item.visible).tuple()),
            kind: replayed_kind(kind, item),
        })
    }

    fn admit_backdrops(
        &mut self,
        items: &[&PendingBackdrop<'_>],
        outputs: &mut [ResolvedComposite],
    ) {
        for item in items {
            let (Some(key), Some(node_id)) = (item.key, item.node_id) else {
                continue;
            };
            let (width, height) = item.capture_rect.pixel_size();
            self.renderer
                .frame_stats
                .record_layer_cache_miss(&key, width, height);
            let gate = match self.renderer.backdrop_gates.entry(node_id) {
                Entry::Occupied(gate) => {
                    let gate = gate.into_mut();
                    if let Some(dead) = gate.observe(key) {
                        self.renderer.layer_cache.remove(&dead);
                    }
                    gate
                }
                Entry::Vacant(slot) => slot.insert(AdmissionGate::pinned(key)),
            };
            if gate.admits() {
                self.admit_backdrop(item, key, node_id, outputs);
            }
        }
    }

    fn admit_backdrop(
        &mut self,
        item: &PendingBackdrop<'_>,
        key: LayerRasterCacheKey,
        node_id: NodeId,
        outputs: &mut [ResolvedComposite],
    ) {
        let Some(output) = outputs
            .iter_mut()
            .find(|composite| composite.z_index == item.z)
        else {
            return;
        };
        let Some(descriptor) = self.transient_descriptor(&output.source) else {
            return;
        };
        let retained = Retained::composite(Rc::clone(&output.source), output.kind.clone());
        if !self
            .renderer
            .layer_cache
            .insert(key, retained, Some(descriptor))
        {
            return;
        }
        self.renderer.frame_stats.record_backdrop_admission();
        if let Some(gate) = self.renderer.backdrop_gates.get_mut(&node_id) {
            gate.admitted();
        }
        output.content = SourceContent::retained(&key);
    }

    fn transient_descriptor(
        &self,
        texture: &Rc<OffscreenTarget>,
    ) -> Option<FrameTextureDescriptor> {
        self.transients
            .iter()
            .find(|(_, transient)| Rc::ptr_eq(transient, texture))
            .map(|(descriptor, _)| *descriptor)
    }

    fn run_stage(
        &mut self,
        pass: &mut LayerPass<'_>,
        items: &[&PendingBackdrop<'_>],
        layout: &StageLayout,
    ) -> Result<Vec<ResolvedComposite>, String> {
        let mut scratch = std::mem::take(&mut self.renderer.stage_side_scratch);
        let result = (|| {
            let scale = pass.scale;
            let placements = &layout.placements;
            let mut singles: Vec<Option<Rc<OffscreenTarget>>> = vec![None; items.len()];
            let stage_end = items.iter().map(|item| item.z).max().unwrap_or(0);
            self.flush_page(pass, stage_end)?;
            for (index, item) in items.iter().enumerate() {
                if placements[index].is_none() {
                    singles[index] =
                        Some(self.capture(pass, item.z, item.capture_rect, "Backdrop Capture")?);
                }
            }
            let mut outputs = Vec::with_capacity(items.len());
            for view in layout.atlas_views() {
                if view.members.is_empty() {
                    continue;
                }
                let (width, height) = view.size();
                let texture = &self.acquire_transient("Backdrop Capture Atlas", width, height);
                let regions: Vec<CaptureRegion> = view
                    .members
                    .iter()
                    .map(|(index, placement)| CaptureRegion {
                        z: items[*index].z,
                        rect: items[*index].capture_rect,
                        origin: [placement.x as f32, placement.y as f32],
                    })
                    .collect();
                self.capture_regions(pass, &regions, texture, "Backdrop Capture Atlas Pass")?;
                let side = self.stage_side_regions(&mut scratch, texture, items, &view, scale)?;
                outputs.extend(stage_composites(
                    texture,
                    side.as_ref(),
                    items,
                    &view.members,
                    self.renderer.ablation.glass,
                ));
            }
            for (index, item) in items.iter().enumerate() {
                if let Some(capture) = singles[index].take() {
                    outputs.push(self.resolve_captured_backdrop(item, capture, scale)?);
                }
            }
            Ok(outputs)
        })();
        self.renderer.stage_side_scratch = scratch;
        result
    }

    fn resolve_child_backdrop(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
        backdrop: &RenderEffect,
        placement: ChildPlacement,
    ) -> Result<Option<ResolvedComposite>, String> {
        let scale = pass.scale;
        let ChildPlacement {
            z,
            visible,
            support,
            dest,
            snap,
        } = placement;
        let padding = ((backdrop.input_padding() + backdrop.output_padding()) * scale).ceil();
        let expanded = visible.expand(padding);
        let capture_rect = expanded
            .intersect(pass.capture_bounds().unwrap_or(expanded))
            .unwrap_or(visible)
            .snap_out();
        let mut item = PendingBackdrop {
            z,
            alpha: child.backdrop_alpha,
            node_id: child.node_id,
            key: None,
            capture_rect,
            layer_rect: dest,
            visible,
            effect: backdrop,
            rounded_mask: grid_rounded_mask(child, snap, scale),
            batched: batched_effect(backdrop),
            stage: 0,
            support: Some(support),
        };
        if !self.backdrop_draws_now(&item) {
            return Ok(self.backdrop_placeholder(&item, scale));
        }
        let cache_keys_use_pending = backdrop_cache_keys_use_pending(pass);
        if cache_keys_use_pending && item.node_id.is_some() && item.batched.is_some() {
            ensure_sorted_by_key(&mut pass.pending, composite_z);
        }
        if item
            .batched
            .is_some_and(|effect| !effect.substrates().is_empty())
        {
            let layout = self.plan_stage(
                std::slice::from_ref(&item),
                pass.target_rect().pixel_size().0,
            );
            if layout.placements[0].is_some() {
                item.key = self.backdrop_cache_key(
                    pass,
                    &item,
                    layout.signature(0),
                    cache_keys_use_pending,
                );
                if let Some(cached) = self.cached_backdrop(&item) {
                    return Ok(Some(cached));
                }
                let items = [&item];
                let mut outputs = self.run_stage(pass, &items, &layout)?;
                self.admit_backdrops(&items, &mut outputs);
                return outputs
                    .pop()
                    .map(Some)
                    .ok_or_else(|| "a child backdrop substrate produced no composite".into());
            }
        }
        item.key = self.backdrop_cache_key(pass, &item, 0, cache_keys_use_pending);
        if let Some(cached) = self.cached_backdrop(&item) {
            return Ok(Some(cached));
        }
        let capture = self.capture(pass, z, capture_rect, "Child Backdrop Capture")?;
        let mut output = self.resolve_captured_backdrop(&item, capture, scale)?;
        self.admit_backdrops(&[&item], std::slice::from_mut(&mut output));
        Ok(Some(output))
    }

    /// Places every batched member of a stage into the atlas, in item order.
    ///
    /// The order is load-bearing, not incidental: `StageLayout::signature`
    /// hashes a member's atlas `(x, y)` into its backdrop cache key, so a
    /// member that shifts inside the atlas re-renders. Placing a member whose
    /// capture resizes every frame -- an animating one -- ahead of a still
    /// member walks the still member's slot and costs it its cache entry every
    /// frame. Sorting by height packs tighter and loses exactly that.
    ///
    /// Shelves end at `shelf_width`, the page's width, so the atlas grows down
    /// instead of out to the dimension limit: the capture fixup and blur
    /// passes load the whole atlas, and a 4096-wide one costs them several
    /// times what its captures cover.
    fn pack_stage(
        &self,
        items: &[PendingBackdrop<'_>],
        shelf_width: u32,
    ) -> (
        AtlasPacker,
        Vec<Option<AtlasPlacement>>,
        Vec<PlannedSubstrates>,
    ) {
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let mut packer = AtlasPacker::new(limit).with_shelf_width(shelf_width);
        let mut placements: Vec<Option<AtlasPlacement>> = vec![None; items.len()];
        for (index, item) in items.iter().enumerate() {
            if item.batched.is_none() {
                continue;
            }
            let (width, height) = item.capture_rect.pixel_size();
            placements[index] = packer.place(width, height);
        }
        let mut substrates = vec![PlannedSubstrates::new(); items.len()];
        for (index, item) in items.iter().enumerate() {
            let Some(placement) = placements[index] else {
                continue;
            };
            let batched = item.batched.expect("a placed item is batched");
            let in_atlas = batched.blur().is_none();
            for spec in batched.substrates() {
                let size = substrate_size(*spec, item.capture_rect.pixel_size());
                let atlas_slot = if in_atlas {
                    let Some(slot) = packer
                        .place(size.0, size.1)
                        .filter(|slot| slot.atlas == placement.atlas)
                    else {
                        break;
                    };
                    Some((slot.x, slot.y, size.0, size.1))
                } else {
                    None
                };
                substrates[index].push(PlannedSubstrate {
                    spec: *spec,
                    size,
                    work_size: match spec {
                        SubstrateSpec::Mean => (1, mean_capture_rect(item).pixel_size().1),
                        _ => size,
                    },
                    atlas_slot,
                });
            }
        }
        (packer, placements, substrates)
    }

    fn plan_stage(&self, items: &[PendingBackdrop<'_>], shelf_width: u32) -> StageLayout {
        let (packer, placements, substrates) = self.pack_stage(items, shelf_width);
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let atlas_sizes: Vec<(u32, u32)> = packer
            .atlases
            .iter()
            .map(|atlas| atlas.padded_size(limit))
            .collect();
        let mut side_sizes = vec![(0, 0); atlas_sizes.len()];
        let mut side: Vec<SideSlots> = vec![SideSlots::default(); items.len()];
        for (atlas_index, side_size) in side_sizes.iter_mut().enumerate() {
            let mut requests = Vec::new();
            for (index, slots) in side.iter_mut().enumerate() {
                if !placements[index].is_some_and(|placement| placement.atlas == atlas_index) {
                    continue;
                }
                if let Some(blur) = items[index].batched.and_then(BatchedEffect::blur) {
                    let (width, height) = items[index].capture_rect.pixel_size();
                    let size = blur_scratch_size(blur.radius_x, blur.radius_y, width, height);
                    requests.push((size, &mut slots.blur));
                }
                slots.substrates.resize(substrates[index].len(), None);
                for (planned, slot) in substrates[index].iter().zip(&mut slots.substrates) {
                    requests.push((planned.work_size, slot));
                }
            }
            requests.sort_unstable_by_key(|((width, height), _)| {
                (std::cmp::Reverse(*height), std::cmp::Reverse(*width))
            });
            let mut side_packer = AtlasPacker::new(limit);
            for ((width, height), slot) in requests {
                *slot = side_packer
                    .place(width, height)
                    .filter(|slot| slot.atlas == 0)
                    .map(|slot| (slot.x, slot.y, width, height));
            }
            *side_size = side_packer
                .atlases
                .first()
                .map_or((0, 0), |atlas| atlas.padded_size(limit));
        }
        StageLayout {
            atlas_sizes,
            placements,
            substrates,
            side_sizes,
            side,
        }
    }

    fn stage_side_regions<'s>(
        &mut self,
        side_scratch: &'s mut StageSideScratch,
        atlas: &Rc<OffscreenTarget>,
        items: &[&PendingBackdrop<'_>],
        view: &AtlasView<'_>,
        scale: f32,
    ) -> Result<Option<StageSideRegions<'s>>, String> {
        side_scratch.clear();
        let members = &view.members;
        if !self.renderer.ablation.blur {
            side_scratch.blurred.extend(
                members
                    .iter()
                    .enumerate()
                    .filter_map(|(member, (index, _))| {
                        Some((member, items[*index].batched?.blur()?))
                    }),
            );
        }
        if side_scratch.blurred.is_empty()
            && members
                .iter()
                .all(|(index, _)| view.substrates(*index).is_empty())
        {
            return Ok(None);
        }
        let mut sinks = SideRegionSinks {
            regions: &mut side_scratch.regions,
            region_slots: &mut side_scratch.region_slots,
            averaged: &mut side_scratch.averaged,
            average_slots: &mut side_scratch.average_slots,
        };
        stage_blur_regions(
            &side_scratch.blurred,
            members,
            items,
            view,
            scale,
            &mut sinks,
            &mut side_scratch.slots,
        )?;
        stage_substrate_regions(
            members,
            items,
            view,
            scale,
            self.renderer.ablation.substrates,
            sinks,
            &mut side_scratch.member_regions,
        )?;
        if side_scratch.regions.is_empty() && side_scratch.averaged.is_empty() {
            return Ok(Some(StageSideRegions {
                result: Rc::clone(atlas),
                blurred: &side_scratch.slots,
                substrates: &side_scratch.member_regions,
            }));
        }
        let (width, height) = view.side_size();
        let direct = direct_side_slots(
            &mut side_scratch.regions,
            &side_scratch.region_slots,
            &mut side_scratch.averaged,
            &side_scratch.average_slots,
        );
        let blur_scratch = self.acquire_transient("Backdrop Blur Scratch", width, height);
        let result = self.acquire_transient("Backdrop Blur Result", width, height);
        let device = self.renderer.device.clone();
        self.renderer.effect_renderer.record_substrates(
            members
                .iter()
                .map(|(index, _)| view.substrates(*index).len() as u32)
                .sum(),
        );
        self.renderer.effect_renderer.encode_blur_atlas_passes(
            self.recorder,
            &device,
            atlas,
            &blur_scratch,
            &result,
            AtlasSideWork {
                blurs: &side_scratch.regions,
                averages: &side_scratch.averaged,
                blur_output: direct.then_some(atlas),
            },
        );
        if !direct {
            record_side_result_copies(
                self.recorder,
                &result,
                atlas,
                &side_scratch.regions,
                &side_scratch.region_slots,
                &side_scratch.averaged,
                &side_scratch.average_slots,
            );
        }
        Ok(Some(StageSideRegions {
            result,
            blurred: &side_scratch.slots,
            substrates: &side_scratch.member_regions,
        }))
    }

    /// Resolves one backdrop effect from its own capture texture: a shader
    /// tail draws in the final pass, anything else is applied into a texture
    /// and blitted with the effect's mask.
    fn resolve_captured_backdrop(
        &mut self,
        item: &PendingBackdrop<'_>,
        capture: Rc<OffscreenTarget>,
        scale: f32,
    ) -> Result<ResolvedComposite, String> {
        let layer_pixel_rect = item.layer_pixel_rect();
        let scissor = item.support.unwrap_or(item.visible);
        if let Some((pre_shader, shader)) = shader_tail(item.effect)
            && item.composites_tail(shader)
        {
            let source = match pre_shader {
                Some(effect) => {
                    let reads = effect_reads(
                        effect,
                        domain_read_rect(item.effect, item.layer_rect, item.capture_rect, scale),
                        item.layer_rect,
                        item.capture_rect,
                        scale,
                    );
                    self.apply_effect(&capture, effect, layer_pixel_rect, reads, "Backdrop Effect")?
                }
                None => capture,
            };
            return Ok(ResolvedComposite {
                z_index: item.z,
                source,
                content: SourceContent::Transient,
                dest: item.capture_rect.tuple(),
                scissor: Some(item.support.unwrap_or(item.visible).tuple()),
                kind: ResolvedCompositeKind::Shader {
                    shader: Arc::clone(shader),
                    layer_pixel_rect,
                    source_region: None,
                    source_logical_size: None,
                    substrate_regions: [None; MAX_SUBSTRATES],
                    rounded_mask: item.rounded_mask,
                    alpha: item.alpha,
                },
            });
        }
        let reads = effect_reads(
            item.effect,
            blit_read_rect(scissor, item.capture_rect, false),
            item.layer_rect,
            item.capture_rect,
            scale,
        );
        let result = self.apply_effect(
            &capture,
            item.effect,
            layer_pixel_rect,
            reads,
            "Backdrop Effect",
        )?;
        Ok(backdrop_blit(
            item,
            CompositeSource {
                texture: result,
                content: SourceContent::Transient,
            },
        ))
    }

    /// Reads what a backdrop at `z` in the layer sees within `rect` into a
    /// texture that size.
    fn capture(
        &mut self,
        pass: &mut LayerPass<'_>,
        z: usize,
        rect: DeviceRect,
        label: &'static str,
    ) -> Result<Rc<OffscreenTarget>, String> {
        let (width, height) = rect.pixel_size();
        let texture = self.acquire_transient(label, width, height);
        let region = CaptureRegion {
            z,
            rect,
            origin: [0.0, 0.0],
        };
        self.capture_regions(
            pass,
            std::slice::from_ref(&region),
            &texture,
            "Backdrop Capture Pass",
        )?;
        Ok(texture)
    }

    /// Reads what every region's backdrop sees into its place in `texture`,
    /// in one pass scissored to each region's texels: the parent's page under
    /// the layer, the layer's own page, and what is below the region's z and
    /// not on the page yet (the ops since the last flush, the deferred ops and
    /// the pending composites). A lone region that needs nothing but the
    /// layer's own page is copied texel for texel instead. A tiled GPU runs
    /// each copy as a job of its own, so several regions share the pass.
    fn capture_regions(
        &mut self,
        pass: &mut LayerPass<'_>,
        regions: &[CaptureRegion],
        texture: &Rc<OffscreenTarget>,
        label: &'static str,
    ) -> Result<(), String> {
        let scale = pass.scale;
        ensure_sorted_by_key(&mut pass.pending, composite_z);
        let fixups: Vec<Cow<'_, [DrawOp]>> = regions
            .iter()
            .map(|region| pass.ops_below(region.z))
            .collect();
        let target = PassTarget {
            view: &texture.view,
            width: texture.width,
            height: texture.height,
        };
        let own_segments: Vec<Option<PassSegment<'_>>> = regions
            .iter()
            .zip(&fixups)
            .map(|(region, fixup)| {
                let offset = region_offset(region);
                let own_end = pass
                    .pending
                    .partition_point(|composite| composite.z_index < region.z);
                let own = region.rect.intersect(pass.target_rect())?;
                Some(PassSegment {
                    scene: &pass.layer.scene,
                    ops: fixup,
                    composites: &pass.pending[..own_end],
                    offset,
                    scissor: Some((
                        (own.x - offset[0]) as u32,
                        (own.y - offset[1]) as u32,
                        own.width as u32,
                        own.height as u32,
                    )),
                    first_run_window: None,
                    transform: SegmentTransform::IDENTITY,
                    scale,
                })
            })
            .collect();
        let fixes_up = own_segments
            .iter()
            .flatten()
            .any(|segment| segment_draws_anything(target, segment));
        if let ([region], false) = (regions, fixes_up)
            && self.copy_region(pass, region, texture)
        {
            return Ok(());
        }
        if fixes_up {
            self.renderer.frame_stats.record_capture_fixup_pass();
        }
        let page_untouched = pass.page_untouched();
        let bases: Vec<Vec<ResolvedComposite>> = regions
            .iter()
            .map(|region| {
                pass.beneath
                    .page
                    .as_ref()
                    .and_then(|base| base.under(region.rect))
                    .into_iter()
                    .chain(pass.page.blit(region.rect).filter(|_| !page_untouched))
                    .collect()
            })
            .collect();
        let mut segments: Vec<PassSegment<'_>> = Vec::with_capacity(regions.len() * 2);
        for ((region, base), own) in regions.iter().zip(&bases).zip(own_segments) {
            let (region_width, region_height) = region.rect.pixel_size();
            segments.push(PassSegment {
                scene: &self.empty_scene,
                ops: &[],
                composites: base,
                offset: region_offset(region),
                scissor: Some((
                    region.origin[0] as u32,
                    region.origin[1] as u32,
                    region_width,
                    region_height,
                )),
                first_run_window: None,
                transform: SegmentTransform::IDENTITY,
                scale,
            });
            segments.extend(own);
        }
        self.renderer.encode_pass(
            self.recorder,
            target,
            &segments,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            label,
        )?;
        Ok(())
    }

    /// Copies `region` of the layer's own page into its place in `texture`
    /// and reports whether it did: only when the layer reads no parent page,
    /// the two formats are copy-compatible and the region is a whole-texel
    /// rect inside the page.
    fn copy_region(
        &mut self,
        pass: &LayerPass<'_>,
        region: &CaptureRegion,
        texture: &OffscreenTarget,
    ) -> bool {
        if pass.beneath.page.is_some() || !copy_compatible(&pass.page.texture, texture) {
            return false;
        }
        let Some(copy) = pass.page.copy(region.rect, texture, region.origin) else {
            return false;
        };
        self.recorder.copy_texture_region(copy);
        true
    }

    fn resolve_effect_range(
        &mut self,
        pass: &mut LayerPass<'_>,
        effect: &EffectLayer,
    ) -> Result<Option<ResolvedComposite>, String> {
        let scale = pass.scale;
        let scene = &pass.layer.scene;
        let snap = effect
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, scale))
            .unwrap_or_default();
        let rect = effect.rect.translate(snap.x, snap.y);
        let visible = match effect.clip {
            Some(clip) => rect.intersect(clip.translate(snap.x, snap.y)),
            None => Some(rect),
        };
        let Some(visible) = visible else {
            return Ok(None);
        };
        let padding = effect.effect.as_ref().map_or(0.0, |effect| {
            effect.input_padding() + effect.output_padding()
        }) * scale;
        let target_rect = pass.target_rect();
        let Some(source_rect) = DeviceRect::from_logical(rect, scale)
            .expand(padding.ceil())
            .intersect(target_rect.expand(padding.ceil()))
        else {
            return Ok(None);
        };
        let source_rect = source_rect.snap_out();
        let (width, height) = source_rect.pixel_size();
        let texture = self.acquire_transient("Effect Range Source", width, height);
        let ops = filtered_ops_in_range(&scene.draw_ops, effect.z_start, effect.z_end, &[]);
        let below = pass.pending_below(effect.z_end);
        let own_start = below.partition_point(|composite| composite.z_index < effect.z_start);
        let segment = PassSegment {
            scene,
            ops: &ops,
            composites: &below[own_start..],
            offset: [source_rect.x, source_rect.y],
            scissor: None,
            first_run_window: None,
            transform: SegmentTransform::IDENTITY,
            scale,
        };
        let target = PassTarget {
            view: &texture.view,
            width,
            height,
        };
        self.renderer.encode_pass(
            self.recorder,
            target,
            std::slice::from_ref(&segment),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "Effect Range Pass",
        )?;
        let layer_rect_device = DeviceRect::from_logical(rect, scale);
        let layer_pixel_rect = [
            layer_rect_device.x - source_rect.x,
            layer_rect_device.y - source_rect.y,
            layer_rect_device.width,
            layer_rect_device.height,
        ];
        let waiting = effect.effect.as_ref().filter(|render_effect| {
            !self.effect_draws_now(render_effect, layer_pixel_rect, (width, height), false)
        });
        let result = match &effect.effect {
            Some(render_effect) if waiting.is_none() => self.apply_effect(
                &texture,
                render_effect,
                layer_pixel_rect,
                EffectReads::default(),
                "Effect Range Result",
            )?,
            _ => texture,
        };
        let mut composite = ResolvedComposite {
            z_index: effect.z_start,
            source: result,
            content: SourceContent::Transient,
            dest: source_rect.tuple(),
            scissor: Some(DeviceRect::from_logical(visible, scale).tuple()),
            kind: ResolvedCompositeKind::Blit {
                alpha: effect.composite_alpha,
                blend_mode: effect.blend_mode,
                rounded_mask: None,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: None,
            },
        };
        clip_to_placeholder(&mut composite, waiting, layer_rect_device, scale);
        Ok(Some(composite))
    }

    /// Runs an effect chain over `source` into a fresh texture of the same
    /// size.
    fn apply_effect(
        &mut self,
        source: &Rc<OffscreenTarget>,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
        reads: EffectReads,
        label: &'static str,
    ) -> Result<Rc<OffscreenTarget>, String> {
        let dest = self.acquire_transient(label, source.width, source.height);
        self.apply_effect_into(source, &dest, effect, layer_pixel_rect, reads)?;
        Ok(dest)
    }

    /// Runs an effect chain over `source` into `dest`, a texture of the same
    /// size.
    fn apply_effect_into(
        &mut self,
        source: &Rc<OffscreenTarget>,
        dest: &OffscreenTarget,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
        reads: EffectReads,
    ) -> Result<(), String> {
        let device = self.renderer.device.clone();
        let format = self.renderer.composition_format;
        let scratch = self
            .renderer
            .effect_renderer
            .acquire_recorded_effect_scratch_targets(
                self.recorder,
                &device,
                effect,
                source.width,
                source.height,
                format,
            );
        let encoded = {
            let mut refs = scratch.refs();
            let passes = self.renderer.effect_renderer.encode_effect(
                self.recorder,
                &device,
                source,
                &dest.view,
                effect,
                layer_pixel_rect,
                reads,
                &mut refs,
            );
            passes.and_then(|passes| refs.assert_consumed().map(|()| passes))
        };
        scratch.release_into(self.recorder);
        let passes = encoded?;
        self.recorder.record_passes(passes);
        Ok(())
    }

    fn resolve_child(
        &mut self,
        pass: &mut LayerPass<'_>,
        index: usize,
        child: &ChildLayer,
    ) -> Result<(), String> {
        let scale = pass.scale;
        let z = child.z_index;
        let ChildFrame {
            snap,
            grid,
            translation,
            dest,
            visible: visible_device,
        } = ChildFrame::of(child, scale, pass.target_rect());
        let layer_rect = dest;

        if !self.renderer.ablation.stages
            && child.backdrop_alpha != 0.0
            && let Some(backdrop) = &child.backdrop
            && let Some(visible) = visible_device
            && let Some(support) =
                child_composite_support(child, backdrop.output_support(), snap, scale, visible)
        {
            let placement = ChildPlacement {
                z,
                visible,
                support,
                dest,
                snap,
            };
            if let Some(composite) =
                self.resolve_child_backdrop(pass, child, backdrop, placement)?
            {
                pass.pending.push(composite);
            }
        }

        let Some(visible) = visible_device else {
            return Ok(());
        };
        if composites_nothing(child) {
            return Ok(());
        }
        if let Some(composite) = self.shader_only_child(child, z, scale, visible, translation, snap)
        {
            pass.pending.extend(composite);
            return Ok(());
        }
        let shown = child_surface_bound(child, snap, scale, pass.target_rect()).unwrap_or(visible);
        let resolved = match pass.resolved.get_mut(index).and_then(Option::take) {
            Some(Resolved::Surface(resolved)) => resolved,
            Some(Resolved::InPlace) | None => {
                self.render_child_surface(pass, child, z, grid, shown)?
            }
        };
        let Some(surface) = resolved else {
            return Ok(());
        };
        let (source, waiting) =
            match self.surface_through_effect(child, &surface, snap, z, scale, shown)? {
                ThroughEffect::Tail(composite) => {
                    pass.pending.push(composite);
                    return Ok(());
                }
                ThroughEffect::Source { source, waiting } => (source, waiting),
            };
        // A layer that is all its waiting effect shows the placeholder alone.
        if let Some(effect) = waiting.filter(|_| draws_nothing(&child.content)) {
            pass.pending.extend(
                self.empty_layer_placeholder(child, effect, layer_rect, visible, snap, scale),
            );
            return Ok(());
        }
        let composite = match surface.grid_dest {
            Some(dest) => {
                let visible = dest.intersect(shown).unwrap_or(visible);
                let mut composite = grid_child_composite(
                    child,
                    z,
                    source,
                    surface.region,
                    dest,
                    snap,
                    scale,
                    visible,
                );
                clip_to_placeholder(&mut composite, waiting, layer_rect, scale);
                composite
            }
            None => {
                let Some(composite) =
                    projected_child_composite(child, z, source, &surface, snap, scale, shown)
                else {
                    return Ok(());
                };
                composite
            }
        };
        pass.pending.push(composite);
        Ok(())
    }

    /// The child's surface through its effect: a shader drawn in the final
    /// pass, or the surface the effect drew into, or, while the effect
    /// compiles, the surface as it is and the effect it waits for.
    fn surface_through_effect<'l>(
        &mut self,
        child: &'l ChildLayer,
        surface: &SurfaceRender,
        snap: Point,
        z: usize,
        scale: f32,
        shown: DeviceRect,
    ) -> Result<ThroughEffect<'l>, String> {
        let Some(effect) = &child.effect else {
            return Ok(ThroughEffect::plain(surface));
        };
        let drawn = match self.shader_tail_over_surface(child, surface, snap, z, scale, shown) {
            Some(Some(composite)) => return Ok(ThroughEffect::Tail(composite)),
            Some(None) => None,
            None => self.effect_over_surface(child, surface, effect)?,
        };
        Ok(match drawn {
            Some(source) => ThroughEffect::Source {
                source,
                waiting: None,
            },
            None => ThroughEffect::Source {
                source: surface.source.clone(),
                waiting: Some(effect),
            },
        })
    }

    /// The child's effect applied over its surface. Over a retained surface
    /// the output is a pure function of the surface's content and the
    /// effect, so it lives in the layer cache once the same output was
    /// wanted two frames running: an animated effect over still content is
    /// drawn afresh, and content that changes carries its effect with it.
    /// None while the effect's pipelines compile.
    fn effect_over_surface(
        &mut self,
        child: &ChildLayer,
        surface: &SurfaceRender,
        effect: &RenderEffect,
    ) -> Result<Option<CompositeSource>, String> {
        let layer_pixel_rect = layer_pixel_rect(child, surface.rect, surface.scale);
        let source = &surface.source.texture;
        let (width, height) = (source.width, source.height);
        if !self.effect_draws_now(effect, layer_pixel_rect, (width, height), false) {
            return Ok(None);
        }
        let content = surface.source.content.derived(&effect.render_hash());
        let retained = surface.source.content.retained_hash().zip(child.node_id);
        if let Some((input, node_id)) = retained {
            let [x, y, w, h] = layer_pixel_rect;
            let key = LayerRasterCacheKey::layer_effect(
                Some(node_id),
                input,
                effect.render_hash(),
                Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
                (width, height),
                RasterScale::from_scale(surface.scale),
            );
            if let Some(cached) = self.renderer.layer_cache.get(&key) {
                self.renderer
                    .frame_stats
                    .record_layer_cache_hit(&key, width, height);
                if let Some(gate) = self.renderer.effect_gates.get_mut(&node_id) {
                    gate.hit(key);
                }
                return Ok(Some(CompositeSource {
                    texture: cached.texture,
                    content,
                }));
            }
            self.renderer
                .frame_stats
                .record_layer_cache_miss(&key, width, height);
            let admits = match self.renderer.effect_gates.entry(node_id) {
                Entry::Occupied(mut gate) => {
                    if let Some(dead) = gate.get_mut().observe(key) {
                        self.renderer.layer_cache.remove(&dead);
                    }
                    gate.get().admits()
                }
                Entry::Vacant(slot) => slot.insert(AdmissionGate::copied(key)).admits(),
            };
            if admits && self.renderer.layer_cache.fits(width, height) {
                let dest = Rc::new(self.renderer.acquire_retained_surface(width, height));
                self.apply_effect_into(
                    source,
                    &dest,
                    effect,
                    layer_pixel_rect,
                    EffectReads::default(),
                )?;
                if self
                    .renderer
                    .layer_cache
                    .insert(key, Retained::surface(Rc::clone(&dest)), None)
                    && let Some(gate) = self.renderer.effect_gates.get_mut(&node_id)
                {
                    gate.admitted();
                }
                return Ok(Some(CompositeSource {
                    texture: dest,
                    content,
                }));
            }
        }
        let texture = self.apply_effect(
            source,
            effect,
            layer_pixel_rect,
            EffectReads::default(),
            "Layer Effect",
        )?;
        Ok(Some(CompositeSource { texture, content }))
    }

    fn shader_tail_composites(&mut self, child: &ChildLayer, shader: &RuntimeShader) -> bool {
        let plain = child.alpha >= 1.0 && child.rounded_clip.is_none();
        shader.substrates().is_empty()
            && child.blend_mode == BlendMode::SrcOver
            && (plain || shader.batched_source())
            && self
                .renderer
                .effect_renderer
                .shader_cache
                .position_independent(shader)
    }

    /// The child's shader drawn in the final pass over its surface: `None`
    /// when it does not draw that way, no composite while its pipeline
    /// compiles.
    fn shader_tail_over_surface(
        &mut self,
        child: &ChildLayer,
        surface: &SurfaceRender,
        snap: Point,
        z: usize,
        scale: f32,
        visible: DeviceRect,
    ) -> Option<Option<ResolvedComposite>> {
        let effect = child.effect.as_ref()?;
        let RenderEffect::Shader { shader } = effect else {
            return None;
        };
        let dest = surface.grid_dest?;
        let visible = dest.intersect(visible)?;
        if !self.shader_tail_composites(child, shader) {
            return None;
        }
        let layer_pixel_rect = layer_pixel_rect(child, surface.rect, surface.scale);
        let source_size = (surface.source.texture.width, surface.source.texture.height);
        if !self.effect_draws_now(effect, layer_pixel_rect, source_size, true) {
            return Some(None);
        }
        Some(Some(shader_tail_composite(
            child,
            shader,
            z,
            surface.source.clone(),
            dest,
            layer_pixel_rect,
            grid_rounded_mask(child, snap, scale),
            visible,
        )))
    }

    /// A child that draws only its shader, composited without a surface of
    /// its own: `None` when the child is not one, and no composite while its
    /// shader compiles without a placeholder.
    fn shader_only_child(
        &mut self,
        child: &ChildLayer,
        z: usize,
        scale: f32,
        visible: DeviceRect,
        translation: Option<Point>,
        snap: Point,
    ) -> Option<Option<ResolvedComposite>> {
        let translation = translation?;
        let effect = child.effect.as_ref()?;
        let RenderEffect::Shader { shader } = effect else {
            return None;
        };
        let support =
            child_composite_support(child, shader.output_support(), snap, scale, visible)?;
        if !draws_nothing(&child.content) || !self.shader_tail_composites(child, shader) {
            return None;
        }
        let surface_logical = child_surface_rect(child, scale)?;
        let surface_rect = DeviceRect::from_logical(surface_logical, scale).snap_out();
        let (width, height) = surface_rect.pixel_size();
        if u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS {
            return None;
        }
        let dest = DeviceRect {
            x: (surface_rect.x + translation.x * scale).round(),
            y: (surface_rect.y + translation.y * scale).round(),
            width: surface_rect.width,
            height: surface_rect.height,
        };
        let layer_pixel_rect = layer_pixel_rect(child, surface_rect, scale);
        let rounded_mask = grid_rounded_mask(child, snap, scale);
        if !self.effect_draws_now(effect, layer_pixel_rect, (width, height), true) {
            let [x, y, width, height] = layer_pixel_rect;
            let layer = DeviceRect {
                x: dest.x + x,
                y: dest.y + y,
                width,
                height,
            };
            return Some(shader.placeholder().map(|placeholder| {
                let mask = placeholder_mask(placeholder, layer, scale).or(rounded_mask);
                self.placeholder_fill(placeholder.color, z, layer, support, child.alpha, mask)
            }));
        }
        let source = self
            .renderer
            .transparent_source(self.recorder, width, height);
        Some(Some(shader_tail_composite(
            child,
            shader,
            z,
            CompositeSource {
                texture: source,
                content: SourceContent::retained(&TRANSPARENT_SOURCE),
            },
            dest,
            layer_pixel_rect,
            rounded_mask,
            support,
        )))
    }

    /// Renders the child's content into its own texture. A child that reads
    /// its backdrop is never cached and renders every frame, so when it sits
    /// on the parent's pixel grid (a translation, or a uniform scale it
    /// renders at) and carries no effect of its own it renders the part of
    /// its surface the page shows (`shown`, its clip within the target),
    /// grown by what its glasses read past it (`backdrop_reach`): a card
    /// wider than the screen costs the screen, and every capture inside it
    /// follows.
    fn render_child_surface(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
        z: usize,
        grid: Option<Point>,
        shown: DeviceRect,
    ) -> Result<Option<SurfaceRender>, String> {
        let Some(plan) = SurfacePlan::of(child, pass.scale, grid, shown) else {
            return Ok(None);
        };
        let retain = match self.source_decision(child, &plan, AdmissionGate::rendered) {
            SourceDecision::Cached(surface) => return Ok(Some(surface)),
            SourceDecision::Render(retain) => retain,
        };
        let beneath = if child.content.contains_backdrop() {
            self.start_page(pass);
            beneath_for_child(pass, child, z, plan.grid_offset.filter(|_| plan.translated))?
        } else {
            Beneath::over(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT))
        };
        self.render_planned_surface(child, &plan, retain, &beneath)
            .map(Some)
    }

    /// Whether the child's surface comes from the cache or is rendered, and
    /// whether a rendered one is kept; `gate` makes a new node's source gate.
    fn source_decision(
        &mut self,
        child: &ChildLayer,
        plan: &SurfacePlan,
        gate: fn(LayerRasterCacheKey) -> AdmissionGate,
    ) -> SourceDecision {
        let key = plan.cache_key(child);
        if let Some(key) = key
            && let Some(texture) = self.cached_source(child.node_id, key, plan.width, plan.height)
        {
            return SourceDecision::Cached(plan.surface(
                CompositeSource {
                    texture,
                    content: SourceContent::retained(&key),
                },
                None,
            ));
        }
        SourceDecision::Render(
            key.filter(|key| {
                self.admits_source(child.node_id, *key, (plan.width, plan.height), gate)
            }),
        )
    }

    fn render_planned_surface(
        &mut self,
        child: &ChildLayer,
        plan: &SurfacePlan,
        retain: Option<LayerRasterCacheKey>,
        beneath: &Beneath<'_>,
    ) -> Result<SurfaceRender, String> {
        let texture = if retain.is_some() {
            Rc::new(
                self.renderer
                    .acquire_retained_surface(plan.width, plan.height),
            )
        } else {
            self.acquire_transient("Layer Surface", plan.width, plan.height)
        };
        let child_page = Page {
            texture: Rc::clone(&texture),
            offset: [plan.surface_rect.x, plan.surface_rect.y],
        };
        self.renderer.frame_stats.record_isolated_layer_render(
            plan.width,
            plan.height,
            child.node_id,
            plan.surface_logical,
        );
        self.render_layer(
            &child.content,
            child_page,
            plan.surface_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            beneath,
        )?;
        let retained = retain.filter(|key| self.retain_source(child.node_id, *key, &texture));
        Ok(plan.surface(
            CompositeSource {
                texture,
                content: retained.map_or(SourceContent::Transient, |key| {
                    SourceContent::retained(&key)
                }),
            },
            None,
        ))
    }

    /// Whether a child that can draw in place does this frame: unless its
    /// surface is cached or retained this frame, which it then resolves into
    /// the pass for `resolve_child`. Content that holds still costs less to
    /// composite from its cached surface than to draw again, and content that
    /// changes every frame, whose surface the cache stops keeping, would be
    /// drawn afresh into a surface it throws away.
    fn draws_in_place(
        &mut self,
        pass: &mut LayerPass<'_>,
        index: usize,
        child: &ChildLayer,
    ) -> Result<bool, String> {
        let scale = pass.scale;
        let target = pass.target_rect();
        let frame = ChildFrame::of(child, scale, target);
        let Some(visible) = frame.visible else {
            return Ok(true);
        };
        let shown = child_surface_bound(child, frame.snap, scale, target).unwrap_or(visible);
        let Some(plan) = SurfacePlan::of(child, scale, frame.grid, shown) else {
            return Ok(true);
        };
        let surface = match self.source_decision(child, &plan, AdmissionGate::drawn_in_place) {
            SourceDecision::Cached(surface) => surface,
            SourceDecision::Render(None) => return Ok(true),
            SourceDecision::Render(retain) => {
                let cleared = Beneath::over(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                self.render_planned_surface(child, &plan, retain, &cleared)?
            }
        };
        pass.resolve(index, Resolved::Surface(Some(surface)));
        Ok(false)
    }

    /// Resolves every flat child ahead of its z: from the cache, drawn in
    /// place, or rendered with the others into atlases, so a frame that
    /// renders many flat surfaces -- a grid laid out again, or one whose cells
    /// just held still long enough to be kept -- renders them in a few passes.
    fn resolve_flat_children(&mut self, pass: &mut LayerPass<'_>) -> Result<(), String> {
        let layer = pass.layer;
        if !layer.children.iter().any(renders_flat) {
            return Ok(());
        }
        let scale = pass.scale;
        let target = pass.target_rect();
        let mut resolved: Vec<Option<Resolved>> = std::iter::repeat_with(|| None)
            .take(layer.children.len())
            .collect();
        let mut batch: Vec<BatchMember> = Vec::new();
        for (index, child) in layer.children.iter().enumerate() {
            if !renders_flat(child) {
                continue;
            }
            let frame = ChildFrame::of(child, scale, target);
            let Some(visible) = frame.visible else {
                continue;
            };
            if composites_nothing(child) {
                continue;
            }
            let in_place = pass.can_draw_in_place(child);
            let shown = child_surface_bound(child, frame.snap, scale, target).unwrap_or(visible);
            let Some(plan) = SurfacePlan::of(child, scale, frame.grid, shown) else {
                resolved[index] = Some(if in_place {
                    Resolved::InPlace
                } else {
                    Resolved::Surface(None)
                });
                continue;
            };
            // A member rendered with the others lands in a shared atlas, so
            // keeping its surface means copying it out.
            let gate = if in_place {
                AdmissionGate::drawn_in_place
            } else {
                AdmissionGate::rendered
            };
            resolved[index] = match self.source_decision(child, &plan, gate) {
                SourceDecision::Cached(surface) => Some(Resolved::Surface(Some(surface))),
                SourceDecision::Render(None) if in_place => Some(Resolved::InPlace),
                SourceDecision::Render(retain) => {
                    batch.push(BatchMember {
                        index,
                        plan,
                        retain,
                    });
                    None
                }
            };
        }
        for (index, surface) in self.render_surface_batch(layer, batch)? {
            resolved[index] = Some(Resolved::Surface(Some(surface)));
        }
        pass.resolved = resolved;
        Ok(())
    }

    fn render_surface_batch(
        &mut self,
        layer: &LayerScene,
        batch: Vec<BatchMember>,
    ) -> Result<Vec<(usize, SurfaceRender)>, String> {
        let mut surfaces = Vec::with_capacity(batch.len());
        match batch.as_slice() {
            [] => {}
            [member] => surfaces.push((member.index, self.render_member_alone(layer, member)?)),
            group => self.render_surface_atlas(layer, group, &mut surfaces)?,
        }
        Ok(surfaces)
    }

    fn render_member_alone(
        &mut self,
        layer: &LayerScene,
        member: &BatchMember,
    ) -> Result<SurfaceRender, String> {
        let cleared = Beneath::over(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        self.render_planned_surface(
            &layer.children[member.index],
            &member.plan,
            member.retain,
            &cleared,
        )
    }

    fn render_surface_atlas(
        &mut self,
        layer: &LayerScene,
        group: &[BatchMember],
        surfaces: &mut Vec<(usize, SurfaceRender)>,
    ) -> Result<(), String> {
        let limit = self.renderer.max_texture_dim();
        let area: u64 = group
            .iter()
            .map(|member| u64::from(member.plan.width) * u64::from(member.plan.height))
            .sum();
        let mut packer =
            AtlasPacker::new(limit).with_shelf_width(u32::try_from(area.isqrt()).unwrap_or(limit));
        // Each surface is copied out of the atlas, so where it lands costs
        // nothing later: the tallest go first, into an atlas near square.
        let mut tallest_first: Vec<usize> = (0..group.len()).collect();
        tallest_first.sort_by_key(|&index| std::cmp::Reverse(group[index].plan.height));
        let mut placements: Vec<Option<AtlasPlacement>> = vec![None; group.len()];
        for index in tallest_first {
            let plan = &group[index].plan;
            placements[index] = packer.place(plan.width, plan.height);
        }
        let sizes: Vec<(u32, u32)> = packer
            .atlases
            .iter()
            .map(|atlas| atlas.padded_size(limit))
            .collect();
        let atlases: Vec<Rc<OffscreenTarget>> = sizes
            .into_iter()
            .map(|(width, height)| self.acquire_transient("Layer Surface Atlas", width, height))
            .collect();
        let ops: Vec<Cow<'_, [DrawOp]>> = group
            .iter()
            .map(|member| z_ordered_ops(&layer.children[member.index].content.scene.draw_ops))
            .collect();
        for (atlas_index, atlas) in atlases.iter().enumerate() {
            let segments: Vec<PassSegment<'_>> = group
                .iter()
                .zip(&placements)
                .zip(&ops)
                .filter_map(|((member, placement), ops)| {
                    let placement = placement.filter(|placement| placement.atlas == atlas_index)?;
                    let plan = &member.plan;
                    Some(PassSegment {
                        scene: &layer.children[member.index].content.scene,
                        ops,
                        composites: &[],
                        offset: [
                            plan.surface_rect.x - placement.x as f32,
                            plan.surface_rect.y - placement.y as f32,
                        ],
                        scissor: Some((placement.x, placement.y, plan.width, plan.height)),
                        first_run_window: None,
                        transform: SegmentTransform::IDENTITY,
                        scale: plan.surface_scale,
                    })
                })
                .collect();
            let target = PassTarget {
                view: &atlas.view,
                width: atlas.width,
                height: atlas.height,
            };
            self.renderer.encode_pass(
                self.recorder,
                target,
                &segments,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                "Layer Surface Atlas Pass",
            )?;
        }
        for (member, placement) in group.iter().zip(placements) {
            let child = &layer.children[member.index];
            let surface = match placement {
                Some(placement) => {
                    self.renderer.frame_stats.record_isolated_layer_render(
                        member.plan.width,
                        member.plan.height,
                        child.node_id,
                        member.plan.surface_logical,
                    );
                    self.retain_from_atlas(child, member, &atlases[placement.atlas], placement)
                }
                None => self.render_member_alone(layer, member)?,
            };
            surfaces.push((member.index, surface));
        }
        Ok(())
    }

    fn retain_from_atlas(
        &mut self,
        child: &ChildLayer,
        member: &BatchMember,
        atlas: &Rc<OffscreenTarget>,
        placement: AtlasPlacement,
    ) -> SurfaceRender {
        let plan = &member.plan;
        let in_atlas = plan.surface(
            CompositeSource {
                texture: Rc::clone(atlas),
                content: SourceContent::Transient,
            },
            Some(DeviceRect {
                x: placement.x as f32,
                y: placement.y as f32,
                width: plan.width as f32,
                height: plan.height as f32,
            }),
        );
        let Some(key) = member.retain else {
            return in_atlas;
        };
        let retained = Rc::new(
            self.renderer
                .acquire_retained_surface(plan.width, plan.height),
        );
        if !copy_compatible(atlas, &retained) {
            return in_atlas;
        }
        self.recorder.copy_texture_region(TextureRegionCopy {
            source: atlas,
            source_origin: [placement.x, placement.y],
            dest: &retained,
            dest_origin: [0, 0],
            size: [plan.width, plan.height],
        });
        if !self.retain_source(child.node_id, key, &retained) {
            return in_atlas;
        }
        plan.surface(
            CompositeSource {
                texture: retained,
                content: SourceContent::retained(&key),
            },
            None,
        )
    }

    fn cached_source(
        &mut self,
        node_id: Option<NodeId>,
        key: LayerRasterCacheKey,
        width: u32,
        height: u32,
    ) -> Option<Rc<OffscreenTarget>> {
        let retained = self.renderer.layer_cache.get(&key)?;
        self.renderer
            .frame_stats
            .record_layer_cache_hit(&key, width, height);
        self.supersede_source(node_id, key);
        if let Some(gate) = self.source_gate(node_id) {
            gate.hit(key);
        }
        Some(retained.texture)
    }

    fn retain_source(
        &mut self,
        node_id: Option<NodeId>,
        key: LayerRasterCacheKey,
        texture: &Rc<OffscreenTarget>,
    ) -> bool {
        self.renderer
            .frame_stats
            .record_layer_cache_miss(&key, texture.width, texture.height);
        let inserted =
            self.renderer
                .layer_cache
                .insert(key, Retained::surface(Rc::clone(texture)), None);
        if inserted && let Some(gate) = self.source_gate(node_id) {
            gate.admitted();
        }
        inserted
    }

    fn source_gate(&mut self, node_id: Option<NodeId>) -> Option<&mut AdmissionGate> {
        node_id.and_then(|node_id| self.renderer.source_gates.get_mut(&node_id))
    }

    fn supersede_source(&mut self, node_id: Option<NodeId>, key: LayerRasterCacheKey) {
        let Some(previous) = self.source_gate(node_id).map(|gate| gate.key) else {
            return;
        };
        if previous.draws_other_content(key) {
            self.renderer.layer_cache.remove(&previous);
        }
    }

    fn admits_source(
        &mut self,
        node_id: Option<NodeId>,
        key: LayerRasterCacheKey,
        (width, height): (u32, u32),
        gate: fn(LayerRasterCacheKey) -> AdmissionGate,
    ) -> bool {
        self.supersede_source(node_id, key);
        let admits = match node_id {
            None => true,
            Some(node_id) => match self.renderer.source_gates.entry(node_id) {
                Entry::Occupied(mut gate) => {
                    if let Some(dead) = gate.get_mut().observe(key) {
                        self.renderer.layer_cache.remove(&dead);
                    }
                    gate.get().admits()
                }
                Entry::Vacant(slot) => slot.insert(gate(key)).admits(),
            },
        };
        admits && self.renderer.layer_cache.fits(width, height)
    }
}

fn backdrop_reach(layer: &LayerScene) -> f32 {
    let padding = |effect: &RenderEffect| effect.input_padding() + effect.output_padding();
    let own = layer
        .scene
        .backdrop_layers
        .iter()
        .map(|backdrop| padding(&backdrop.effect));
    let children = layer.children.iter().map(|child| {
        child
            .backdrop
            .as_ref()
            .map_or(0.0, padding)
            .max(child.surface_scale * backdrop_reach(&child.content))
    });
    own.chain(children).fold(0.0, f32::max)
}

/// What lies beneath an isolated child that reads its backdrop: the parent's
/// page, drawn up to the child, re-based into the child's device space when
/// the child only translates at the parent's scale and projected into it
/// otherwise; and the parent's content described for the cache key.
fn beneath_for_child<'a>(
    pass: &'a mut LayerPass<'_>,
    child: &ChildLayer,
    z: usize,
    shift: Option<Point>,
) -> Result<Beneath<'a>, String> {
    let scale = pass.scale;
    let source = Rc::clone(&pass.page.texture);
    let origin = pass.page.offset;
    let placement = match shift {
        Some(shift) => PagePlacement::Translated {
            shift: [shift.x, shift.y],
        },
        None => projected_placement(pass, child, scale)?,
    };
    let page = Some(PageBase {
        source,
        origin,
        placement,
    });
    let shift = shift.unwrap_or_default();
    ensure_sorted_by_key(&mut pass.pending, composite_z);
    let scene = &pass.layer.scene;
    let drawn = &pass.drawn[..pass
        .drawn
        .partition_point(|composite| composite.z_index <= z)];
    let pending = &pass.pending[..pass
        .pending
        .partition_point(|composite| composite.z_index <= z)];
    let source = Some(BeneathSegment {
        base: pass.beneath.base,
        scene,
        z_end: z + 1,
        drawn,
        pending,
        placement: [-shift.x, -shift.y],
    });
    Ok(Beneath {
        base: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        page,
        source,
    })
}

/// The parent page's pixels under a transformed child, mapped into the
/// child's device space.
fn projected_placement(
    pass: &LayerPass<'_>,
    child: &ChildLayer,
    scale: f32,
) -> Result<PagePlacement, String> {
    let snap = child
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default();
    let parent_rect = pass.target_rect();
    let surface_scale = scale * child.surface_scale;
    let child_device_to_parent_device = ProjectiveTransform::uniform_scale(1.0 / surface_scale)
        .then(child.transform)
        .then(ProjectiveTransform::translation(snap.x, snap.y))
        .then(ProjectiveTransform::uniform_scale(scale));
    let parent_device_to_page =
        ProjectiveTransform::translation(-pass.page.offset[0], -pass.page.offset[1]);
    let child_device_to_page = child_device_to_parent_device.then(parent_device_to_page);
    let page_to_child_device = child_device_to_page
        .inverse()
        .ok_or_else(|| "child transform is not invertible".to_string())?;
    let source_rect = Rect {
        x: parent_rect.x - pass.page.offset[0],
        y: parent_rect.y - pass.page.offset[1],
        width: parent_rect.width,
        height: parent_rect.height,
    };
    let row = page_to_child_device.matrix()[2];
    let denominators = ProjectiveTransform::identity()
        .map_rect(source_rect)
        .map(|[x, y]| row[0] * x + row[1] * y + row[2]);
    let bounded = denominators.iter().all(|&w| w > f32::EPSILON)
        || denominators.iter().all(|&w| w < -f32::EPSILON);
    let bounds = bounded.then(|| quad_device_bounds(page_to_child_device.map_rect(source_rect)));
    Ok(PagePlacement::Projected {
        bounds,
        inverse: child_device_to_page.matrix(),
    })
}

/// Splits an effect that ends in a runtime shader into the effects before
/// it and the shader itself, so the shader can draw straight into the final
/// pass instead of through one more texture. `None` when the effect does not
/// end in a shader.
fn shader_tail(effect: &RenderEffect) -> Option<(Option<&RenderEffect>, &Arc<RuntimeShader>)> {
    match effect {
        RenderEffect::Shader { shader } => Some((None, shader)),
        RenderEffect::Chain { first, second } => match second.as_ref() {
            RenderEffect::Shader { shader } => Some((Some(first.as_ref()), shader)),
            _ => None,
        },
        _ => None,
    }
    .filter(|(_, shader)| shader.substrates().is_empty())
}

/// The rounded mask of a child whose transform keeps its rounded clip
/// axis-aligned, a uniform scale and a translation, with the radii scaled
/// as the child is; none under a rotation or a projection, which no
/// axis-aligned mask matches.
fn grid_rounded_mask(child: &ChildLayer, snap: Point, scale: f32) -> Option<RoundedCompositeMask> {
    let clip = child.rounded_clip?;
    let (uniform, _) = uniform_scale_translation(child.transform)?;
    Some(rounded_mask(
        LayerRoundedClip {
            rect: quad_bounds(child.transform.map_rect(clip.rect)).translate(snap.x, snap.y),
            radii: clip.radii.map(|radius| radius * uniform),
        },
        Point::default(),
        scale,
    ))
}

fn rounded_mask(clip: LayerRoundedClip, snap: Point, scale: f32) -> RoundedCompositeMask {
    let rect = DeviceRect::from_logical(clip.rect.translate(snap.x, snap.y), scale);
    RoundedCompositeMask {
        rect: [rect.x, rect.y, rect.width, rect.height],
        radii: clip.radii.map(|radius| radius * scale),
    }
}

fn quad_device_bounds(quad: [[f32; 2]; 4]) -> DeviceRect {
    let bounds = quad_bounds(quad);
    DeviceRect {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    }
}

/// Source pixel -> parent device pixel for a projective child surface.
fn surface_to_parent_device(
    surface: &SurfaceRender,
    transform: ProjectiveTransform,
    snap: Point,
    scale: f32,
) -> ProjectiveTransform {
    ProjectiveTransform::translation(surface.rect.x, surface.rect.y)
        .then(ProjectiveTransform::uniform_scale(1.0 / surface.scale))
        .then(transform)
        .then(ProjectiveTransform::translation(snap.x, snap.y))
        .then(ProjectiveTransform::uniform_scale(scale))
}

fn filtered_ops<'a>(
    ops: &'a [DrawOp],
    z_end: usize,
    excluded: &[(usize, usize)],
) -> Cow<'a, [DrawOp]> {
    filtered_ops_in_range(ops, 0, z_end, excluded)
}

fn filtered_ops_in_range<'a>(
    ops: &'a [DrawOp],
    z_start: usize,
    z_end: usize,
    excluded: &[(usize, usize)],
) -> Cow<'a, [DrawOp]> {
    if z_end <= z_start {
        return Cow::Borrowed(&[]);
    }
    let start = ops.partition_point(|op| op.z_index < z_start);
    let end = ops.partition_point(|op| op.z_index < z_end);
    let range = &ops[start..end];
    if excluded.is_empty() {
        return Cow::Borrowed(range);
    }
    Cow::Owned(
        range
            .iter()
            .filter(|op| {
                !excluded
                    .iter()
                    .any(|(from, to)| op.z_index >= *from && op.z_index < *to)
            })
            .copied()
            .collect(),
    )
}

fn pending_draw_ops<'a>(
    scene_ops: &'a [DrawOp],
    drawn_z: usize,
    z: usize,
    excluded: &[(usize, usize)],
    deferred: &'a [DrawOp],
) -> Cow<'a, [DrawOp]> {
    let ops = filtered_ops_in_range(scene_ops, drawn_z, z, excluded);
    let deferred_end = deferred.partition_point(|op| op.z_index < z);
    if deferred_end == 0 {
        return ops;
    }
    let deferred = &deferred[..deferred_end];
    if ops.is_empty() {
        return Cow::Borrowed(deferred);
    }
    merge_draw_ops(ops, deferred.iter().copied())
}

fn merge_draw_ops(
    ops: Cow<'_, [DrawOp]>,
    deferred: impl ExactSizeIterator<Item = DrawOp>,
) -> Cow<'_, [DrawOp]> {
    let deferred_len = deferred.len();
    let mut merged = match ops {
        Cow::Owned(ops) => ops,
        Cow::Borrowed(ops) => {
            let mut merged = Vec::with_capacity(ops.len() + deferred_len);
            merged.extend_from_slice(ops);
            merged
        }
    };
    merged.extend(deferred);
    merged.sort_by_key(draw_op_z);
    Cow::Owned(merged)
}

/// The logical rect a child's surface covers: everything its content draws,
/// clipped to its bounds when it clips, expanded for its effect's reach.
fn child_surface_rect(child: &ChildLayer, scale: f32) -> Option<Rect> {
    let surface_scale = scale * child.surface_scale;
    let bounds = if child.rounded_clip.is_some() {
        Some(child.local_bounds)
    } else {
        union_rect(
            Some(child.local_bounds),
            scene_bounds(&child.content, surface_scale),
        )
    }?;
    let padding = child.effect.as_ref().map_or(0.0, |effect| {
        effect.input_padding() + effect.output_padding()
    });
    let rect = expand_rect(bounds, padding);
    let rect = surface_within_budget(
        rect,
        expand_rect(child.local_bounds, padding),
        surface_scale,
    );
    (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
}

fn expand_rect(rect: Rect, padding: f32) -> Rect {
    if padding <= 0.0 {
        return rect;
    }
    Rect {
        x: rect.x - padding,
        y: rect.y - padding,
        width: rect.width + padding * 2.0,
        height: rect.height + padding * 2.0,
    }
}

fn surface_pixels(rect: Rect, scale: f32) -> u64 {
    let width = (rect.width * scale).ceil().max(0.0) as u64;
    let height = (rect.height * scale).ceil().max(0.0) as u64;
    width * height
}

/// Keeps a child's surface inside the pixel budget by falling back to the
/// child's own box.
///
/// The rect above covers everything the content draws, and a scroll or a wide
/// row reaches far past what the parent shows: one LeetCodeDaily draft made a
/// 1412x1480 layer ask for 5403x3315, which is 17.9M pixels against a 16.7M
/// budget. Over the budget the caller has no texture to render into and drops
/// the layer whole, so 555 draw ops of application UI became a blank window
/// with no error anywhere.
///
/// The child's own box is what the parent positions and clips, so anything
/// outside it was already clipped away or off the frame. Trading the overflow
/// for the visible pixels is the only answer that draws something.
fn surface_within_budget(content: Rect, own_box: Rect, scale: f32) -> Rect {
    if surface_pixels(content, scale) <= MAX_SURFACE_PIXELS {
        return content;
    }
    if surface_pixels(own_box, scale) < surface_pixels(content, scale) {
        return own_box;
    }
    content
}

fn union_rect(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let left = a.x.min(b.x);
            let top = a.y.min(b.y);
            let right = (a.x + a.width).max(b.x + b.width);
            let bottom = (a.y + a.height).max(b.y + b.height);
            Some(Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            })
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn clipped(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}

pub(crate) fn scene_bounds(layer: &LayerScene, scale: f32) -> Option<Rect> {
    let scene = &layer.scene;
    let mut bounds = None;
    for op in &scene.draw_ops {
        let rect = match op.kind {
            DrawOpKind::Run(index) => {
                let run = &scene.runs[index];
                clipped(run.bounds, run.placement.clip)
            }
            DrawOpKind::Image(index) => {
                let image = &scene.images[index];
                clipped(quad_bounds(image.quad), image.clip)
            }
            DrawOpKind::Text(index) => {
                let text = &scene.texts[index];
                clipped(text.rect, text.clip)
            }
            DrawOpKind::RRectShadow(index) => {
                crate::rrect_shadow::rrect_shadow_bounds(&scene.rrect_shadows[index], scale)
            }
            DrawOpKind::Shadow(index) => {
                let shadow = &scene.shadow_draws[index];
                let mut shadow_bounds = None;
                if let Some(run) = &shadow.shapes {
                    shadow_bounds = union_rect(shadow_bounds, Some(run.bounds));
                }
                for text in &shadow.texts {
                    shadow_bounds = union_rect(shadow_bounds, Some(text.rect));
                }
                shadow_bounds.and_then(|rect| {
                    let margin =
                        cranpose_render_common::geometry::blur_reach(shadow.blur_radius, scale);
                    clipped(
                        Rect {
                            x: rect.x - margin,
                            y: rect.y - margin,
                            width: rect.width + margin * 2.0,
                            height: rect.height + margin * 2.0,
                        },
                        shadow.clip,
                    )
                })
            }
        };
        bounds = union_rect(bounds, rect);
    }
    for effect in &scene.effect_layers {
        bounds = union_rect(bounds, clipped(effect.rect, effect.clip));
    }
    for backdrop in &scene.backdrop_layers {
        bounds = union_rect(bounds, clipped(backdrop.rect, backdrop.clip));
    }
    for child in &layer.children {
        let surface_rect = child_surface_rect(child, scale).unwrap_or(child.local_bounds);
        let child_bounds = quad_bounds(child.transform.map_rect(surface_rect));
        bounds = union_rect(bounds, clipped(child_bounds, child.clip));
    }
    bounds
}

#[cfg(test)]
#[path = "tests/frame_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/frame_atlas_padding_tests.rs"]
mod atlas_padding_tests;
