use std::{
    borrow::Cow,
    collections::hash_map::Entry,
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{CachePolicy, ProjectiveTransform, quad_bounds},
    raster_cache::{LayerRasterCacheKey, ScaleBucket},
};
use cranpose_ui_graphics::{
    BlendMode, MAX_SUBSTRATES, Point, Rect, RenderEffect, RenderHash, RuntimeShader, SubstrateSpec,
    TileMode,
};

use crate::{
    ablation::Ablation,
    capture_hash::{CaptureWindow, capture_hasher, hash_capture_composites, hash_capture_ops},
    collect::{ChildLayer, LayerScene, uniform_scale_translation},
    debug_toggles::DebugToggle,
    draw_pass::{
        PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind, SourceContent,
        op_draw_bounds, segment_draws_anything,
    },
    effect_renderer::{
        AtlasSideWork, BlurRegion, CompositeSampleMode, EffectReads, EffectScratchTargetProvider,
        RoundedCompositeMask, SubstrateRegion, SubstrateRegions, blur_scratch_size,
        substrate_scratch_size,
    },
    frame_graph::{
        FrameCommandRecorder, FrameTextureDescriptor, TextureRegionCopy, copy_compatible,
    },
    geometry::snap_delta_for_anchor,
    layer_cache::{Retained, RetainedContent},
    offscreen::{OffscreenTarget, composition_format},
    opaque_prefix::{OpaquePrefix, PrefixContext, opaque_prefix},
    render::GpuRenderer,
    scene::{BackdropLayer, CompositorScene, DrawOp, DrawOpKind, EffectLayer, LayerRoundedClip},
};

const MAX_SURFACE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_RESOLVE_DEPTH: usize = 24;

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
        Self {
            x: rect.x * scale,
            y: rect.y * scale,
            width: rect.width * scale,
            height: rect.height * scale,
        }
    }

    fn tuple(self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.height)
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

    /// What is left of the rect outside `hole`: up to four rects that
    /// partition it exactly, none overlapping the hole.
    fn subtract(self, hole: Self) -> Vec<Self> {
        let Some(hole) = hole.intersect(self) else {
            return vec![self];
        };
        let right = self.x + self.width;
        let bottom = self.y + self.height;
        let hole_right = hole.x + hole.width;
        let hole_bottom = hole.y + hole.height;
        let mut parts = Vec::with_capacity(4);
        let mut push = |x: f32, y: f32, width: f32, height: f32| {
            if width > 0.0 && height > 0.0 {
                parts.push(Self {
                    x,
                    y,
                    width,
                    height,
                });
            }
        };
        push(self.x, self.y, self.width, hole.y - self.y);
        push(self.x, hole_bottom, self.width, bottom - hole_bottom);
        push(self.x, hole.y, hole.x - self.x, hole.height);
        push(hole_right, hole.y, right - hole_right, hole.height);
        parts
    }

    /// The rect minus every hole, as rects that partition what is left.
    fn subtract_all(self, holes: &[Self]) -> Vec<Self> {
        holes.iter().fold(vec![self], |parts, hole| {
            parts
                .into_iter()
                .flat_map(|part| part.subtract(*hole))
                .collect()
        })
    }

    fn pixel_size(self) -> (u32, u32) {
        (
            (self.width.ceil().max(1.0)) as u32,
            (self.height.ceil().max(1.0)) as u32,
        )
    }
}

/// What a layer's captures read beneath the layer's own pixels: the base its
/// page starts from, the parent's page under it when it is an isolated child
/// reading its backdrop, and a description of that parent content for the
/// backdrop result cache's key.
struct Beneath<'a> {
    base: wgpu::LoadOp<wgpu::Color>,
    page: Option<PageBase>,
    described: Vec<BeneathSegment<'a>>,
}

/// One ancestor scene's content beneath a layer, as the backdrop result
/// cache hashes it: the ops below `z_end` outside `excluded`, the composites
/// already drawn and still pending below it, and where the scene's device
/// origin sits in the layer's device space.
#[derive(Clone, Copy)]
struct BeneathSegment<'a> {
    scene: &'a CompositorScene,
    z_end: usize,
    drawn: &'a [ResolvedComposite],
    pending: &'a [ResolvedComposite],
    excluded: &'a [(usize, usize)],
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
        dest_quad: [[f32; 2]; 4],
        inverse: [[f32; 3]; 3],
    },
}

impl PageBase {
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
            PagePlacement::Projected { dest_quad, inverse } => Some(ResolvedComposite {
                z_index: 0,
                source: Rc::clone(&self.source),
                content: SourceContent::Transient,
                dest: quad_device_bounds(dest_quad).tuple(),
                scissor: None,
                kind: ResolvedCompositeKind::Projective {
                    dest_quad,
                    inverse,
                    alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    sample_mode: CompositeSampleMode::Linear,
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
            offset: self.offset,
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
        let coords = [
            source[0],
            source[1],
            rect.width,
            rect.height,
            origin[0],
            origin[1],
        ];
        if coords
            .iter()
            .any(|value| value.fract() != 0.0 || *value < 0.0)
        {
            return None;
        }
        let size = [rect.width as u32, rect.height as u32];
        let source_origin = [source[0] as u32, source[1] as u32];
        let dest_origin = [origin[0] as u32, origin[1] as u32];
        let fits = |origin: [u32; 2], target: &OffscreenTarget| {
            origin[0] + size[0] <= target.width && origin[1] + size[1] <= target.height
        };
        (fits(source_origin, &self.texture) && fits(dest_origin, dest)).then_some(
            TextureRegionCopy {
                source: &self.texture,
                source_origin,
                dest,
                dest_origin,
                size,
            },
        )
    }
}

/// One layer's render in progress: the page it draws into, the strata it has
/// drawn (every op below `drawn_z` except the deferred ones, and every
/// composite in `drawn`), the composites still to draw, the ops held back
/// behind a captured glass, the glasses of the running stage whose
/// composites are not on the page yet, and the backdrops waiting for their
/// stage.
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

/// One thing a flush may draw, in the order the pass draws them: at one z
/// a composite before an op. A composite is named by its index in the
/// flush's list, so the candidates stay small enough to sort in place.
enum Candidate {
    Composite { z: usize, index: usize },
    Op(DrawOp),
}

impl Candidate {
    fn order(&self) -> (usize, u8) {
        match self {
            Candidate::Composite { z, .. } => (*z, 0),
            Candidate::Op(op) => (op.z_index, 1),
        }
    }
}

impl LayerPass<'_> {
    fn target_rect(&self) -> DeviceRect {
        self.page.rect()
    }

    /// The ops between the page's drawn z and `z` outside the excluded
    /// ranges, with the deferred ops below `z`, in z order.
    fn ops_below(&self, z: usize) -> Vec<DrawOp> {
        let deferred_end = self.deferred.partition_point(|op| op.z_index < z);
        let mut ops =
            filtered_ops_in_range(&self.layer.scene.draw_ops, self.drawn_z, z, &self.excluded);
        ops.extend_from_slice(&self.deferred[..deferred_end]);
        ops.sort_by_key(|op| op.z_index);
        ops
    }

    /// Splits what a flush would draw into what draws now and what waits.
    /// In z order, anything that touches a blocker below it waits: an op is
    /// deferred whole, a composite is drawn outside the blockers it overlaps
    /// and its covered parts stay pending; and what waits blocks in turn, so
    /// nothing above it that overlaps it is drawn before it.
    fn release(
        &mut self,
        ops: Vec<DrawOp>,
        composites: Vec<ResolvedComposite>,
    ) -> (Vec<DrawOp>, Vec<ResolvedComposite>) {
        let scene = &self.layer.scene;
        let scale = self.scale;
        let mut candidates: Vec<Candidate> = composites
            .iter()
            .enumerate()
            .map(|(index, composite)| Candidate::Composite {
                z: composite.z_index,
                index,
            })
            .chain(ops.into_iter().map(Candidate::Op))
            .collect();
        candidates.sort_by_key(Candidate::order);
        let mut composites: Vec<Option<ResolvedComposite>> =
            composites.into_iter().map(Some).collect();
        let mut holes = self.blockers.clone();
        let mut now_ops = Vec::new();
        let mut now = Vec::new();
        for candidate in candidates {
            match candidate {
                Candidate::Op(op) => {
                    let bounds = op_draw_bounds(scene, &op, scale)
                        .map(|bounds| DeviceRect::from_logical(bounds, scale));
                    let blocked = bounds.filter(|bounds| {
                        holes.iter().any(|hole| {
                            hole.z < op.z_index && hole.rect.intersect(*bounds).is_some()
                        })
                    });
                    match blocked {
                        Some(rect) => {
                            holes.push(Blocker {
                                z: op.z_index,
                                rect,
                            });
                            self.deferred.push(op);
                        }
                        None => now_ops.push(op),
                    }
                }
                Candidate::Composite { index, .. } => {
                    let composite = composites[index]
                        .take()
                        .expect("a flush releases each composite once");
                    let Some(coverage) = composite_coverage(&composite) else {
                        continue;
                    };
                    let covered: Vec<DeviceRect> = holes
                        .iter()
                        .filter(|hole| hole.z < composite.z_index)
                        .filter_map(|hole| hole.rect.intersect(coverage))
                        .collect();
                    if covered.is_empty() {
                        now.push(composite);
                        continue;
                    }
                    now.extend(
                        coverage
                            .subtract_all(&covered)
                            .into_iter()
                            .map(|part| with_scissor(&composite, part)),
                    );
                    for (index, hole) in covered.iter().enumerate() {
                        for part in hole.subtract_all(&covered[..index]) {
                            holes.push(Blocker {
                                z: composite.z_index,
                                rect: part,
                            });
                            self.pending.push(with_scissor(&composite, part));
                        }
                    }
                }
            }
        }
        self.deferred.sort_by_key(|op| op.z_index);
        (now_ops, now)
    }

    /// The pending composites below `z`, in z order.
    fn pending_below(&mut self, z: usize) -> &[ResolvedComposite] {
        self.pending.sort_by_key(|composite| composite.z_index);
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
    Shader(&'a RuntimeShader),
    BlurThenShader(BlurSpec, &'a RuntimeShader),
}

impl<'a> BatchedEffect<'a> {
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
        log::warn!(
            "[stage-diag] stage={stage} z={} capture=({:.0},{:.0},{:.0},{:.0}) visible=({:.0},{:.0},{:.0},{:.0}) batched={} blur={blur} substrates={substrates}",
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
        if shadow.blur_radius > 0.0 {
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
) -> Option<PendingBackdrop<'_>> {
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
    let capture_rect = visible
        .expand(padding.ceil())
        .intersect(target_rect)
        .unwrap_or(visible)
        .snap_out();
    Some(PendingBackdrop {
        z,
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
                other.z < item.z && other.visible.intersect(item.capture_rect).is_some()
            })
            .map(|other| other.stage + 1)
            .max()
            .unwrap_or(0);
        self.pending.push(item);
    }
}

/// Where a child lands in its parent: its z, the device pixels it may
/// touch, its device bounds, and its translation when it only translates.
#[derive(Clone, Copy)]
struct ChildPlacement {
    z: usize,
    visible: DeviceRect,
    support: DeviceRect,
    dest: DeviceRect,
    translation: Option<Point>,
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

/// Whether a child's runtime shader can draw in the final pass over the
/// child's content: the shader must apply the child's clip and alpha itself
/// unless the child has neither.
fn shader_tail_composites(child: &ChildLayer, shader: &RuntimeShader) -> bool {
    let plain = child.alpha >= 1.0 && child.rounded_clip.is_none();
    child.blend_mode == BlendMode::SrcOver && (plain || shader.batched_source())
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
#[allow(clippy::too_many_arguments)]
fn shader_tail_composite(
    child: &ChildLayer,
    shader: &RuntimeShader,
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
            shader: Rc::new(shader.clone()),
            layer_pixel_rect,
            source_region: None,
            source_logical_size: None,
            substrate_regions: [None; MAX_SUBSTRATES],
            rounded_mask,
            alpha: child.alpha,
        },
    }
}

/// A translated child's runtime shader drawn in the final pass over its
/// rendered content, so an animated shader over cached content costs no
/// pass; none when the child is not on the parent grid or the shader cannot
/// apply the child's clip and alpha.
fn shader_tail_over_surface(
    child: &ChildLayer,
    surface: &SurfaceRender,
    translation: Option<Point>,
    snap: Point,
    z: usize,
    scale: f32,
    visible: DeviceRect,
) -> Option<ResolvedComposite> {
    let Some(RenderEffect::Shader { shader }) = &child.effect else {
        return None;
    };
    let dest = surface.grid_dest.filter(|_| translation.is_some())?;
    shader_tail_composites(child, shader).then(|| {
        shader_tail_composite(
            child,
            shader,
            z,
            surface.source.clone(),
            dest,
            layer_pixel_rect(child, surface.rect, surface.scale),
            grid_rounded_mask(child, snap, scale),
            visible,
        )
    })
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

/// The composite restricted to `scissor`, a part of its coverage.
fn with_scissor(composite: &ResolvedComposite, scissor: DeviceRect) -> ResolvedComposite {
    ResolvedComposite {
        scissor: Some(scissor.tuple()),
        ..composite.clone()
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
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_mask: item.rounded_mask,
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: None,
        },
    }
}

/// A child whose surface lies on the parent's pixel grid, blitted one to one
/// at `dest`.
fn grid_child_composite(
    child: &ChildLayer,
    z: usize,
    source: CompositeSource,
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
            source_viewport: None,
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
            sample_mode: CompositeSampleMode::Linear,
        },
    })
}

fn replayed_kind(
    kind: &ResolvedCompositeKind,
    item: &PendingBackdrop<'_>,
) -> ResolvedCompositeKind {
    match kind {
        ResolvedCompositeKind::Blit {
            alpha,
            blend_mode,
            sample_mode,
            source_viewport,
            ..
        } => ResolvedCompositeKind::Blit {
            alpha: *alpha,
            blend_mode: *blend_mode,
            rounded_mask: item.rounded_mask,
            sample_mode: *sample_mode,
            source_viewport: *source_viewport,
        },
        ResolvedCompositeKind::Shader {
            shader,
            source_region,
            source_logical_size,
            substrate_regions,
            alpha,
            ..
        } => ResolvedCompositeKind::Shader {
            shader: Rc::clone(shader),
            layer_pixel_rect: item.layer_pixel_rect(),
            source_region: *source_region,
            source_logical_size: *source_logical_size,
            substrate_regions: *substrate_regions,
            rounded_mask: item.rounded_mask,
            alpha: *alpha,
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
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask: item.rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: Some(region),
            };
            let kind = match item.batched.expect("packed items are batched") {
                _ if glass_as_blit => blit,
                BatchedEffect::Blur(_) => ResolvedCompositeKind::Blit {
                    alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    rounded_mask: item.rounded_mask,
                    sample_mode: CompositeSampleMode::Linear,
                    source_viewport: Some(region),
                },
                BatchedEffect::Shader(shader) | BatchedEffect::BlurThenShader(_, shader) => {
                    ResolvedCompositeKind::Shader {
                        shader: Rc::new(shader.clone()),
                        layer_pixel_rect: item.layer_pixel_rect(),
                        source_region: Some(region),
                        source_logical_size: logical_size,
                        substrate_regions,
                        rounded_mask: item.rounded_mask,
                        alpha: 1.0,
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

fn region_tuple((x, y, width, height): TexelRect) -> (f32, f32, f32, f32) {
    (x as f32, y as f32, width as f32, height as f32)
}

/// The texels a substrate of a capture occupies: a block average keeps one
/// texel per block, a blur its scratch size.
fn substrate_size(spec: SubstrateSpec, (width, height): (u32, u32)) -> (u32, u32) {
    match spec {
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
    atlas_slot: Option<TexelRect>,
}

#[derive(Clone, Default)]
struct SideSlots {
    blur: Option<TexelRect>,
    substrates: Vec<TexelRect>,
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
    substrates: Vec<Vec<PlannedSubstrate>>,
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

    fn atlas_views(&self) -> Vec<AtlasView<'_>> {
        (0..self.atlas_sizes.len())
            .map(|atlas| AtlasView {
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
            .collect()
    }

    fn restrict(&self, indices: &[usize]) -> Self {
        Self {
            atlas_sizes: self.atlas_sizes.clone(),
            placements: indices
                .iter()
                .map(|index| self.placements[*index])
                .collect(),
            substrates: indices
                .iter()
                .map(|index| self.substrates[*index].clone())
                .collect(),
            side_sizes: self.side_sizes.clone(),
            side: indices
                .iter()
                .map(|index| self.side[*index].clone())
                .collect(),
        }
    }
}

/// What a stage renders beside its capture atlas: the texture holding the
/// blurred regions, per atlas member the downscaled slot its blur wrote,
/// and per member the regions of its substrates in the texture it reads.
struct StageSideRegions {
    result: Rc<OffscreenTarget>,
    blurred: Vec<Option<TexelRect>>,
    substrates: Vec<SubstrateRegions>,
}

impl StageSideRegions {
    fn blurred_slot(&self, member: usize) -> Option<(&Rc<OffscreenTarget>, TexelRect)> {
        self.blurred[member].map(|slot| (&self.result, slot))
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
    atlases: Vec<Atlas>,
}

impl AtlasPacker {
    fn new(limit: u32) -> Self {
        Self {
            limit,
            atlases: Vec::new(),
        }
    }

    fn place(&mut self, width: u32, height: u32) -> Option<AtlasPlacement> {
        if width > self.limit || height > self.limit {
            return None;
        }
        for (atlas_index, atlas) in self.atlases.iter_mut().enumerate() {
            for shelf in &mut atlas.shelves {
                if shelf.height >= height && shelf.x + width <= self.limit {
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
    admitted_pixels: u64,
    prefix_admitted_pixels: u64,
}

const MAX_ADMISSION_PATIENCE: u32 = 16;

pub(crate) struct AdmissionGate {
    key: LayerRasterCacheKey,
    run: u32,
    patience: u32,
    unread: bool,
    seen: bool,
}

impl AdmissionGate {
    fn new(key: LayerRasterCacheKey) -> Self {
        Self {
            key,
            run: 1,
            patience: 1,
            unread: false,
            seen: true,
        }
    }

    fn observe(&mut self, key: LayerRasterCacheKey) {
        self.seen = true;
        if self.key == key {
            self.run = self.run.saturating_add(1);
            return;
        }
        if self.unread {
            self.patience = (self.patience * 2).min(MAX_ADMISSION_PATIENCE);
        }
        self.key = key;
        self.run = 1;
        self.unread = false;
    }

    fn admits(&self) -> bool {
        self.run > self.patience
    }

    fn admitted(&mut self) {
        self.unread = true;
    }

    fn hit(&mut self, key: LayerRasterCacheKey) {
        self.observe(key);
        self.patience = 1;
        self.unread = false;
    }

    pub(crate) fn end_frame(&mut self) -> bool {
        std::mem::take(&mut self.seen)
    }
}

const MAX_BACKDROP_ADMISSION_PIXELS: u64 = 120_000;

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
            admitted_pixels: 0,
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
        let beneath = Beneath {
            base: load_op,
            page: None,
            described: Vec::new(),
        };
        self.render_layer(root, page.clone(), root_scale, load_op, &beneath)?;
        if let Some(overlay) = overlay {
            let beneath = Beneath {
                base: wgpu::LoadOp::Load,
                page: None,
                described: Vec::new(),
            };
            self.render_layer(overlay, page, root_scale, wgpu::LoadOp::Load, &beneath)?;
        }
        self.release_transients();
        Ok(())
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
        let descriptor = FrameTextureDescriptor::render_attachment(
            label,
            width.min(max),
            height.min(max),
            self.renderer.composition_format,
        );
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
            return Err("layer nesting exceeds the resolve depth limit".to_string());
        }
        self.depth += 1;
        let result = self.render_layer_inner(layer, page, scale, load_op, beneath);
        self.depth -= 1;
        result
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
        };
        let target_rect = pass.target_rect();

        for (z, event) in layer_events(layer) {
            match event {
                Event::Backdrop(index) => {
                    let backdrop = &scene.backdrop_layers[index];
                    if !self.renderer.ablation.stages
                        && let Some(item) = plan_backdrop(backdrop, z, scale, target_rect)
                    {
                        pass.stages.push(item);
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
                Event::Child(index) => {
                    let child = &layer.children[index];
                    if child.reads_backdrop() {
                        self.run_stages(&mut pass)?;
                        self.flush_page(&mut pass, z + 1)?;
                    }
                    self.resolve_child(&mut pass, child)?;
                }
            }
        }
        self.run_stages(&mut pass)?;
        self.flush_page(&mut pass, usize::MAX)
    }

    /// Draws the next stratum: the ops from the last flush up to `z` outside
    /// the excluded ranges, the deferred ops below `z`, and every pending
    /// composite below `z`, except what still waits behind a blocker.
    fn flush_page(&mut self, pass: &mut LayerPass<'_>, z: usize) -> Result<(), String> {
        let ops = pass.ops_below(z);
        let deferred_end = pass.deferred.partition_point(|op| op.z_index < z);
        pass.deferred.drain(..deferred_end);
        pass.pending.sort_by_key(|composite| composite.z_index);
        let end = pass
            .pending
            .partition_point(|composite| composite.z_index < z);
        let composites: Vec<ResolvedComposite> = pass.pending.drain(..end).collect();
        let (ops, mut composites) = pass.release(ops, composites);
        let mut load_op = pass.load_op.take();
        if ops.is_empty() && composites.is_empty() && load_op.is_none() {
            pass.drawn_z = pass.drawn_z.max(z);
            return Ok(());
        }
        let first_run_window = match load_op {
            Some(base) => {
                self.reuse_opaque_prefix(pass, &ops, base, &mut composites, &mut load_op)?
            }
            None => None,
        };
        let segment = PassSegment {
            scene: &pass.layer.scene,
            ops: &ops,
            composites: &composites,
            offset: pass.page.offset,
            scissor: None,
            first_run_window,
        };
        let label = LAYER_PASS_LABELS[pass.segments.min(LAYER_PASS_LABELS.len() - 1)];
        pass.segments += 1;
        self.renderer.encode_pass(
            self.recorder,
            pass.page.pass_target(),
            std::slice::from_ref(&segment),
            load_op.unwrap_or(wgpu::LoadOp::Load),
            pass.scale,
            label,
        )?;
        pass.drawn.extend(composites);
        pass.drawn.sort_by_key(|composite| composite.z_index);
        pass.drawn_z = pass.drawn_z.max(z);
        Ok(())
    }

    fn reuse_opaque_prefix(
        &mut self,
        pass: &mut LayerPass<'_>,
        ops: &[DrawOp],
        base: wgpu::LoadOp<wgpu::Color>,
        composites: &mut Vec<ResolvedComposite>,
        load_op: &mut Option<wgpu::LoadOp<wgpu::Color>>,
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
            format: composition_format(),
        };
        let Some(prefix) = opaque_prefix(&context, ops) else {
            return Ok(None);
        };
        if composites
            .iter()
            .any(|composite| composite.z_index <= prefix.z_index)
        {
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
            composites.sort_by_key(|composite| composite.z_index);
            return Ok(Some(1..u32::MAX));
        }
        self.renderer
            .frame_stats
            .record_layer_cache_miss(&prefix.key, pixel_width, pixel_height);
        let admits = match self.renderer.fill_gates.entry(prefix.command) {
            Entry::Occupied(mut gate) => {
                gate.get_mut().observe(prefix.key);
                gate.get().admits()
            }
            Entry::Vacant(slot) => {
                slot.insert(AdmissionGate::new(prefix.key));
                false
            }
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
        };
        self.renderer.encode_pass(
            self.recorder,
            pass.page.pass_target(),
            std::slice::from_ref(&segment),
            base,
            pass.scale,
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
            pass.blockers = pending[start..]
                .iter()
                .map(|item| Blocker {
                    z: item.z,
                    rect: item.capture_rect,
                })
                .collect();
            let layout = {
                let stage_items: Vec<&PendingBackdrop<'_>> = pending[start..end].iter().collect();
                self.plan_stage(&stage_items, pass.scale)
            };
            let (items, indices) = self.take_uncached(pass, &mut pending[start..end], &layout);
            if !items.is_empty() {
                if diagnose {
                    log_stage(stage, &items);
                }
                let mut outputs = self.run_stage(pass, &items, &layout.restrict(&indices))?;
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
        layout: &StageLayout,
    ) -> (Vec<&'a PendingBackdrop<'scene>>, Vec<usize>) {
        let mut kept = Vec::with_capacity(items.len());
        let mut indices = Vec::with_capacity(items.len());
        let mut hits = Vec::new();
        for (index, item) in items.iter_mut().enumerate() {
            item.key = self.backdrop_cache_key(pass, item, layout.signature(index));
            match self.cached_backdrop(item) {
                Some(composite) => hits.push(composite),
                None => {
                    kept.push(&*item);
                    indices.push(index);
                }
            }
        }
        pass.pending.extend(hits);
        (kept, indices)
    }

    /// The cache key of a backdrop whose result can be reused: the hash of
    /// everything its capture reads, relative to the capture, with the
    /// effect and the capture's size. None when the backdrop is not batched,
    /// has no node, reads a projected parent page, or reads a texture drawn
    /// anew every frame.
    fn backdrop_cache_key(
        &self,
        pass: &mut LayerPass<'_>,
        item: &PendingBackdrop<'_>,
        layout: u64,
    ) -> Option<LayerRasterCacheKey> {
        let node_id = item.node_id?;
        item.batched.as_ref()?;
        if NO_BACKDROP_CACHE.flag() {
            return None;
        }
        if matches!(
            pass.beneath.page,
            Some(PageBase {
                placement: PagePlacement::Projected { .. },
                ..
            })
        ) {
            return None;
        }
        let scale = pass.scale;
        let mut hasher = capture_hasher();
        hash_base(pass.beneath.base, &mut hasher);
        for segment in &pass.beneath.described {
            let ops = filtered_ops(&segment.scene.draw_ops, segment.z_end, segment.excluded);
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
            if !hash_capture_composites(drawn, window, &mut hasher)
                || !hash_capture_composites(pending, window, &mut hasher)
            {
                return None;
            }
        }
        let window = capture_window(item.capture_rect);
        let ops = filtered_ops(&pass.layer.scene.draw_ops, item.z, &[]);
        hash_capture_ops(&pass.layer.scene, &ops, window, scale, &mut hasher);
        if !hash_capture_composites(pass.drawn_below(item.z), window, &mut hasher) {
            return None;
        }
        if !hash_capture_composites(pass.pending_below(item.z), window, &mut hasher) {
            return None;
        }
        layout.hash(&mut hasher);
        let [x, y, width, height] = item.layer_pixel_rect();
        Some(LayerRasterCacheKey::backdrop_effect(
            Some(node_id),
            hasher.finish(),
            item.effect.render_hash(),
            Rect {
                x,
                y,
                width,
                height,
            },
            item.capture_rect.pixel_size(),
            ScaleBucket::from_scale(scale),
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
            let admits = match self.renderer.backdrop_gates.entry(node_id) {
                Entry::Occupied(mut gate) => {
                    gate.get_mut().observe(key);
                    gate.get().admits()
                }
                Entry::Vacant(slot) => {
                    slot.insert(AdmissionGate::new(key));
                    false
                }
            };
            if !admits || self.admitted_pixels >= MAX_BACKDROP_ADMISSION_PIXELS {
                continue;
            }
            let Some(output) = outputs
                .iter_mut()
                .find(|composite| composite.z_index == item.z)
            else {
                continue;
            };
            let Some(descriptor) = self.transient_descriptor(&output.source) else {
                continue;
            };
            let retained = Retained::composite(Rc::clone(&output.source), output.kind.clone());
            if !self
                .renderer
                .layer_cache
                .insert(key, retained, Some(descriptor))
            {
                continue;
            }
            self.admitted_pixels += u64::from(width) * u64::from(height);
            self.renderer.frame_stats.record_backdrop_admission();
            if let Some(gate) = self.renderer.backdrop_gates.get_mut(&node_id) {
                gate.admitted();
            }
            output.content = SourceContent::retained(&key);
        }
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
            let side = self.stage_side_regions(texture, items, &view, scale)?;
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
    }

    fn resolve_child_backdrop(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
        backdrop: &RenderEffect,
        placement: ChildPlacement,
    ) -> Result<ResolvedComposite, String> {
        let scale = pass.scale;
        let ChildPlacement {
            z,
            visible,
            support,
            dest,
            translation,
            snap,
        } = placement;
        let padding = ((backdrop.input_padding() + backdrop.output_padding()) * scale).ceil();
        let capture_rect = visible
            .expand(padding)
            .intersect(pass.target_rect())
            .unwrap_or(visible)
            .snap_out();
        let capture = self.capture(pass, z, capture_rect, "Child Backdrop Capture")?;
        let layer_pixel_rect = [
            dest.x - capture_rect.x,
            dest.y - capture_rect.y,
            dest.width,
            dest.height,
        ];
        let rounded_mask = translation.and_then(|_| grid_rounded_mask(child, snap, scale));
        if let RenderEffect::Shader { shader } = backdrop
            && (rounded_mask.is_none() || shader.batched_source())
        {
            return Ok(ResolvedComposite {
                z_index: z,
                source: capture,
                content: SourceContent::Transient,
                dest: capture_rect.tuple(),
                scissor: Some(support.tuple()),
                kind: ResolvedCompositeKind::Shader {
                    shader: Rc::new(shader.clone()),
                    layer_pixel_rect,
                    source_region: None,
                    source_logical_size: None,
                    substrate_regions: [None; MAX_SUBSTRATES],
                    rounded_mask,
                    alpha: 1.0,
                },
            });
        }
        let reads = effect_reads(
            backdrop,
            blit_read_rect(support, capture_rect, false),
            dest,
            capture_rect,
            scale,
        );
        let result = self.apply_effect(
            &capture,
            backdrop,
            layer_pixel_rect,
            reads,
            "Child Backdrop Effect",
        )?;
        Ok(ResolvedComposite {
            z_index: z,
            source: result,
            content: SourceContent::Transient,
            dest: capture_rect.tuple(),
            scissor: Some(support.tuple()),
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: None,
            },
        })
    }

    fn pack_stage(
        &self,
        items: &[&PendingBackdrop<'_>],
    ) -> (
        AtlasPacker,
        Vec<Option<AtlasPlacement>>,
        Vec<Vec<PlannedSubstrate>>,
    ) {
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let mut packer = AtlasPacker::new(limit);
        let order: Vec<usize> = (0..items.len()).collect();
        let mut placements: Vec<Option<AtlasPlacement>> = vec![None; items.len()];
        for index in &order {
            let item = items[*index];
            if item.batched.is_none() {
                continue;
            }
            let (width, height) = item.capture_rect.pixel_size();
            placements[*index] = packer.place(width, height);
        }
        let mut substrates: Vec<Vec<PlannedSubstrate>> = vec![Vec::new(); items.len()];
        for index in order {
            let item = items[index];
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
                    atlas_slot,
                });
            }
        }
        (packer, placements, substrates)
    }

    fn plan_stage(&self, items: &[&PendingBackdrop<'_>], scale: f32) -> StageLayout {
        let (packer, placements, substrates) = self.pack_stage(items);
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let atlas_sizes: Vec<(u32, u32)> = packer
            .atlases
            .iter()
            .map(|atlas| atlas.padded_size(limit))
            .collect();
        let mut side_sizes = vec![(0, 0); atlas_sizes.len()];
        let mut side: Vec<SideSlots> = vec![SideSlots::default(); items.len()];
        for (atlas_index, side_size) in side_sizes.iter_mut().enumerate() {
            let members: Vec<usize> = (0..items.len())
                .filter(|index| {
                    placements[*index].is_some_and(|placement| placement.atlas == atlas_index)
                })
                .collect();
            let blurred: Vec<(usize, BlurSpec)> = members
                .iter()
                .filter_map(|index| Some((*index, items[*index].batched?.blur()?)))
                .collect();
            let mut side_packer = AtlasPacker::new(limit);
            let mut first_atlas = |width: u32, height: u32| {
                side_packer
                    .place(width, height)
                    .filter(|slot| slot.atlas == 0)
                    .map(|slot| (slot.x, slot.y, width, height))
            };
            for (index, blur) in blurred {
                let (width, height) = items[index].capture_rect.pixel_size();
                let (scaled_width, scaled_height) =
                    blur_scratch_size(blur.radius_x * scale, blur.radius_y * scale, width, height);
                side[index].blur = first_atlas(scaled_width, scaled_height);
            }
            for index in &members {
                for planned in &substrates[*index] {
                    if let Some(slot) = first_atlas(planned.size.0, planned.size.1) {
                        side[*index].substrates.push(slot);
                    }
                }
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

    fn stage_side_regions(
        &mut self,
        atlas: &Rc<OffscreenTarget>,
        items: &[&PendingBackdrop<'_>],
        view: &AtlasView<'_>,
        scale: f32,
    ) -> Result<Option<StageSideRegions>, String> {
        let members = &view.members;
        let blurred: Vec<(usize, BlurSpec)> = members
            .iter()
            .enumerate()
            .filter_map(|(member, (index, _))| Some((member, items[*index].batched?.blur()?)))
            .collect();
        let blurred = if self.renderer.ablation.blur {
            Vec::new()
        } else {
            blurred
        };
        if blurred.is_empty()
            && members
                .iter()
                .all(|(index, _)| view.substrates(*index).is_empty())
        {
            return Ok(None);
        }
        let mut slots = vec![None; members.len()];
        let mut regions = Vec::with_capacity(blurred.len());
        for (member, blur) in blurred {
            let (index, placement) = members[member];
            let (width, height) = items[index].capture_rect.pixel_size();
            let radius_x = blur.radius_x * scale;
            let radius_y = blur.radius_y * scale;
            let Some(scratch) = view.side(index).blur else {
                return Err("a blurred region outgrew the atlas that held it".into());
            };
            slots[member] = Some(scratch);
            regions.push(BlurRegion {
                source: (placement.x, placement.y, width, height),
                scratch,
                radius_x,
                radius_y,
                tile_mode: blur.tile_mode,
                read: member_read_texels(items[index], placement, scale),
            });
        }
        let mut averaged = Vec::new();
        let mut copies: Vec<(TexelRect, [u32; 2])> = Vec::new();
        let mut member_regions = vec![[None; MAX_SUBSTRATES]; members.len()];
        for (member, (index, placement)) in members.iter().enumerate() {
            let (source_width, source_height) = items[*index].capture_rect.pixel_size();
            let source = (placement.x, placement.y, source_width, source_height);
            for (order, planned) in view.substrates(*index).iter().enumerate() {
                if self.renderer.ablation.substrates {
                    member_regions[member][order] = Some(region_tuple(source));
                    continue;
                }
                let (width, height) = planned.size;
                let Some(scratch) = view.side(*index).substrates.get(order).copied() else {
                    return Err("a substrate outgrew the atlas that held it".into());
                };
                let read = member_read_texels(items[*index], *placement, scale);
                match planned.spec {
                    SubstrateSpec::Average { block } => averaged.push(SubstrateRegion {
                        source,
                        scratch,
                        block,
                        read,
                    }),
                    SubstrateSpec::Blur { radius_px } => regions.push(BlurRegion {
                        source,
                        scratch,
                        radius_x: radius_px,
                        radius_y: radius_px,
                        tile_mode: TileMode::Clamp,
                        read,
                    }),
                }
                member_regions[member][order] = Some(region_tuple(match planned.atlas_slot {
                    Some((x, y, _, _)) => {
                        copies.push((scratch, [x, y]));
                        (x, y, width, height)
                    }
                    None => scratch,
                }));
            }
        }
        if regions.is_empty() && averaged.is_empty() {
            return Ok(Some(StageSideRegions {
                result: Rc::clone(atlas),
                blurred: slots,
                substrates: member_regions,
            }));
        }
        let (width, height) = view.side_size();
        let scratch = self.acquire_transient("Backdrop Blur Scratch", width, height);
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
            &scratch,
            &result,
            AtlasSideWork {
                blurs: &regions,
                averages: &averaged,
            },
        );
        for ((x, y, width, height), dest_origin) in copies {
            self.recorder.copy_texture_region(TextureRegionCopy {
                source: &result,
                source_origin: [x, y],
                dest: atlas,
                dest_origin,
                size: [width, height],
            });
        }
        Ok(Some(StageSideRegions {
            result,
            blurred: slots,
            substrates: member_regions,
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
            && (item.rounded_mask.is_none() || shader.batched_source())
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
                    shader: Rc::new(shader.clone()),
                    layer_pixel_rect,
                    source_region: None,
                    source_logical_size: None,
                    substrate_regions: [None; MAX_SUBSTRATES],
                    rounded_mask: item.rounded_mask,
                    alpha: 1.0,
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

    /// Reads what every region's backdrop sees into its place in `texture`.
    /// A region of the layer's own page is copied texel for texel; what is
    /// below the region's z and not on the page (the ops since the last
    /// flush, the deferred ops and the pending composites) that reaches into
    /// it is then drawn over the copies in one pass loading them, scissored
    /// to each region's texels and recorded only when some region has such a
    /// fix-up. Under a parent's page, or when a region cannot be copied, the
    /// pass starts from transparent and draws the parent's page, the layer's
    /// own page and the fix-ups for every region.
    fn capture_regions(
        &mut self,
        pass: &mut LayerPass<'_>,
        regions: &[CaptureRegion],
        texture: &Rc<OffscreenTarget>,
        label: &'static str,
    ) -> Result<(), String> {
        let scale = pass.scale;
        let copied = self.copy_regions(pass, regions, texture);
        let beneath = pass.beneath;
        let bases: Vec<Vec<ResolvedComposite>> = regions
            .iter()
            .map(|region| {
                if copied {
                    return Vec::new();
                }
                beneath
                    .page
                    .as_ref()
                    .and_then(|base| base.under(region.rect))
                    .into_iter()
                    .chain(pass.page.blit(region.rect))
                    .collect()
            })
            .collect();
        let fixups: Vec<Vec<DrawOp>> = regions
            .iter()
            .map(|region| pass.ops_below(region.z))
            .collect();
        pass.pending.sort_by_key(|composite| composite.z_index);
        let target = PassTarget {
            view: &texture.view,
            width: texture.width,
            height: texture.height,
            offset: [0.0, 0.0],
        };
        let mut segments: Vec<PassSegment<'_>> = Vec::with_capacity(regions.len() * 2);
        for ((region, base), fixup) in regions.iter().zip(&bases).zip(&fixups) {
            let offset = [
                region.rect.x - region.origin[0],
                region.rect.y - region.origin[1],
            ];
            let (region_width, region_height) = region.rect.pixel_size();
            let scissor = Some((
                region.origin[0] as u32,
                region.origin[1] as u32,
                region_width,
                region_height,
            ));
            if !copied {
                segments.push(PassSegment {
                    scene: &self.empty_scene,
                    ops: &[],
                    composites: base,
                    offset,
                    scissor,
                    first_run_window: None,
                });
            }
            let own_end = pass
                .pending
                .partition_point(|composite| composite.z_index < region.z);
            let segment = PassSegment {
                scene: &pass.layer.scene,
                ops: fixup,
                composites: &pass.pending[..own_end],
                offset,
                scissor,
                first_run_window: None,
            };
            if !copied || segment_draws_anything(target, &segment, scale) {
                segments.push(segment);
            }
        }
        if copied && segments.is_empty() {
            return Ok(());
        }
        let load_op = if copied {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        };
        self.renderer
            .encode_pass(self.recorder, target, &segments, load_op, scale, label)?;
        if copied {
            self.renderer.frame_stats.record_capture_fixup_pass();
        }
        Ok(())
    }

    /// Copies every region of the layer's own page into its place in
    /// `texture` and reports whether it did: only when the layer reads no
    /// parent page, the two formats are copy-compatible and every region is
    /// a whole-texel rect inside the page.
    fn copy_regions(
        &mut self,
        pass: &LayerPass<'_>,
        regions: &[CaptureRegion],
        texture: &OffscreenTarget,
    ) -> bool {
        if pass.beneath.page.is_some() || !copy_compatible(&pass.page.texture, texture) {
            return false;
        }
        let copies: Option<Vec<TextureRegionCopy<'_>>> = regions
            .iter()
            .map(|region| pass.page.copy(region.rect, texture, region.origin))
            .collect();
        let Some(copies) = copies else {
            return false;
        };
        for copy in copies {
            self.recorder.copy_texture_region(copy);
        }
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
        };
        let target = PassTarget {
            view: &texture.view,
            width,
            height,
            offset: [source_rect.x, source_rect.y],
        };
        self.renderer.encode_pass(
            self.recorder,
            target,
            std::slice::from_ref(&segment),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            scale,
            "Effect Range Pass",
        )?;
        let layer_rect_device = DeviceRect::from_logical(rect, scale);
        let layer_pixel_rect = [
            layer_rect_device.x - source_rect.x,
            layer_rect_device.y - source_rect.y,
            layer_rect_device.width,
            layer_rect_device.height,
        ];
        let result = match &effect.effect {
            Some(render_effect) => self.apply_effect(
                &texture,
                render_effect,
                layer_pixel_rect,
                EffectReads::default(),
                "Effect Range Result",
            )?,
            None => texture,
        };
        Ok(Some(ResolvedComposite {
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
        }))
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
        Ok(dest)
    }

    fn resolve_child(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
    ) -> Result<(), String> {
        let scale = pass.scale;
        let z = child.z_index;
        let snap = child
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, scale))
            .unwrap_or_default();
        let grid = uniform_scale_translation(child.transform)
            .filter(|(uniform, _)| (uniform - child.surface_scale).abs() <= 1e-4)
            .map(|(_, translation)| Point::new(translation.x + snap.x, translation.y + snap.y));
        let translation = grid.filter(|_| (child.surface_scale - 1.0).abs() <= 1e-4);
        let (dest, visible_device) = child_device_placement(child, snap, scale, pass.target_rect());

        if !self.renderer.ablation.stages
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
                translation,
                snap,
            };
            let composite = self.resolve_child_backdrop(pass, child, backdrop, placement)?;
            pass.pending.push(composite);
        }

        let Some(visible) = visible_device else {
            return Ok(());
        };
        if let Some(composite) = self.shader_only_child(child, z, scale, visible, translation, snap)
        {
            pass.pending.push(composite);
            return Ok(());
        }
        let Some(surface) = self.render_child_surface(pass, child, z, grid, visible)? else {
            return Ok(());
        };
        if let Some(composite) =
            shader_tail_over_surface(child, &surface, translation, snap, z, scale, visible)
        {
            pass.pending.push(composite);
            return Ok(());
        }
        let source = match &child.effect {
            Some(effect) => {
                let layer_pixel_rect = layer_pixel_rect(child, surface.rect, surface.scale);
                CompositeSource {
                    texture: self.apply_effect(
                        &surface.source.texture,
                        effect,
                        layer_pixel_rect,
                        EffectReads::default(),
                        "Layer Effect",
                    )?,
                    content: surface.source.content.derived(&effect.render_hash()),
                }
            }
            None => surface.source.clone(),
        };
        let composite = match surface.grid_dest {
            Some(dest) => grid_child_composite(child, z, source, dest, snap, scale, visible),
            None => {
                let Some(composite) =
                    projected_child_composite(child, z, source, &surface, snap, scale, visible)
                else {
                    return Ok(());
                };
                composite
            }
        };
        pass.pending.push(composite);
        Ok(())
    }

    /// Renders the child's content into its own texture, from the layer
    /// cache when its pixels are a pure function of its content.
    /// A translated child that draws nothing itself and whose effect is one
    /// runtime shader composites as that shader drawn straight into the final
    /// pass over a shared transparent input, so it costs no surface pass.
    /// The shader must apply the child's clip and alpha itself, unless the
    /// child has neither.
    fn shader_only_child(
        &mut self,
        child: &ChildLayer,
        z: usize,
        scale: f32,
        visible: DeviceRect,
        translation: Option<Point>,
        snap: Point,
    ) -> Option<ResolvedComposite> {
        let translation = translation?;
        let Some(RenderEffect::Shader { shader }) = &child.effect else {
            return None;
        };
        let support =
            child_composite_support(child, shader.output_support(), snap, scale, visible)?;
        let content = &child.content;
        let draws_nothing = content.scene.draw_ops.is_empty()
            && content.children.is_empty()
            && content.scene.backdrop_layers.is_empty()
            && content.scene.effect_layers.is_empty();
        if !draws_nothing || !shader_tail_composites(child, shader) {
            return None;
        }
        let surface_logical = child_surface_rect(child, scale)?;
        let surface_rect = DeviceRect::from_logical(surface_logical, scale).snap_out();
        let (width, height) = surface_rect.pixel_size();
        if u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS {
            return None;
        }
        let source = self
            .renderer
            .transparent_source(self.recorder, width, height);
        let dest = DeviceRect {
            x: (surface_rect.x + translation.x * scale).round(),
            y: (surface_rect.y + translation.y * scale).round(),
            width: surface_rect.width,
            height: surface_rect.height,
        };
        Some(shader_tail_composite(
            child,
            shader,
            z,
            CompositeSource {
                texture: source,
                content: SourceContent::retained(&TRANSPARENT_SOURCE),
            },
            dest,
            layer_pixel_rect(child, surface_rect, scale),
            grid_rounded_mask(child, snap, scale),
            support,
        ))
    }

    /// Renders the child's content into its own texture. A child that reads
    /// its backdrop is never cached and renders every frame, so when it sits
    /// on the parent's pixel grid (a translation, or a uniform scale it
    /// renders at) and carries no effect of its own it renders the part of
    /// its surface `visible` shows, grown by what its glasses read past it
    /// (`backdrop_reach`): a card wider than the screen costs the screen,
    /// and every capture inside it follows.
    #[allow(clippy::too_many_arguments)]
    fn render_child_surface(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
        z: usize,
        grid: Option<Point>,
        visible: DeviceRect,
    ) -> Result<Option<SurfaceRender>, String> {
        let scale = pass.scale;
        let surface_scale = scale * child.surface_scale;
        let translated = (child.surface_scale - 1.0).abs() <= 1e-4;
        let Some(surface_logical) = child_surface_rect(child, surface_scale) else {
            return Ok(None);
        };
        let child_rect = DeviceRect::from_logical(surface_logical, surface_scale).snap_out();
        let grid_offset = grid.map(|grid| {
            let offset = Point::new(grid.x * scale, grid.y * scale);
            if translated {
                Point::new(offset.x.round(), offset.y.round())
            } else {
                offset
            }
        });
        let reads_backdrop = child.reads_backdrop();
        let (surface_rect, grid_dest, device_phase) = match grid_offset {
            Some(offset) => {
                let whole = child_rect.translated(offset).snap_out();
                let dest = if reads_backdrop && child.effect.is_none() {
                    let reach = (backdrop_reach(&child.content) * surface_scale).ceil() + 1.0;
                    visible
                        .expand(reach)
                        .intersect(whole)
                        .map_or(whole, DeviceRect::snap_out)
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
            return Ok(None);
        }
        let cache_key = (!reads_backdrop && child.cache_policy == CachePolicy::Auto).then(|| {
            LayerRasterCacheKey::source_content(
                child.node_id,
                child.content_hash,
                surface_logical,
                (width, height),
                ScaleBucket::from_scale(surface_scale),
                device_phase,
            )
        });
        if let Some(key) = cache_key
            && let Some(retained) = self.renderer.layer_cache.get(&key)
        {
            self.renderer
                .frame_stats
                .record_layer_cache_hit(&key, width, height);
            return Ok(Some(SurfaceRender {
                source: CompositeSource {
                    texture: retained.texture,
                    content: SourceContent::retained(&key),
                },
                rect: surface_rect,
                scale: surface_scale,
                grid_dest,
            }));
        }
        let cache_key = cache_key.filter(|_| self.renderer.layer_cache.fits(width, height));
        let texture = if cache_key.is_some() {
            Rc::new(self.renderer.acquire_retained_surface(width, height))
        } else {
            self.acquire_transient("Layer Surface", width, height)
        };
        let child_page = Page {
            texture: Rc::clone(&texture),
            offset: [surface_rect.x, surface_rect.y],
        };
        let child_beneath = if reads_backdrop {
            beneath_for_child(pass, child, z, grid_offset.filter(|_| translated))?
        } else {
            Beneath {
                base: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                page: None,
                described: Vec::new(),
            }
        };
        self.renderer.frame_stats.record_isolated_layer_render(
            width,
            height,
            child.node_id,
            surface_logical,
        );
        self.render_layer(
            &child.content,
            child_page,
            surface_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &child_beneath,
        )?;
        let retained = cache_key.filter(|key| {
            self.renderer
                .frame_stats
                .record_layer_cache_miss(key, width, height);
            self.renderer
                .layer_cache
                .insert(*key, Retained::surface(Rc::clone(&texture)), None)
        });
        Ok(Some(SurfaceRender {
            source: CompositeSource {
                texture,
                content: retained.map_or(SourceContent::Transient, |key| {
                    SourceContent::retained(&key)
                }),
            },
            rect: surface_rect,
            scale: surface_scale,
            grid_dest,
        }))
    }
}

/// How far, in a layer's logical units, any glass in it reads past the
/// pixels it shows: the largest input and output padding of its backdrop
/// effects and of its children's, at the children's surface scale.
fn backdrop_reach(layer: &LayerScene) -> f32 {
    let padding = |effect: &RenderEffect| effect.input_padding() + effect.output_padding();
    let own = layer
        .scene
        .backdrop_layers
        .iter()
        .map(|backdrop| padding(&backdrop.effect));
    let children = layer.children.iter().map(|child| {
        child.surface_scale
            * child
                .backdrop
                .as_ref()
                .map_or(0.0, padding)
                .max(backdrop_reach(&child.content))
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
    pass.pending.sort_by_key(|composite| composite.z_index);
    let scene = &pass.layer.scene;
    let drawn = &pass.drawn[..pass
        .drawn
        .partition_point(|composite| composite.z_index <= z)];
    let pending = &pass.pending[..pass
        .pending
        .partition_point(|composite| composite.z_index <= z)];
    let mut described: Vec<BeneathSegment<'a>> = pass
        .beneath
        .described
        .iter()
        .map(|segment| BeneathSegment {
            placement: [
                segment.placement[0] - shift.x,
                segment.placement[1] - shift.y,
            ],
            ..*segment
        })
        .collect();
    described.push(BeneathSegment {
        scene,
        z_end: z + 1,
        drawn,
        pending,
        excluded: &[],
        placement: [-shift.x, -shift.y],
    });
    Ok(Beneath {
        base: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        page,
        described,
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
    let dest_bounds =
        quad_bounds(child.transform.map_rect(child.local_bounds)).translate(snap.x, snap.y);
    let parent_rect = DeviceRect::from_logical(dest_bounds, scale)
        .expand(2.0)
        .snap_out()
        .intersect(pass.target_rect())
        .unwrap_or(DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
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
    let dest_quad = page_to_child_device.map_rect(Rect {
        x: parent_rect.x - pass.page.offset[0],
        y: parent_rect.y - pass.page.offset[1],
        width: parent_rect.width,
        height: parent_rect.height,
    });
    Ok(PagePlacement::Projected {
        dest_quad,
        inverse: child_device_to_page.matrix(),
    })
}

/// Splits an effect that ends in a runtime shader into the effects before
/// it and the shader itself, so the shader can draw straight into the final
/// pass instead of through one more texture. `None` when the effect does not
/// end in a shader.
fn shader_tail(effect: &RenderEffect) -> Option<(Option<&RenderEffect>, &RuntimeShader)> {
    match effect {
        RenderEffect::Shader { shader } => Some((None, shader)),
        RenderEffect::Chain { first, second } => match second.as_ref() {
            RenderEffect::Shader { shader } => Some((Some(first.as_ref()), shader)),
            _ => None,
        },
        _ => None,
    }
}

/// The rounded mask of a child composited at `translation`.
fn grid_rounded_mask(child: &ChildLayer, snap: Point, scale: f32) -> Option<RoundedCompositeMask> {
    child.rounded_clip.map(|clip| {
        rounded_mask(
            LayerRoundedClip {
                rect: quad_bounds(child.transform.map_rect(clip.rect)).translate(snap.x, snap.y),
                radii: clip.radii.map(|radius| radius * child.surface_scale),
            },
            Point::default(),
            scale,
        )
    })
}

fn rounded_mask(clip: LayerRoundedClip, snap: Point, scale: f32) -> RoundedCompositeMask {
    RoundedCompositeMask {
        rect: [
            (clip.rect.x + snap.x) * scale,
            (clip.rect.y + snap.y) * scale,
            clip.rect.width * scale,
            clip.rect.height * scale,
        ],
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

/// The ops below `z_end` outside the excluded ranges. Ops are pushed in z
/// order, so without exclusions this is a prefix of the list and borrows it.
fn filtered_ops<'a>(
    ops: &'a [DrawOp],
    z_end: usize,
    excluded: &[(usize, usize)],
) -> Cow<'a, [DrawOp]> {
    let end = ops.partition_point(|op| op.z_index < z_end);
    let prefix = &ops[..end];
    if excluded.is_empty() {
        return Cow::Borrowed(prefix);
    }
    Cow::Owned(
        prefix
            .iter()
            .filter(|op| {
                !excluded
                    .iter()
                    .any(|(start, end)| op.z_index >= *start && op.z_index < *end)
            })
            .copied()
            .collect(),
    )
}

/// The ops with z in `z_start..z_end` outside the excluded ranges.
fn filtered_ops_in_range(
    ops: &[DrawOp],
    z_start: usize,
    z_end: usize,
    excluded: &[(usize, usize)],
) -> Vec<DrawOp> {
    if z_end <= z_start {
        return Vec::new();
    }
    let start = ops.partition_point(|op| op.z_index < z_start);
    let end = ops.partition_point(|op| op.z_index < z_end);
    ops[start..end]
        .iter()
        .filter(|op| {
            !excluded
                .iter()
                .any(|(from, to)| op.z_index >= *from && op.z_index < *to)
        })
        .copied()
        .collect()
}

/// The logical rect a child's surface covers: everything its content draws,
/// clipped to its bounds when it clips, expanded for its effect's reach.
fn child_surface_rect(child: &ChildLayer, scale: f32) -> Option<Rect> {
    let mut bounds = union_rect(
        Some(child.local_bounds),
        scene_bounds(&child.content, scale * child.surface_scale),
    );
    if child.rounded_clip.is_some() || child.content.scene.draw_ops.is_empty() {
        bounds = Some(child.local_bounds);
    }
    let bounds = bounds?;
    let padding = child.effect.as_ref().map_or(0.0, |effect| {
        effect.input_padding() + effect.output_padding()
    });
    let rect = if padding > 0.0 {
        Rect {
            x: bounds.x - padding,
            y: bounds.y - padding,
            width: bounds.width + padding * 2.0,
            height: bounds.height + padding * 2.0,
        }
    } else {
        bounds
    };
    (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
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
        let child_bounds = child_surface_rect(child, scale)
            .map(|rect| quad_bounds(child.transform.map_rect(rect)))
            .unwrap_or(quad_bounds(child.transform.map_rect(child.local_bounds)));
        bounds = union_rect(bounds, clipped(child_bounds, child.clip));
    }
    bounds
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_backdrop_keeps_its_capture_and_records_the_part_of_it_inside_the_effects_output_support() {
        let mut shader = RuntimeShader::new("fn glass_fs() {}");
        shader.set_input_padding(2.0);
        shader.set_output_padding(3.0);
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let target = DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 400.0,
        };
        let plan = |shader: RuntimeShader| {
            let layer = BackdropLayer {
                node_id: None,
                rect,
                clip: None,
                rounded_clip: None,
                snap_anchor: None,
                effect: RenderEffect::Shader { shader },
                z_index: 0,
            };
            let planned =
                plan_backdrop(&layer, 0, 2.0, target).expect("the backdrop is on the target");
            (planned.visible, planned.capture_rect, planned.support)
        };
        let (whole_visible, whole_capture, whole_support) = plan(shader.clone());
        assert_eq!(whole_visible, DeviceRect::from_logical(rect, 2.0));
        assert_eq!(whole_capture, whole_visible.expand(10.0).snap_out());
        assert_eq!(whole_support, None);

        shader.set_output_support(Some(Rect {
            x: 30.0,
            y: 5.0,
            width: 20.0,
            height: 10.0,
        }));
        let (visible, capture_rect, support) = plan(shader);
        assert_eq!(visible, whole_visible);
        assert_eq!(capture_rect, whole_capture);
        assert_eq!(
            support,
            Some(DeviceRect::from_logical(
                Rect {
                    x: 40.0,
                    y: 25.0,
                    width: 20.0,
                    height: 10.0,
                },
                2.0,
            ))
        );
    }

    #[test]
    fn a_gate_admits_a_key_that_held_for_more_than_its_patience() {
        let key = gate_key(1);
        let mut gate = AdmissionGate::new(key);
        assert!(!gate.admits(), "a key seen once is only remembered");
        gate.observe(key);
        assert!(gate.admits(), "the second frame of a key admits it");
        gate.admitted();
        gate.hit(key);
        assert_eq!(gate.patience, 1);
        assert!(gate.end_frame(), "a gate seen this frame stays");
        assert!(!gate.end_frame(), "a gate not seen since goes");
    }

    #[test]
    fn a_cached_key_between_misses_breaks_the_other_keys_consecutive_run() {
        let first = gate_key(1);
        let other = gate_key(2);
        let mut gate = AdmissionGate::new(first);
        gate.observe(first);
        assert!(gate.admits());
        gate.admitted();
        gate.observe(other);
        assert!(!gate.admits());
        gate.observe(other);
        assert!(!gate.admits());
        gate.hit(first);
        gate.observe(other);
        assert!(
            !gate.admits(),
            "the other key has held for only one frame since the cache hit"
        );
    }
    fn gate_frame(gate: &mut AdmissionGate, key: LayerRasterCacheKey) -> bool {
        if gate.unread && gate.key == key {
            gate.hit(key);
            return false;
        }
        gate.observe(key);
        if gate.admits() {
            gate.admitted();
            return true;
        }
        false
    }

    fn admissions_over(gate: &mut AdmissionGate, holds: impl IntoIterator<Item = u32>) -> u32 {
        let mut admissions = 0;
        for (step, hold) in holds.into_iter().enumerate() {
            for _ in 0..hold {
                admissions += u32::from(gate_frame(gate, gate_key(step as u64 + 1)));
            }
        }
        admissions
    }

    #[test]
    fn a_gate_waits_twice_as_long_after_an_admission_nothing_read_back() {
        let mut gate = AdmissionGate::new(gate_key(0));
        assert_eq!(
            admissions_over(&mut gate, std::iter::repeat_n(2, 40)),
            1,
            "a key that never holds a third frame is admitted once"
        );
        assert_eq!(gate.patience, 2);
        let mut gate = AdmissionGate::new(gate_key(0));
        assert_eq!(
            admissions_over(&mut gate, std::iter::repeat_n(3, 12)),
            12,
            "a key that holds a third frame is read back once per admission"
        );
        assert_eq!(
            gate.patience, 1,
            "an admission read back does not double the patience"
        );
    }

    #[test]
    fn a_gate_never_waits_longer_than_the_cap() {
        let mut gate = AdmissionGate::new(gate_key(0));
        let admissions = admissions_over(&mut gate, [2, 3, 5, 9, 17, 17, 17]);
        assert_eq!(
            admissions, 7,
            "each hold one frame past the patience is admitted on its last frame"
        );
        assert_eq!(gate.patience, MAX_ADMISSION_PATIENCE);
    }

    fn gate_key(content: u64) -> LayerRasterCacheKey {
        LayerRasterCacheKey::backdrop_effect(
            None,
            content,
            0,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            (1, 1),
            ScaleBucket::from_scale(1.0),
        )
    }

    use super::*;
    use crate::scene::DrawOpKind;

    fn op(z_index: usize) -> DrawOp {
        DrawOp {
            z_index,
            kind: DrawOpKind::Run(0),
        }
    }

    #[test]
    fn an_inverted_op_range_is_empty_even_when_an_op_sits_at_its_end() {
        let ops = [op(1), op(3), op(3), op(5)];
        assert!(filtered_ops_in_range(&ops, 4, 3, &[]).is_empty());
        assert!(filtered_ops_in_range(&ops, 3, 3, &[]).is_empty());
        assert_eq!(
            filtered_ops_in_range(&ops, 3, 4, &[])
                .iter()
                .map(|op| op.z_index)
                .collect::<Vec<_>>(),
            [3, 3]
        );
    }

    #[test]
    fn subtracting_holes_partitions_a_rect_exactly() {
        let rect = DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let holes = [
            DeviceRect {
                x: 2.0,
                y: 2.0,
                width: 3.0,
                height: 3.0,
            },
            DeviceRect {
                x: 6.0,
                y: 6.0,
                width: 10.0,
                height: 10.0,
            },
        ];
        let parts = rect.subtract_all(&holes);
        let area: f32 = parts.iter().map(|part| part.width * part.height).sum();
        assert_eq!(area, 100.0 - 9.0 - 16.0);
        for (index, a) in parts.iter().enumerate() {
            assert!(holes.iter().all(|hole| a.intersect(*hole).is_none()));
            for b in &parts[index + 1..] {
                assert!(a.intersect(*b).is_none(), "parts overlap: {a:?} {b:?}");
            }
        }
    }
}

#[cfg(test)]
mod atlas_padding_tests {
    use super::{ATLAS_SIZE_STEP, padded_dimension};

    #[test]
    fn padded_dimensions_step_by_an_eighth_of_their_magnitude_and_never_exceed_the_limit() {
        assert_eq!(padded_dimension(1, 4096), ATLAS_SIZE_STEP);
        assert_eq!(padded_dimension(17, 4096), 32);
        assert_eq!(padded_dimension(300, 4096), 320);
        assert_eq!(padded_dimension(1080, 4096), 1280);
        assert_eq!(padded_dimension(2072, 4096), 2560);
        assert_eq!(padded_dimension(4000, 4096), 4096);
        assert_eq!(padded_dimension(2100, 3000), 2560);
        assert_eq!(padded_dimension(2900, 3000), 3000);
        assert_eq!(padded_dimension(24, 20), 20);
    }
}
