use std::{borrow::Cow, rc::Rc};

use cranpose_render_common::{
    graph::{CachePolicy, ProjectiveTransform, quad_bounds},
    raster_cache::{LayerRasterCacheKey, ScaleBucket},
};
use cranpose_ui_graphics::{BlendMode, Point, Rect, RenderEffect, RuntimeShader, TileMode};

use crate::{
    collect::{ChildLayer, LayerScene, uniform_scale_translation},
    draw_pass::{PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind},
    effect_renderer::{
        BlurRegion, CompositeSampleMode, EffectScratchTargetProvider, RoundedCompositeMask,
        blur_scratch_size,
    },
    frame_graph::{FrameCommandRecorder, FrameTextureDescriptor},
    geometry::snap_delta_for_anchor,
    offscreen::OffscreenTarget,
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

    fn from_tuple((x, y, width, height): (f32, f32, f32, f32)) -> Self {
        Self {
            x,
            y,
            width,
            height,
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

    fn pixel_size(self) -> (u32, u32) {
        (
            (self.width.ceil().max(1.0)) as u32,
            (self.height.ceil().max(1.0)) as u32,
        )
    }
}

/// What lies beneath a layer scene, expressed in that scene's device space:
/// the base its root chain clears to and every ancestor's ops below the
/// layer's position, so a capture inside the layer can rebuild the pixels a
/// backdrop reads without a target ever reading itself.
#[derive(Clone)]
struct Beneath<'a> {
    base: wgpu::LoadOp<wgpu::Color>,
    projected: Option<ProjectedBase>,
    segments: Vec<BeneathSegment<'a>>,
}

#[derive(Clone, Copy)]
struct BeneathSegment<'a> {
    scene: &'a CompositorScene,
    z_end: usize,
    composites: &'a [ResolvedComposite],
    excluded: &'a [(usize, usize)],
    placement: [f32; 2],
}

/// The parent's pixels under a transformed child, already captured in the
/// parent's device space, and the mapping from the child's device space back
/// into that capture.
#[derive(Clone)]
struct ProjectedBase {
    source: Rc<OffscreenTarget>,
    dest_quad: [[f32; 2]; 4],
    inverse: [[f32; 3]; 3],
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

impl BlurSpec {
    /// Texels to leave around a region so no blur tap reads a neighbour.
    fn gap(self, scale: f32) -> u32 {
        (self.radius_x.max(self.radius_y) * scale).ceil().max(1.0) as u32
    }
}

/// A backdrop effect the renderer can resolve from a shared atlas: a blur,
/// a shader that reads its source region, or the two chained.
#[derive(Clone, Copy)]
enum BatchedEffect<'a> {
    Blur(BlurSpec),
    Shader(&'a RuntimeShader),
    BlurThenShader(BlurSpec, &'a RuntimeShader),
}

impl BatchedEffect<'_> {
    fn blur(self) -> Option<BlurSpec> {
        match self {
            Self::Blur(blur) | Self::BlurThenShader(blur, _) => Some(blur),
            Self::Shader(_) => None,
        }
    }
}

/// How much a shader drawn straight into the final pass may be re-run by
/// the captures above it before resolving it into a texture once is
/// cheaper: the intersecting capture area as a fraction of its own.
const TAIL_RECAPTURE_LIMIT: f32 = 0.5;

/// The device rects every backdrop capture of a layer scene reads, with the
/// z it reads at. A runtime shader composited straight into the final pass
/// is re-run inside each of those captures above it, so a shader that many
/// captures read (a page-wide shader under a list of glass cards) resolves
/// into a texture once instead.
struct CaptureReaders {
    rects: Vec<(usize, DeviceRect)>,
}

impl CaptureReaders {
    fn of(layer: &LayerScene, scale: f32, target_rect: DeviceRect) -> Self {
        let mut rects: Vec<(usize, DeviceRect)> = layer
            .scene
            .backdrop_layers
            .iter()
            .filter_map(|backdrop| {
                plan_backdrop(backdrop, backdrop.z_index, scale, target_rect)
                    .map(|pending| (pending.z, pending.capture_rect))
            })
            .collect();
        for child in &layer.children {
            if child.reads_backdrop() {
                let bounds = quad_bounds(child.transform.map_rect(child.local_bounds));
                rects.push((child.z_index, DeviceRect::from_logical(bounds, scale)));
            }
        }
        Self { rects }
    }

    /// Whether any capture above `z` reads pixels of `visible`.
    fn reads(&self, z: usize, visible: DeviceRect) -> bool {
        self.rects
            .iter()
            .any(|(reader_z, rect)| *reader_z > z && rect.intersect(visible).is_some())
    }

    fn tail_allowed(&self, z: usize, visible: DeviceRect) -> bool {
        let own = visible.width * visible.height;
        if own <= 0.0 {
            return true;
        }
        let recaptured: f32 = self
            .rects
            .iter()
            .filter(|(reader_z, _)| *reader_z > z)
            .filter_map(|(_, rect)| rect.intersect(visible))
            .map(|overlap| overlap.width * overlap.height)
            .sum();
        recaptured <= own * TAIL_RECAPTURE_LIMIT
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
    capture_rect: DeviceRect,
    layer_rect: DeviceRect,
    visible: DeviceRect,
    effect: &'a RenderEffect,
    rounded_mask: Option<RoundedCompositeMask>,
    batched: Option<BatchedEffect<'a>>,
    stage: usize,
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
    let padding = (backdrop.effect.input_padding() + backdrop.effect.output_padding()) * scale;
    let capture_rect = visible
        .expand(padding.ceil())
        .intersect(target_rect)
        .unwrap_or(visible)
        .snap_out();
    Some(PendingBackdrop {
        z,
        capture_rect,
        layer_rect: DeviceRect::from_logical(rect, scale),
        visible,
        effect: &backdrop.effect,
        rounded_mask: backdrop
            .rounded_clip
            .map(|clip| rounded_mask(clip, snap, scale)),
        batched: batched_effect(&backdrop.effect),
        stage: 0,
    })
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
    source: Rc<OffscreenTarget>,
    dest: DeviceRect,
    layer_pixel_rect: [f32; 4],
    rounded_mask: Option<RoundedCompositeMask>,
    visible: DeviceRect,
) -> ResolvedComposite {
    ResolvedComposite {
        z_index: z,
        source,
        dest: dest.tuple(),
        scissor: Some(visible.tuple()),
        kind: ResolvedCompositeKind::Shader {
            shader: Rc::new(shader.clone()),
            layer_pixel_rect,
            source_region: None,
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
            Rc::clone(&surface.texture),
            dest,
            layer_pixel_rect(child, surface.rect, surface.scale),
            grid_rounded_mask(child, snap, scale),
            visible,
        )
    })
}

/// A child whose surface lies on the parent's pixel grid, blitted one to one
/// at `dest`.
fn grid_child_composite(
    child: &ChildLayer,
    z: usize,
    texture: Rc<OffscreenTarget>,
    dest: DeviceRect,
    snap: Point,
    scale: f32,
    visible: DeviceRect,
) -> ResolvedComposite {
    ResolvedComposite {
        z_index: z,
        source: texture,
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
    texture: Rc<OffscreenTarget>,
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
        source: texture,
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

/// The composite of every member of one capture atlas: a blur reads its
/// blurred region, a shader reads its capture region.
fn stage_composites(
    texture: &Rc<OffscreenTarget>,
    blurred: Option<&BlurredRegions>,
    items: &[&PendingBackdrop<'_>],
    members: &[(usize, AtlasPlacement)],
) -> Vec<ResolvedComposite> {
    members
        .iter()
        .enumerate()
        .map(|(member, (index, placement))| {
            let item = items[*index];
            let (region_width, region_height) = item.capture_rect.pixel_size();
            let (source, origin) = match blurred.and_then(|blurred| blurred.slot(member)) {
                Some((blurred, origin)) => (blurred, origin),
                None => (texture, (placement.x, placement.y)),
            };
            let region = (
                origin.0 as f32,
                origin.1 as f32,
                region_width as f32,
                region_height as f32,
            );
            let kind = match item.batched.expect("packed items are batched") {
                BatchedEffect::Blur(_) => ResolvedCompositeKind::Blit {
                    alpha: 1.0,
                    blend_mode: BlendMode::SrcOver,
                    rounded_mask: item.rounded_mask,
                    sample_mode: CompositeSampleMode::Nearest,
                    source_viewport: Some(region),
                },
                BatchedEffect::Shader(shader) | BatchedEffect::BlurThenShader(_, shader) => {
                    ResolvedCompositeKind::Shader {
                        shader: Rc::new(shader.clone()),
                        layer_pixel_rect: item.layer_pixel_rect(),
                        source_region: Some(region),
                        rounded_mask: item.rounded_mask,
                        alpha: 1.0,
                    }
                }
            };
            ResolvedComposite {
                z_index: item.z,
                source: Rc::clone(source),
                dest: item.capture_rect.tuple(),
                scissor: Some(item.visible.tuple()),
                kind,
            }
        })
        .collect()
}

/// The blurred regions of one capture atlas: the texture holding them and,
/// per atlas member, where its blurred region starts.
struct BlurredRegions {
    result: Rc<OffscreenTarget>,
    slots: Vec<Option<(u32, u32)>>,
}

impl BlurredRegions {
    fn slot(&self, member: usize) -> Option<(&Rc<OffscreenTarget>, (u32, u32))> {
        self.slots[member].map(|origin| (&self.result, origin))
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
    fn padded_size(&self) -> (u32, u32) {
        let round = |value: u32| value.div_ceil(ATLAS_SIZE_STEP).max(1) * ATLAS_SIZE_STEP;
        (round(self.width), round(self.height))
    }
}

/// Shelf packing of regions into as few atlases as the dimension limit
/// allows; every region is surrounded by its gap.
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

    fn place(&mut self, width: u32, height: u32, gap: u32) -> Option<AtlasPlacement> {
        let padded_width = width.checked_add(gap.checked_mul(2)?)?;
        let padded_height = height.checked_add(gap.checked_mul(2)?)?;
        if padded_width > self.limit || padded_height > self.limit {
            return None;
        }
        for (atlas_index, atlas) in self.atlases.iter_mut().enumerate() {
            for shelf in &mut atlas.shelves {
                if shelf.height >= padded_height && shelf.x + padded_width <= self.limit {
                    let placement = AtlasPlacement {
                        atlas: atlas_index,
                        x: shelf.x + gap,
                        y: shelf.y + gap,
                    };
                    shelf.x += padded_width;
                    atlas.width = atlas.width.max(shelf.x);
                    return Some(placement);
                }
            }
            if atlas.height + padded_height <= self.limit {
                let placement = AtlasPlacement {
                    atlas: atlas_index,
                    x: gap,
                    y: atlas.height + gap,
                };
                atlas.shelves.push(Shelf {
                    y: atlas.height,
                    height: padded_height,
                    x: padded_width,
                });
                atlas.height += padded_height;
                atlas.width = atlas.width.max(padded_width);
                return Some(placement);
            }
        }
        self.atlases.push(Atlas {
            width: padded_width,
            height: padded_height,
            shelves: vec![Shelf {
                y: 0,
                height: padded_height,
                x: padded_width,
            }],
        });
        Some(AtlasPlacement {
            atlas: self.atlases.len() - 1,
            x: gap,
            y: gap,
        })
    }
}

pub(crate) struct FrameExecutor<'r, 'c, C: FrameCommandRecorder> {
    renderer: &'r mut GpuRenderer,
    recorder: &'c mut C,
    transients: Vec<(FrameTextureDescriptor, Rc<OffscreenTarget>)>,
    empty_scene: CompositorScene,
    depth: usize,
}

struct SurfaceRender {
    texture: Rc<OffscreenTarget>,
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
        Self {
            renderer,
            recorder,
            transients: Vec::new(),
            empty_scene: CompositorScene::new(),
            depth: 0,
        }
    }

    /// Renders the root scene into the frame's target, then the overlay on
    /// top of it.
    pub(crate) fn render_frame(
        mut self,
        root: &LayerScene,
        overlay: Option<&LayerScene>,
        target: PassTarget<'_>,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        let beneath = Beneath {
            base: load_op,
            projected: None,
            segments: Vec::new(),
        };
        self.render_layer(root, target, root_scale, load_op, &beneath)?;
        if let Some(overlay) = overlay {
            let beneath = Beneath {
                base: wgpu::LoadOp::Load,
                projected: None,
                segments: Vec::new(),
            };
            self.render_layer(overlay, target, root_scale, wgpu::LoadOp::Load, &beneath)?;
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

    fn target_rect(target: PassTarget<'_>) -> DeviceRect {
        DeviceRect {
            x: target.offset[0],
            y: target.offset[1],
            width: target.width as f32,
            height: target.height as f32,
        }
    }

    /// Resolves every backdrop, isolated child, effect range and blurred
    /// shadow of the layer into textures, then draws the layer's ops and those
    /// textures into the target in one pass. Backdrop effects queue into
    /// stages (`ResolveStages`) and resolve together: one capture pass and one
    /// blur pair per stage rather than per effect.
    fn render_layer(
        &mut self,
        layer: &LayerScene,
        target: PassTarget<'_>,
        scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        beneath: &Beneath<'_>,
    ) -> Result<(), String> {
        if self.depth >= MAX_RESOLVE_DEPTH {
            return Err("layer nesting exceeds the resolve depth limit".to_string());
        }
        self.depth += 1;
        let result = self.render_layer_inner(layer, target, scale, load_op, beneath);
        self.depth -= 1;
        result
    }

    fn render_layer_inner(
        &mut self,
        layer: &LayerScene,
        target: PassTarget<'_>,
        scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        beneath: &Beneath<'_>,
    ) -> Result<(), String> {
        let scene = &layer.scene;
        let mut resolved: Vec<ResolvedComposite> = Vec::new();
        let mut excluded: Vec<(usize, usize)> = Vec::new();
        let mut stages = ResolveStages::default();
        let target_rect = Self::target_rect(target);
        let readers = CaptureReaders::of(layer, scale, target_rect);

        for (z, event) in layer_events(layer) {
            match event {
                Event::Backdrop(index) => {
                    let backdrop = &scene.backdrop_layers[index];
                    if let Some(pending) = plan_backdrop(backdrop, z, scale, target_rect) {
                        stages.push(pending);
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
                        &mut resolved,
                    );
                }
                Event::Effect(index) => {
                    self.run_stages(layer, scale, beneath, &readers, &mut stages, &mut resolved)?;
                    let effect = &scene.effect_layers[index];
                    excluded.push((effect.z_start, effect.z_end));
                    if let Some(composite) =
                        self.resolve_effect_range(scene, effect, scale, target_rect, &resolved)?
                    {
                        resolved.push(composite);
                    }
                }
                Event::Child(index) => {
                    let child = &layer.children[index];
                    if child.reads_backdrop() {
                        self.run_stages(
                            layer,
                            scale,
                            beneath,
                            &readers,
                            &mut stages,
                            &mut resolved,
                        )?;
                    }
                    self.resolve_child(
                        layer,
                        child,
                        scale,
                        target_rect,
                        beneath,
                        &readers,
                        &mut resolved,
                    )?;
                }
            }
        }
        self.run_stages(layer, scale, beneath, &readers, &mut stages, &mut resolved)?;

        let ops = filtered_ops(&scene.draw_ops, usize::MAX, &excluded);
        let segment = PassSegment {
            scene,
            ops: &ops,
            composites: &resolved,
            offset: target.offset,
            scissor: None,
        };
        self.renderer.encode_pass(
            self.recorder,
            target,
            std::slice::from_ref(&segment),
            load_op,
            scale,
            "Layer Pass",
        )?;
        Ok(())
    }

    /// Resolves the queued backdrop effects stage by stage. Every stage
    /// packs the captures it can share into atlases, renders each atlas in
    /// one capture pass, blurs all its blurred regions in one pass pair, and
    /// hands the results to the final pass as composites reading their
    /// region; the rest of the stage resolves one effect at a time.
    fn run_stages(
        &mut self,
        layer: &LayerScene,
        scale: f32,
        beneath: &Beneath<'_>,
        readers: &CaptureReaders,
        stages: &mut ResolveStages<'_>,
        resolved: &mut Vec<ResolvedComposite>,
    ) -> Result<(), String> {
        let mut pending = std::mem::take(&mut stages.pending);
        if pending.is_empty() {
            return Ok(());
        }
        pending.sort_by_key(|item| (item.stage, item.z));
        let stage_count = pending.last().map_or(0, |item| item.stage + 1);
        for stage in 0..stage_count {
            let items: Vec<&PendingBackdrop<'_>> =
                pending.iter().filter(|item| item.stage == stage).collect();
            let mut outputs = self.run_stage(layer, scale, beneath, resolved, &items)?;
            self.resolve_read_tails(&mut outputs, readers, scale)?;
            resolved.extend(outputs);
            resolved.sort_by_key(|composite| composite.z_index);
        }
        Ok(())
    }

    fn run_stage(
        &mut self,
        layer: &LayerScene,
        scale: f32,
        beneath: &Beneath<'_>,
        resolved: &[ResolvedComposite],
        items: &[&PendingBackdrop<'_>],
    ) -> Result<Vec<ResolvedComposite>, String> {
        let mut outputs = Vec::with_capacity(items.len());
        let (packer, placements) = self.pack_stage(items, scale);
        for (atlas_index, atlas) in packer.atlases.iter().enumerate() {
            let members: Vec<(usize, AtlasPlacement)> = placements
                .iter()
                .enumerate()
                .filter_map(|(index, placement)| {
                    placement
                        .filter(|placement| placement.atlas == atlas_index)
                        .map(|placement| (index, placement))
                })
                .collect();
            let (width, height) = atlas.padded_size();
            let texture = self.acquire_transient("Backdrop Capture Atlas", width, height);
            let regions: Vec<CaptureRegion> = members
                .iter()
                .map(|(index, placement)| CaptureRegion {
                    z: items[*index].z,
                    rect: items[*index].capture_rect,
                    origin: [placement.x as f32, placement.y as f32],
                })
                .collect();
            let target = PassTarget {
                view: &texture.view,
                width,
                height,
                offset: [0.0, 0.0],
            };
            self.capture_regions(
                layer,
                &regions,
                target,
                scale,
                beneath,
                resolved,
                "Backdrop Capture Atlas Pass",
            )?;

            let blurred = self.blur_regions(&texture, items, &members, scale)?;
            outputs.extend(stage_composites(
                &texture,
                blurred.as_ref(),
                items,
                &members,
            ));
        }

        for (index, item) in items.iter().enumerate() {
            if placements[index].is_some() {
                continue;
            }
            let capture = self.capture(
                layer,
                item.z,
                scale,
                item.capture_rect,
                beneath,
                resolved,
                "Backdrop Capture",
            )?;
            outputs.push(self.resolve_captured_backdrop(item, capture)?);
        }
        Ok(outputs)
    }

    /// A shader tail of a stage that a later capture reads would be shaded
    /// again inside every such capture; this draws every read tail of the
    /// stage once, packed into one texture in one pass, and turns each into a
    /// blit of its pixels for the captures and the final pass alike.
    fn resolve_read_tails(
        &mut self,
        outputs: &mut [ResolvedComposite],
        readers: &CaptureReaders,
        scale: f32,
    ) -> Result<(), String> {
        let mut read: Vec<(usize, DeviceRect)> = outputs
            .iter()
            .enumerate()
            .filter(|(_, composite)| matches!(composite.kind, ResolvedCompositeKind::Shader { .. }))
            .filter_map(|(index, composite)| {
                let visible = DeviceRect::from_tuple(composite.scissor?).snap_out();
                readers
                    .reads(composite.z_index, visible)
                    .then_some((index, visible))
            })
            .collect();
        if read.is_empty() {
            return Ok(());
        }
        read.sort_by_key(|(_, visible)| std::cmp::Reverse(visible.pixel_size().1));
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let mut packer = AtlasPacker::new(limit);
        let placed: Vec<(usize, AtlasPlacement, DeviceRect)> = read
            .into_iter()
            .filter_map(|(index, visible)| {
                let (width, height) = visible.pixel_size();
                packer
                    .place(width, height, 1)
                    .map(|placement| (index, placement, visible))
            })
            .collect();
        for (atlas_index, atlas) in packer.atlases.iter().enumerate() {
            let members: Vec<&(usize, AtlasPlacement, DeviceRect)> = placed
                .iter()
                .filter(|(_, placement, _)| placement.atlas == atlas_index)
                .collect();
            let (width, height) = atlas.padded_size();
            let texture = self.acquire_transient("Backdrop Resolve", width, height);
            let segments: Vec<PassSegment<'_>> = members
                .iter()
                .map(|(index, placement, visible)| {
                    let (region_width, region_height) = visible.pixel_size();
                    PassSegment {
                        scene: &self.empty_scene,
                        ops: &[],
                        composites: std::slice::from_ref(&outputs[*index]),
                        offset: [
                            visible.x - placement.x as f32,
                            visible.y - placement.y as f32,
                        ],
                        scissor: Some((placement.x, placement.y, region_width, region_height)),
                    }
                })
                .collect();
            let target = PassTarget {
                view: &texture.view,
                width,
                height,
                offset: [0.0, 0.0],
            };
            self.renderer.encode_pass(
                self.recorder,
                target,
                &segments,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                scale,
                "Backdrop Resolve Pass",
            )?;
            for (index, placement, visible) in members {
                let (region_width, region_height) = visible.pixel_size();
                outputs[*index] = ResolvedComposite {
                    z_index: outputs[*index].z_index,
                    source: Rc::clone(&texture),
                    dest: visible.tuple(),
                    scissor: Some(visible.tuple()),
                    kind: ResolvedCompositeKind::Blit {
                        alpha: 1.0,
                        blend_mode: BlendMode::SrcOver,
                        rounded_mask: None,
                        sample_mode: CompositeSampleMode::Nearest,
                        source_viewport: Some((
                            placement.x as f32,
                            placement.y as f32,
                            region_width as f32,
                            region_height as f32,
                        )),
                    },
                };
            }
        }
        Ok(())
    }

    /// Resolves the backdrop a child reads: captures the ops beneath it and
    /// either hands the capture to a shader tail in the final pass or applies
    /// the effect into a texture blitted through the child's mask.
    #[allow(clippy::too_many_arguments)]
    fn resolve_child_backdrop(
        &mut self,
        layer: &LayerScene,
        child: &ChildLayer,
        backdrop: &RenderEffect,
        placement: ChildPlacement,
        scale: f32,
        target_rect: DeviceRect,
        beneath: &Beneath<'_>,
        resolved: &[ResolvedComposite],
    ) -> Result<ResolvedComposite, String> {
        let ChildPlacement {
            z,
            visible,
            dest,
            translation,
            snap,
        } = placement;
        let padding = ((backdrop.input_padding() + backdrop.output_padding()) * scale).ceil();
        let capture_rect = visible
            .expand(padding)
            .intersect(target_rect)
            .unwrap_or(visible)
            .snap_out();
        let capture = self.capture(
            layer,
            z,
            scale,
            capture_rect,
            beneath,
            resolved,
            "Child Backdrop Capture",
        )?;
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
                dest: capture_rect.tuple(),
                scissor: Some(visible.tuple()),
                kind: ResolvedCompositeKind::Shader {
                    shader: Rc::new(shader.clone()),
                    layer_pixel_rect,
                    source_region: None,
                    rounded_mask,
                    alpha: 1.0,
                },
            });
        }
        let result = self.apply_effect(
            &capture,
            backdrop,
            layer_pixel_rect,
            "Child Backdrop Effect",
        )?;
        Ok(ResolvedComposite {
            z_index: z,
            source: result,
            dest: capture_rect.tuple(),
            scissor: Some(visible.tuple()),
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: None,
            },
        })
    }

    /// Packs every batched item of a stage into as few atlases as the
    /// dimension limit allows, tallest first; an unbatched item gets no
    /// placement and captures alone.
    fn pack_stage(
        &self,
        items: &[&PendingBackdrop<'_>],
        scale: f32,
    ) -> (AtlasPacker, Vec<Option<AtlasPlacement>>) {
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let mut packer = AtlasPacker::new(limit);
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by_key(|index| std::cmp::Reverse(items[*index].capture_rect.pixel_size().1));
        let mut placements: Vec<Option<AtlasPlacement>> = vec![None; items.len()];
        for index in order {
            let item = items[index];
            let Some(batched) = item.batched else {
                continue;
            };
            let (width, height) = item.capture_rect.pixel_size();
            let gap = batched.blur().map_or(1, |blur| blur.gap(scale));
            placements[index] = packer.place(width, height, gap);
        }
        (packer, placements)
    }

    /// Runs the blur pass pair over the blurred members of one capture atlas.
    /// The scratch holds each region downscaled and the result holds each
    /// region at full size, packed by height so both stay as small as the
    /// regions themselves; a composite of a blurred member reads the result.
    fn blur_regions(
        &mut self,
        atlas: &Rc<OffscreenTarget>,
        items: &[&PendingBackdrop<'_>],
        members: &[(usize, AtlasPlacement)],
        scale: f32,
    ) -> Result<Option<BlurredRegions>, String> {
        let mut blurred: Vec<(usize, BlurSpec)> = members
            .iter()
            .enumerate()
            .filter_map(|(member, (index, _))| Some((member, items[*index].batched?.blur()?)))
            .collect();
        if blurred.is_empty() {
            return Ok(None);
        }
        blurred.sort_by_key(|(member, _)| {
            std::cmp::Reverse(items[members[*member].0].capture_rect.pixel_size().1)
        });
        let limit = self.renderer.max_texture_dim().min(MAX_ATLAS_DIM);
        let mut scratch_packer = AtlasPacker::new(limit);
        let mut result_packer = AtlasPacker::new(limit);
        let mut slots = vec![None; members.len()];
        let mut regions = Vec::with_capacity(blurred.len());
        for (member, blur) in blurred {
            let (index, placement) = members[member];
            let (width, height) = items[index].capture_rect.pixel_size();
            let radius_x = blur.radius_x * scale;
            let radius_y = blur.radius_y * scale;
            let (scaled_width, scaled_height) =
                blur_scratch_size(radius_x, radius_y, width, height);
            let (Some(scratch), Some(result)) = (
                scratch_packer.place(scaled_width, scaled_height, 1),
                result_packer.place(width, height, 1),
            ) else {
                return Err("a blurred region outgrew the atlas that held it".into());
            };
            if scratch.atlas != 0 || result.atlas != 0 {
                return Err("blurred regions of one atlas spilled into a second".into());
            }
            slots[member] = Some((result.x, result.y));
            regions.push(BlurRegion {
                source: (placement.x, placement.y, width, height),
                scratch: (scratch.x, scratch.y, scaled_width, scaled_height),
                dest: (result.x, result.y),
                radius_x,
                radius_y,
                tile_mode: blur.tile_mode,
            });
        }
        let (scratch_width, scratch_height) = scratch_packer.atlases[0].padded_size();
        let (result_width, result_height) = result_packer.atlases[0].padded_size();
        let scratch =
            self.acquire_transient("Backdrop Blur Scratch", scratch_width, scratch_height);
        let result = self.acquire_transient("Backdrop Blur Result", result_width, result_height);
        let device = self.renderer.device.clone();
        self.renderer.effect_renderer.encode_blur_atlas_passes(
            self.recorder,
            &device,
            atlas,
            &scratch,
            &result,
            &regions,
        );
        Ok(Some(BlurredRegions { result, slots }))
    }

    /// Resolves one backdrop effect from its own capture texture: a shader
    /// tail draws in the final pass, anything else is applied into a texture
    /// and blitted with the effect's mask.
    fn resolve_captured_backdrop(
        &mut self,
        item: &PendingBackdrop<'_>,
        capture: Rc<OffscreenTarget>,
    ) -> Result<ResolvedComposite, String> {
        let layer_pixel_rect = item.layer_pixel_rect();
        if let Some((pre_shader, shader)) = shader_tail(item.effect)
            && (item.rounded_mask.is_none() || shader.batched_source())
        {
            let source = match pre_shader {
                Some(effect) => {
                    self.apply_effect(&capture, effect, layer_pixel_rect, "Backdrop Effect")?
                }
                None => capture,
            };
            return Ok(ResolvedComposite {
                z_index: item.z,
                source,
                dest: item.capture_rect.tuple(),
                scissor: Some(item.visible.tuple()),
                kind: ResolvedCompositeKind::Shader {
                    shader: Rc::new(shader.clone()),
                    layer_pixel_rect,
                    source_region: None,
                    rounded_mask: item.rounded_mask,
                    alpha: 1.0,
                },
            });
        }
        let result =
            self.apply_effect(&capture, item.effect, layer_pixel_rect, "Backdrop Effect")?;
        Ok(ResolvedComposite {
            z_index: item.z,
            source: result,
            dest: item.capture_rect.tuple(),
            scissor: Some(item.visible.tuple()),
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask: item.rounded_mask,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: None,
            },
        })
    }

    /// Renders what a backdrop at `z` in `layer` reads within `rect` into a
    /// texture that size: the root chain's base, every ancestor's ops beneath
    /// the layer, and the layer's own ops and resolved composites below `z`.
    #[allow(clippy::too_many_arguments)]
    fn capture(
        &mut self,
        layer: &LayerScene,
        z: usize,
        scale: f32,
        rect: DeviceRect,
        beneath: &Beneath<'_>,
        resolved: &[ResolvedComposite],
        label: &'static str,
    ) -> Result<Rc<OffscreenTarget>, String> {
        let (width, height) = rect.pixel_size();
        let texture = self.acquire_transient(label, width, height);
        let target = PassTarget {
            view: &texture.view,
            width,
            height,
            offset: [0.0, 0.0],
        };
        let region = CaptureRegion {
            z,
            rect,
            origin: [0.0, 0.0],
        };
        self.capture_regions(
            layer,
            std::slice::from_ref(&region),
            target,
            scale,
            beneath,
            resolved,
            "Backdrop Capture Pass",
        )?;
        Ok(texture)
    }

    /// Renders every region's beneath into its place in the target in one
    /// pass: each region draws the ancestors' ops and the layer's ops and
    /// composites below its z, scissored to its own texels.
    #[allow(clippy::too_many_arguments)]
    fn capture_regions(
        &mut self,
        layer: &LayerScene,
        regions: &[CaptureRegion],
        target: PassTarget<'_>,
        scale: f32,
        beneath: &Beneath<'_>,
        resolved: &[ResolvedComposite],
        label: &'static str,
    ) -> Result<(), String> {
        let projected: Vec<[ResolvedComposite; 1]> = beneath
            .projected
            .iter()
            .map(|projected| {
                [ResolvedComposite {
                    z_index: 0,
                    source: Rc::clone(&projected.source),
                    dest: quad_device_bounds(projected.dest_quad).tuple(),
                    scissor: None,
                    kind: ResolvedCompositeKind::Projective {
                        dest_quad: projected.dest_quad,
                        inverse: projected.inverse,
                        alpha: 1.0,
                        blend_mode: BlendMode::SrcOver,
                        sample_mode: CompositeSampleMode::Linear,
                    },
                }]
            })
            .collect();
        let beneath_ops: Vec<Cow<'_, [DrawOp]>> = beneath
            .segments
            .iter()
            .map(|segment| filtered_ops(&segment.scene.draw_ops, segment.z_end, segment.excluded))
            .collect();
        let own_ops: Vec<Cow<'_, [DrawOp]>> = regions
            .iter()
            .map(|region| filtered_ops(&layer.scene.draw_ops, region.z, &[]))
            .collect();
        let mut segments: Vec<PassSegment<'_>> =
            Vec::with_capacity(regions.len() * (beneath.segments.len() + 2));
        for (region, ops) in regions.iter().zip(&own_ops) {
            let shift = [
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
            for composites in &projected {
                segments.push(PassSegment {
                    scene: &self.empty_scene,
                    ops: &[],
                    composites,
                    offset: shift,
                    scissor,
                });
            }
            for (segment, beneath_ops) in beneath.segments.iter().zip(&beneath_ops) {
                segments.push(PassSegment {
                    scene: segment.scene,
                    ops: beneath_ops,
                    composites: segment.composites,
                    offset: [
                        shift[0] - segment.placement[0],
                        shift[1] - segment.placement[1],
                    ],
                    scissor,
                });
            }
            let own_end = resolved.partition_point(|composite| composite.z_index < region.z);
            segments.push(PassSegment {
                scene: &layer.scene,
                ops,
                composites: &resolved[..own_end],
                offset: shift,
                scissor,
            });
        }
        let base = match beneath.projected {
            Some(_) => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            None => beneath.base,
        };
        self.renderer
            .encode_pass(self.recorder, target, &segments, base, scale, label)?;
        Ok(())
    }

    fn resolve_effect_range(
        &mut self,
        scene: &CompositorScene,
        effect: &EffectLayer,
        scale: f32,
        target_rect: DeviceRect,
        resolved: &[ResolvedComposite],
    ) -> Result<Option<ResolvedComposite>, String> {
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
        let Some(source_rect) = DeviceRect::from_logical(rect, scale)
            .expand(padding.ceil())
            .intersect(target_rect.expand(padding.ceil()))
        else {
            return Ok(None);
        };
        let source_rect = source_rect.snap_out();
        let (width, height) = source_rect.pixel_size();
        let texture = self.acquire_transient("Effect Range Source", width, height);
        let ops = filtered_ops_in_range(&scene.draw_ops, effect.z_start, effect.z_end);
        let own_start = resolved.partition_point(|composite| composite.z_index < effect.z_start);
        let own_end = resolved.partition_point(|composite| composite.z_index < effect.z_end);
        let segment = PassSegment {
            scene,
            ops: &ops,
            composites: &resolved[own_start..own_end],
            offset: [source_rect.x, source_rect.y],
            scissor: None,
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
                "Effect Range Result",
            )?,
            None => texture,
        };
        Ok(Some(ResolvedComposite {
            z_index: effect.z_start,
            source: result,
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
                &mut refs,
            );
            passes.and_then(|passes| refs.assert_consumed().map(|()| passes))
        };
        scratch.release_into(self.recorder);
        let passes = encoded?;
        self.recorder.record_passes(passes);
        Ok(dest)
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn resolve_child(
        &mut self,
        layer: &LayerScene,
        child: &ChildLayer,
        scale: f32,
        target_rect: DeviceRect,
        beneath: &Beneath<'_>,
        readers: &CaptureReaders,
        resolved: &mut Vec<ResolvedComposite>,
    ) -> Result<(), String> {
        let z = child.z_index;
        let snap = child
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, scale))
            .unwrap_or_default();
        let grid = uniform_scale_translation(child.transform)
            .filter(|(uniform, _)| (uniform - child.surface_scale).abs() <= 1e-4)
            .map(|(_, translation)| Point::new(translation.x + snap.x, translation.y + snap.y));
        let translation = grid.filter(|_| (child.surface_scale - 1.0).abs() <= 1e-4);
        let (dest, visible_device) = child_device_placement(child, snap, scale, target_rect);

        if let Some(backdrop) = &child.backdrop
            && let Some(visible) = visible_device
        {
            let placement = ChildPlacement {
                z,
                visible,
                dest,
                translation,
                snap,
            };
            let composite = self.resolve_child_backdrop(
                layer,
                child,
                backdrop,
                placement,
                scale,
                target_rect,
                beneath,
                resolved,
            )?;
            resolved.push(composite);
        }

        let Some(visible) = visible_device else {
            return Ok(());
        };
        if readers.tail_allowed(z, visible)
            && let Some(composite) =
                self.shader_only_child(child, z, scale, visible, translation, snap)
        {
            resolved.push(composite);
            return Ok(());
        }
        let Some(surface) =
            self.render_child_surface(layer, child, z, scale, beneath, resolved, grid)?
        else {
            return Ok(());
        };
        if let Some(composite) =
            shader_tail_over_surface(child, &surface, translation, snap, z, scale, visible)
        {
            resolved.push(composite);
            return Ok(());
        }
        let texture = match &child.effect {
            Some(effect) => {
                let layer_pixel_rect = layer_pixel_rect(child, surface.rect, surface.scale);
                self.apply_effect(&surface.texture, effect, layer_pixel_rect, "Layer Effect")?
            }
            None => Rc::clone(&surface.texture),
        };
        let composite = match surface.grid_dest {
            Some(dest) => grid_child_composite(child, z, texture, dest, snap, scale, visible),
            None => {
                let Some(composite) =
                    projected_child_composite(child, z, texture, &surface, snap, scale, visible)
                else {
                    return Ok(());
                };
                composite
            }
        };
        resolved.push(composite);
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
        let content = &child.content;
        let draws_nothing = content.scene.draw_ops.is_empty()
            && content.children.is_empty()
            && content.scene.backdrop_layers.is_empty()
            && content.scene.effect_layers.is_empty();
        if !draws_nothing || !shader_tail_composites(child, shader) {
            return None;
        }
        let surface_logical = child_surface_rect(child)?;
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
            source,
            dest,
            layer_pixel_rect(child, surface_rect, scale),
            grid_rounded_mask(child, snap, scale),
            visible,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_child_surface(
        &mut self,
        layer: &LayerScene,
        child: &ChildLayer,
        z: usize,
        scale: f32,
        beneath: &Beneath<'_>,
        resolved: &[ResolvedComposite],
        grid: Option<Point>,
    ) -> Result<Option<SurfaceRender>, String> {
        let surface_scale = scale * child.surface_scale;
        let translated = (child.surface_scale - 1.0).abs() <= 1e-4;
        let Some(surface_logical) = child_surface_rect(child) else {
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
        let (surface_rect, grid_dest, device_phase) = match grid_offset {
            Some(offset) => {
                let dest = child_rect.translated(offset).snap_out();
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
        let reads_backdrop = child.reads_backdrop();
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
            && let Some(texture) = self.renderer.layer_cache.get(&key)
        {
            self.renderer
                .frame_stats
                .record_layer_cache_hit(&key, width, height);
            return Ok(Some(SurfaceRender {
                texture,
                rect: surface_rect,
                scale: surface_scale,
                grid_dest,
            }));
        }
        let texture = if cache_key.is_some() {
            Rc::new(self.renderer.acquire_retained_surface(width, height))
        } else {
            self.acquire_transient("Layer Surface", width, height)
        };
        let target = PassTarget {
            view: &texture.view,
            width,
            height,
            offset: [surface_rect.x, surface_rect.y],
        };
        let child_beneath = if reads_backdrop {
            self.beneath_for_child(
                layer,
                child,
                z,
                scale,
                beneath,
                resolved,
                grid_offset.filter(|_| translated),
            )?
        } else {
            Beneath {
                base: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                projected: None,
                segments: Vec::new(),
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
            target,
            surface_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &child_beneath,
        )?;
        if let Some(key) = cache_key {
            self.renderer
                .frame_stats
                .record_layer_cache_miss(&key, width, height);
            self.renderer.layer_cache.insert(key, Rc::clone(&texture));
        }
        Ok(Some(SurfaceRender {
            texture,
            rect: surface_rect,
            scale: surface_scale,
            grid_dest,
        }))
    }

    /// What lies beneath an isolated child that reads its backdrop: for a
    /// translated child at the parent's scale, the parent's own beneath and
    /// its ops below the child, re-based into the child's device space; for
    /// any other transform, the parent's pixels under the child captured once
    /// and projected into the child's space.
    #[allow(clippy::too_many_arguments)]
    fn beneath_for_child<'a>(
        &mut self,
        layer: &'a LayerScene,
        child: &ChildLayer,
        z: usize,
        scale: f32,
        beneath: &Beneath<'a>,
        resolved: &'a [ResolvedComposite],
        shift: Option<Point>,
    ) -> Result<Beneath<'a>, String> {
        let own_end = resolved.partition_point(|composite| composite.z_index <= z);
        if let Some(shift) = shift {
            let shift = [shift.x, shift.y];
            let mut segments: Vec<BeneathSegment<'_>> = beneath
                .segments
                .iter()
                .map(|segment| BeneathSegment {
                    placement: [
                        segment.placement[0] - shift[0],
                        segment.placement[1] - shift[1],
                    ],
                    ..*segment
                })
                .collect();
            segments.push(BeneathSegment {
                scene: &layer.scene,
                z_end: z + 1,
                composites: &resolved[..own_end],
                excluded: &[],
                placement: [-shift[0], -shift[1]],
            });
            return Ok(Beneath {
                base: beneath.base,
                projected: beneath.projected.clone(),
                segments,
            });
        }
        let snap = child
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, scale))
            .unwrap_or_default();
        let dest_bounds =
            quad_bounds(child.transform.map_rect(child.local_bounds)).translate(snap.x, snap.y);
        let parent_rect = DeviceRect::from_logical(dest_bounds, scale)
            .expand(2.0)
            .snap_out();
        let capture = self.capture(
            layer,
            z + 1,
            scale,
            parent_rect,
            beneath,
            &resolved[..own_end],
            "Projected Parent Capture",
        )?;
        let surface_scale = scale * child.surface_scale;
        let child_device_to_parent_device = ProjectiveTransform::uniform_scale(1.0 / surface_scale)
            .then(child.transform)
            .then(ProjectiveTransform::translation(snap.x, snap.y))
            .then(ProjectiveTransform::uniform_scale(scale));
        let parent_device_to_capture =
            ProjectiveTransform::translation(-parent_rect.x, -parent_rect.y);
        let child_device_to_capture = child_device_to_parent_device.then(parent_device_to_capture);
        let capture_to_child_device = child_device_to_capture
            .inverse()
            .ok_or_else(|| "child transform is not invertible".to_string())?;
        let dest_quad = capture_to_child_device.map_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: parent_rect.width,
            height: parent_rect.height,
        });
        Ok(Beneath {
            base: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            projected: Some(ProjectedBase {
                source: capture,
                dest_quad,
                inverse: child_device_to_capture.matrix(),
            }),
            segments: Vec::new(),
        })
    }
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

fn filtered_ops_in_range(ops: &[DrawOp], z_start: usize, z_end: usize) -> Vec<DrawOp> {
    ops.iter()
        .filter(|op| op.z_index >= z_start && op.z_index < z_end)
        .copied()
        .collect()
}

/// The logical rect a child's surface covers: everything its content draws,
/// clipped to its bounds when it clips, expanded for its effect's reach.
fn child_surface_rect(child: &ChildLayer) -> Option<Rect> {
    let mut bounds = union_rect(Some(child.local_bounds), scene_bounds(&child.content));
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

pub(crate) fn scene_bounds(layer: &LayerScene) -> Option<Rect> {
    let scene = &layer.scene;
    let mut bounds = None;
    for op in &scene.draw_ops {
        let rect = match op.kind {
            DrawOpKind::Shape(index) => {
                let shape = &scene.shapes[index];
                clipped(quad_bounds(shape.quad), shape.clip)
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
                for (shape, _) in &shadow.shapes {
                    shadow_bounds = union_rect(shadow_bounds, Some(quad_bounds(shape.quad)));
                }
                for text in &shadow.texts {
                    shadow_bounds = union_rect(shadow_bounds, Some(text.rect));
                }
                shadow_bounds.and_then(|rect| {
                    let margin =
                        cranpose_render_common::geometry::blur_extent_margin(shadow.blur_radius);
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
        let child_bounds = child_surface_rect(child)
            .map(|rect| quad_bounds(child.transform.map_rect(rect)))
            .unwrap_or(quad_bounds(child.transform.map_rect(child.local_bounds)));
        bounds = union_rect(bounds, clipped(child_bounds, child.clip));
    }
    bounds
}
