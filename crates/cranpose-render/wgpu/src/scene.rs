//! Scene structures for GPU rendering

use std::{ops::Range, sync::Arc};

use cranpose_core::NodeId;
use cranpose_render_common::graph::DrawCommandId;
pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CommandRecording, DrawPrimitive, GraphicsLayer, ImageBitmap,
    ImageSampling, Point, Recorded, Rect, RenderEffect, ShapeRecorder,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SnapAnchor {
    pub origin: Point,
    pub device_pixel_step: f32,
}

impl SnapAnchor {
    pub(crate) fn rigid(origin: Point) -> Self {
        Self {
            origin,
            device_pixel_step: 1.0,
        }
    }
}

/// Where a recording's record space lands in the scene: the logical
/// offset of its origin, the rigid anchor it snaps with, the clip its
/// records take, and the paint every record takes on the way to the
/// device. One placement per run; the vertex stage reads it as a uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub offset: Point,
    pub snap_anchor: Option<SnapAnchor>,
    pub clip: Option<Rect>,
    /// The corner radius `clip` is rounded with, in logical units; `0.0`
    /// for a rect clip. Records take the rounded rect's coverage, as a layer
    /// clipped to it composites through the same mask.
    pub clip_radius: f32,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
}

impl Placement {
    /// Whether the placement paints its records: an alpha or a colour
    /// filter, which may leave an opaque colour translucent.
    pub(crate) fn paints(&self) -> bool {
        self.alpha != 1.0 || self.color_filter.is_some()
    }

    pub(crate) fn at(offset: Point, snap_anchor: Option<SnapAnchor>, clip: Option<Rect>) -> Self {
        Self {
            offset,
            snap_anchor,
            clip,
            clip_radius: 0.0,
            alpha: 1.0,
            color_filter: None,
        }
    }

    /// The placement with `clip` rounded at `radius`.
    pub(crate) fn with_clip_radius(self, radius: f32) -> Self {
        Self {
            clip_radius: radius,
            ..self
        }
    }

    /// Whether the clip has rounded corners the records take coverage from.
    pub(crate) fn clip_rounded(&self) -> bool {
        self.clip.is_some() && self.clip_radius > 0.0
    }

    /// A placement that paints its records with the layer's alpha and
    /// colour filter; the layer's affine part is identity for anything
    /// drawn direct.
    pub(crate) fn painted(
        offset: Point,
        snap_anchor: Option<SnapAnchor>,
        clip: Option<Rect>,
        layer: &GraphicsLayer,
    ) -> Self {
        Self {
            offset,
            snap_anchor,
            clip,
            clip_radius: 0.0,
            alpha: layer.alpha.clamp(0.0, 1.0),
            color_filter: layer.color_filter,
        }
    }

    pub(crate) fn translated_bounds(&self, local: Rect) -> Rect {
        local.translate(self.offset.x, self.offset.y)
    }
}

/// One recording's shapes drawn under one placement: the POD tables the
/// GPU reads, shared with the recorder, and the segment range this run
/// covers. `command` keys the retained GPU copy; a run without one (a
/// layer's loose primitives, a shadow) is copied into the frame arena.
#[derive(Clone)]
pub(crate) struct RunDraw {
    pub recorder: Arc<ShapeRecorder>,
    pub command: Option<DrawCommandId>,
    pub segments: Range<u32>,
    pub placement: Placement,
    /// The scene-space rect the run's records can reach, before snapping.
    pub bounds: Rect,
}

impl RunDraw {
    pub(crate) fn of(
        recording: &CommandRecording,
        command: Option<DrawCommandId>,
        segments: Range<u32>,
        placement: Placement,
    ) -> Self {
        Self::of_recorder(recording.shape_recorder(), command, segments, placement)
    }

    pub(crate) fn of_recorder(
        recorder: &Arc<ShapeRecorder>,
        command: Option<DrawCommandId>,
        segments: Range<u32>,
        placement: Placement,
    ) -> Self {
        Self {
            recorder: Arc::clone(recorder),
            command,
            segments,
            placement,
            bounds: recorder.bounds().map_or(
                Rect {
                    x: placement.offset.x,
                    y: placement.offset.y,
                    width: 0.0,
                    height: 0.0,
                },
                |bounds| placement.translated_bounds(bounds),
            ),
        }
    }

    /// The whole recorder as one run, when it recorded anything.
    pub(crate) fn whole(recorder: Arc<ShapeRecorder>, placement: Placement) -> Option<Self> {
        (!recorder.is_empty()).then(|| {
            let segments = recorder.all_segments();
            Self::of_recorder(&recorder, None, segments, placement)
        })
    }

    pub(crate) fn tables(&self) -> &cranpose_ui_graphics::RecordTables {
        self.recorder.tables()
    }

    pub(crate) fn segment_records(
        &self,
    ) -> impl Iterator<Item = &cranpose_ui_graphics::RecordSegment> {
        self.tables().segments[self.segments.start as usize..self.segments.end as usize]
            .iter()
            .filter(|segment| segment.lane == cranpose_ui_graphics::RecordLane::Shapes)
    }

    pub(crate) fn record_count(&self) -> u32 {
        self.segment_records().map(|segment| segment.count).sum()
    }
}

/// The primitives a layer pushes one by one, recorded together under the
/// placement they share until one arrives under another placement or
/// something else takes a z between them.
struct LooseRun {
    recorder: ShapeRecorder,
    placement: Placement,
}

#[derive(Clone)]
pub(crate) struct TextDraw {
    pub node_id: NodeId,
    pub rect: Rect,
    pub snap_anchor: Option<SnapAnchor>,
    pub text: std::sync::Arc<cranpose_ui::text::RenderString>,
    pub color: Color,
    pub text_style: Arc<TextStyle>,
    pub font_size: f32,
    pub scale: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub(crate) struct ImageDraw {
    pub rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub snap_anchor: Option<SnapAnchor>,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub src_rect: Option<Rect>,
    pub motion_context_animated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DrawOpKind {
    Run(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawOp {
    pub z_index: usize,
    pub kind: DrawOpKind,
}

/// A shadow's casters: the shapes as one run in the shadow's own space,
/// the cutouts taken after the blur as another under the same placement,
/// and the texts. The shapes' blend modes ride in the records.
#[derive(Clone)]
pub(crate) struct ShadowDraw {
    pub shapes: Option<RunDraw>,
    pub post_blur_cutouts: Option<RunDraw>,
    pub texts: Vec<TextDraw>,
    pub blur_radius: f32,
    pub clip: Option<Rect>,
    pub rounded_clip: Option<LayerRoundedClip>,
    pub occluder: Option<Rect>,
    pub z_index: usize,
}

impl ShadowDraw {
    pub(crate) fn requires_surface(&self) -> bool {
        self.blur_radius > 0.0
            || self.post_blur_cutouts.is_some()
            || self.shapes.as_ref().is_some_and(|shapes| {
                shapes
                    .segment_records()
                    .any(|segment| segment.blend == BlendMode::DstOut)
            })
    }
}

#[derive(Clone)]
pub(crate) struct EffectLayer {
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<SnapAnchor>,
    pub effect: Option<RenderEffect>,
    pub blend_mode: BlendMode,
    pub composite_alpha: f32,
    pub z_start: usize,
    pub z_end: usize,
}

/// A rounded clip in a scene's logical space, applied as a mask when the
/// clipped content is composited from a texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerRoundedClip {
    pub(crate) rect: Rect,
    pub(crate) radii: [f32; 4],
}

#[derive(Clone)]
pub(crate) struct BackdropLayer {
    pub node_id: Option<NodeId>,
    pub alpha: f32,
    pub rect: Rect,
    /// What the layer paints within: the clips above it and its own.
    pub clip: Option<Rect>,
    /// What its effect may read: the clip the layer is drawn in, the way a
    /// list's edge bounds what lies beneath a control inside it. The layer's
    /// own clip bounds its paint alone, so a control still reads past its
    /// own edge.
    pub reach: Option<Rect>,
    pub rounded_clip: Option<LayerRoundedClip>,
    pub snap_anchor: Option<SnapAnchor>,
    pub effect: RenderEffect,
    pub z_index: usize,
}

pub(crate) struct CompositorScene {
    pub runs: Vec<RunDraw>,
    loose: LooseRun,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadow_draws: Vec<ShadowDraw>,
    shadow_recorders: Vec<Arc<ShapeRecorder>>,
    pub draw_ops: Vec<DrawOp>,
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub next_z: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneCapacityHint {
    pub runs: usize,
    pub images: usize,
    pub texts: usize,
    pub shadow_draws: usize,
    pub draw_ops: usize,
    pub effect_layers: usize,
    pub backdrop_layers: usize,
}

const SCENE_BUFFER_POOL_LIMIT: usize = 4;

thread_local! {
    static SCENE_BUFFER_POOL: std::cell::RefCell<Vec<SceneBuffers>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SceneBuffers {
    runs: Vec<RunDraw>,
    loose: ShapeRecorder,
    images: Vec<ImageDraw>,
    texts: Vec<TextDraw>,
    shadow_draws: Vec<ShadowDraw>,
    shadow_recorders: Vec<Arc<ShapeRecorder>>,
    draw_ops: Vec<DrawOp>,
    effect_layers: Vec<EffectLayer>,
    backdrop_layers: Vec<BackdropLayer>,
}

impl Drop for CompositorScene {
    fn drop(&mut self) {
        let _ = SCENE_BUFFER_POOL.try_with(|pool| {
            let Ok(mut pool) = pool.try_borrow_mut() else {
                return;
            };
            if pool.len() >= SCENE_BUFFER_POOL_LIMIT {
                pool.remove(0);
            }
            self.clear();
            pool.push(SceneBuffers {
                runs: std::mem::take(&mut self.runs),
                loose: std::mem::take(&mut self.loose.recorder),
                images: std::mem::take(&mut self.images),
                texts: std::mem::take(&mut self.texts),
                shadow_draws: std::mem::take(&mut self.shadow_draws),
                shadow_recorders: std::mem::take(&mut self.shadow_recorders),
                draw_ops: std::mem::take(&mut self.draw_ops),
                effect_layers: std::mem::take(&mut self.effect_layers),
                backdrop_layers: std::mem::take(&mut self.backdrop_layers),
            });
        });
    }
}

impl CompositorScene {
    pub fn new() -> Self {
        Self::with_capacity(SceneCapacityHint::default())
    }

    pub fn with_capacity(hint: SceneCapacityHint) -> Self {
        if let Some(buffers) = SCENE_BUFFER_POOL.with(|pool| pool.borrow_mut().pop()) {
            return Self {
                runs: buffers.runs,
                loose: LooseRun {
                    recorder: buffers.loose,
                    placement: Placement::at(Point::default(), None, None),
                },
                images: buffers.images,
                texts: buffers.texts,
                shadow_draws: buffers.shadow_draws,
                shadow_recorders: buffers.shadow_recorders,
                draw_ops: buffers.draw_ops,
                effect_layers: buffers.effect_layers,
                backdrop_layers: buffers.backdrop_layers,
                next_z: 0,
            };
        }
        Self {
            runs: Vec::with_capacity(hint.runs),
            loose: LooseRun {
                recorder: ShapeRecorder::default(),
                placement: Placement::at(Point::default(), None, None),
            },
            images: Vec::with_capacity(hint.images),
            texts: Vec::with_capacity(hint.texts),
            shadow_draws: Vec::with_capacity(hint.shadow_draws),
            shadow_recorders: Vec::new(),
            draw_ops: Vec::with_capacity(hint.draw_ops),
            effect_layers: Vec::with_capacity(hint.effect_layers),
            backdrop_layers: Vec::with_capacity(hint.backdrop_layers),
            next_z: 0,
        }
    }

    pub fn capacity_hint(&self) -> SceneCapacityHint {
        SceneCapacityHint {
            runs: self.runs.len(),
            images: self.images.len(),
            texts: self.texts.len(),
            shadow_draws: self.shadow_draws.len(),
            draw_ops: self.draw_ops.len(),
            effect_layers: self.effect_layers.len(),
            backdrop_layers: self.backdrop_layers.len(),
        }
    }

    pub fn clear(&mut self) {
        self.runs.clear();
        self.loose.recorder.clear();
        self.images.clear();
        self.texts.clear();
        let recorder_limit = self.shadow_draws.capacity().saturating_mul(2);
        for shadow in self.shadow_draws.drain(..) {
            for mut run in [shadow.shapes, shadow.post_blur_cutouts]
                .into_iter()
                .flatten()
            {
                if self.shadow_recorders.len() < recorder_limit
                    && let Some(recorder) = Arc::get_mut(&mut run.recorder)
                {
                    recorder.clear();
                    self.shadow_recorders.push(run.recorder);
                }
            }
        }
        self.draw_ops.clear();
        self.effect_layers.clear();
        self.backdrop_layers.clear();
        self.next_z = 0;
    }

    /// The z the next push takes. Closes the open loose run first, since
    /// whatever the caller places at this z must draw above it.
    pub fn next_z(&mut self) -> usize {
        self.flush_loose();
        self.next_z
    }

    /// Records one shape primitive under `placement`, joining the open
    /// loose run when it shares the placement and closing it otherwise.
    /// The primitive is in the placement's record space and carries its
    /// own paint; the placement's alpha and filter are identity.
    pub fn push_loose(&mut self, primitive: DrawPrimitive, placement: Placement) {
        if !self.loose.recorder.is_empty() && self.loose.placement != placement {
            self.flush_loose();
        }
        self.loose.placement = placement;
        if let Recorded::Other(other) = self.loose.recorder.push_primitive(primitive) {
            unreachable!("only shapes join a loose run, not {other:?}");
        }
    }

    /// Closes the open loose run into a run draw at the next z.
    pub fn flush_loose(&mut self) {
        if self.loose.recorder.is_empty() {
            return;
        }
        let Some(run) = RunDraw::whole(
            Arc::new(std::mem::take(&mut self.loose.recorder)),
            self.loose.placement,
        ) else {
            return;
        };
        self.push_run_unflushed(run);
    }

    pub fn push_run(&mut self, run: RunDraw) {
        self.flush_loose();
        self.push_run_unflushed(run);
    }

    fn push_run_unflushed(&mut self, run: RunDraw) {
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.runs.len();
        self.runs.push(run);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Run(index),
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_image_with_geometry(
        &mut self,
        rect: Rect,
        quad: [[f32; 2]; 4],
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
        clip: Option<Rect>,
        src_rect: Option<Rect>,
        blend_mode: BlendMode,
        motion_context_animated: bool,
    ) {
        self.flush_loose();
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.images.len();
        self.images.push(ImageDraw {
            rect,
            quad,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
            clip,
            blend_mode,
            src_rect,
            motion_context_animated,
        });
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Image(index),
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        text: Arc<cranpose_ui::text::RenderString>,
        color: Color,
        text_style: Arc<TextStyle>,
        font_size: f32,
        scale: f32,
        layout_options: TextLayoutOptions,
        clip: Option<Rect>,
    ) {
        self.flush_loose();
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.texts.len();
        self.texts.push(TextDraw {
            node_id,
            rect,
            snap_anchor: None,
            text,
            color,
            text_style,
            font_size,
            scale,
            layout_options,
            clip,
        });
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Text(index),
        });
    }

    pub(crate) fn take_shadow_recorder(&mut self) -> Arc<ShapeRecorder> {
        self.shadow_recorders.pop().unwrap_or_default()
    }

    pub fn push_shadow_draw(&mut self, mut draw: ShadowDraw) {
        self.flush_loose();
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.shadow_draws.len();
        draw.z_index = z_index;
        self.shadow_draws.push(draw);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Shadow(index),
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_effect_layer(
        &mut self,
        rect: Rect,
        clip: Option<Rect>,
        effect: Option<RenderEffect>,
        blend_mode: BlendMode,
        composite_alpha: f32,
        z_start: usize,
        z_end: usize,
    ) {
        self.flush_loose();
        self.effect_layers.push(EffectLayer {
            rect,
            clip,
            snap_anchor: None,
            effect,
            blend_mode,
            composite_alpha,
            z_start,
            z_end,
        });
    }

    pub fn push_backdrop_layer(&mut self, mut layer: BackdropLayer) {
        self.flush_loose();
        layer.z_index = self.next_z;
        self.next_z += 1;
        self.backdrop_layers.push(layer);
    }
}

/// Where a scene's lists stood when a span of pushes began.
#[derive(Clone, Copy)]
pub(crate) struct SceneMark {
    runs: usize,
    images: usize,
    texts: usize,
    shadows: usize,
    ops: usize,
    effects: usize,
    backdrops: usize,
    z: usize,
}

/// The draws a span of pushes appended, with their z and list indices counted
/// from the span's start, so the span can be appended again at another z.
#[derive(Clone)]
pub(crate) struct SceneSegment {
    runs: Vec<RunDraw>,
    images: Vec<ImageDraw>,
    texts: Vec<TextDraw>,
    shadows: Vec<ShadowDraw>,
    ops: Vec<DrawOp>,
    z_count: usize,
}

impl DrawOpKind {
    fn with_index(self, index: impl Fn(usize, usize) -> usize, mark: &SceneMark) -> Self {
        match self {
            Self::Run(at) => Self::Run(index(at, mark.runs)),
            Self::Image(at) => Self::Image(index(at, mark.images)),
            Self::Text(at) => Self::Text(index(at, mark.texts)),
            Self::Shadow(at) => Self::Shadow(index(at, mark.shadows)),
        }
    }

    /// The index counted from where `mark`'s list stood.
    fn since(self, mark: &SceneMark) -> Self {
        self.with_index(|at, from| at - from, mark)
    }

    /// The index of a segment's draw appended where `mark`'s list stands.
    fn after(self, mark: &SceneMark) -> Self {
        self.with_index(|at, from| at + from, mark)
    }
}

impl CompositorScene {
    /// Closes the open loose run, so nothing pushed later joins one begun
    /// before, and marks where the next pushes begin.
    pub(crate) fn mark(&mut self) -> SceneMark {
        self.flush_loose();
        SceneMark {
            runs: self.runs.len(),
            images: self.images.len(),
            texts: self.texts.len(),
            shadows: self.shadow_draws.len(),
            ops: self.draw_ops.len(),
            effects: self.effect_layers.len(),
            backdrops: self.backdrop_layers.len(),
            z: self.next_z,
        }
    }

    /// Whether the pushes since `mark` added draws only, no effect or
    /// backdrop layers.
    pub(crate) fn only_draws_since(&self, mark: SceneMark) -> bool {
        self.effect_layers.len() == mark.effects && self.backdrop_layers.len() == mark.backdrops
    }

    /// The draws pushed since `mark`, after closing the open loose run.
    pub(crate) fn segment_since(&mut self, mark: SceneMark) -> SceneSegment {
        self.flush_loose();
        SceneSegment {
            runs: self.runs[mark.runs..].to_vec(),
            images: self.images[mark.images..].to_vec(),
            texts: self.texts[mark.texts..].to_vec(),
            shadows: self.shadow_draws[mark.shadows..]
                .iter()
                .map(|shadow| ShadowDraw {
                    z_index: shadow.z_index - mark.z,
                    ..shadow.clone()
                })
                .collect(),
            ops: self.draw_ops[mark.ops..]
                .iter()
                .map(|op| DrawOp {
                    z_index: op.z_index - mark.z,
                    kind: op.kind.since(&mark),
                })
                .collect(),
            z_count: self.next_z - mark.z,
        }
    }

    /// Drops the draws pushed since `mark`.
    #[cfg(debug_assertions)]
    pub(crate) fn rewind(&mut self, mark: SceneMark) {
        self.flush_loose();
        self.runs.truncate(mark.runs);
        self.images.truncate(mark.images);
        self.texts.truncate(mark.texts);
        self.shadow_draws.truncate(mark.shadows);
        self.draw_ops.truncate(mark.ops);
        self.next_z = mark.z;
    }

    /// Appends `segment`'s draws above everything pushed so far.
    pub(crate) fn append_segment(&mut self, segment: &SceneSegment) {
        let mark = self.mark();
        self.runs.extend_from_slice(&segment.runs);
        self.images.extend_from_slice(&segment.images);
        self.texts.extend_from_slice(&segment.texts);
        self.shadow_draws
            .extend(segment.shadows.iter().map(|shadow| ShadowDraw {
                z_index: shadow.z_index + mark.z,
                ..shadow.clone()
            }));
        self.draw_ops.extend(segment.ops.iter().map(|op| DrawOp {
            z_index: op.z_index + mark.z,
            kind: op.kind.after(&mark),
        }));
        self.next_z += segment.z_count;
    }
}

#[cfg(debug_assertions)]
impl SceneSegment {
    /// Whether `other` draws what this segment draws: the check a reused
    /// segment gets against a fresh collection in debug builds.
    pub(crate) fn draws_as(&self, other: &Self) -> bool {
        self.z_count == other.z_count
            && self.ops == other.ops
            && same_all(&self.runs, &other.runs, same_run)
            && same_all(&self.images, &other.images, same_image)
            && same_all(&self.texts, &other.texts, same_text)
            && same_all(&self.shadows, &other.shadows, same_shadow)
    }
}

#[cfg(debug_assertions)]
fn same_all<T>(a: &[T], b: &[T], same: impl Fn(&T, &T) -> bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
}

#[cfg(debug_assertions)]
fn same_run(a: &RunDraw, b: &RunDraw) -> bool {
    a.recorder.fingerprint() == b.recorder.fingerprint()
        && a.command == b.command
        && a.segments == b.segments
        && a.placement == b.placement
        && a.bounds == b.bounds
}

#[cfg(debug_assertions)]
fn same_optional_run(a: &Option<RunDraw>, b: &Option<RunDraw>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same_run(a, b),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(debug_assertions)]
fn same_text(a: &TextDraw, b: &TextDraw) -> bool {
    a.node_id == b.node_id
        && a.rect == b.rect
        && a.snap_anchor == b.snap_anchor
        && *a.text == *b.text
        && a.color == b.color
        && *a.text_style == *b.text_style
        && a.font_size == b.font_size
        && a.scale == b.scale
        && a.layout_options == b.layout_options
        && a.clip == b.clip
}

#[cfg(debug_assertions)]
fn same_image(a: &ImageDraw, b: &ImageDraw) -> bool {
    a.rect == b.rect
        && a.quad == b.quad
        && a.snap_anchor == b.snap_anchor
        && a.image.id() == b.image.id()
        && a.alpha == b.alpha
        && a.color_filter == b.color_filter
        && a.sampling == b.sampling
        && a.clip == b.clip
        && a.blend_mode == b.blend_mode
        && a.src_rect == b.src_rect
        && a.motion_context_animated == b.motion_context_animated
}

#[cfg(debug_assertions)]
fn same_shadow(a: &ShadowDraw, b: &ShadowDraw) -> bool {
    same_optional_run(&a.shapes, &b.shapes)
        && same_optional_run(&a.post_blur_cutouts, &b.post_blur_cutouts)
        && same_all(&a.texts, &b.texts, same_text)
        && a.blur_radius == b.blur_radius
        && a.clip == b.clip
        && a.rounded_clip == b.rounded_clip
        && a.occluder == b.occluder
        && a.z_index == b.z_index
}

impl Default for CompositorScene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests/scene_tests.rs"]
mod tests;
