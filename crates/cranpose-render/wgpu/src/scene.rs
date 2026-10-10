//! Scene structures for GPU rendering

use std::{ops::Range, sync::Arc};

use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_render_common::{
    graph::DrawCommandId, layer_shadow::ShadowRRect, text_paint::TextPaint,
};
use cranpose_ui::{TextLayoutOptions, TextStyle, text::RenderString};
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
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placement {
    pub offset: Point,
    pub snap_anchor: Option<SnapAnchor>,
    pub clip: Option<Rect>,
    /// The corner radius `clip` is rounded with, in logical units; `0.0`
    /// for a rect clip. Records take the rounded rect's coverage, as a layer
    /// clipped to it composites through the same mask.
    pub clip_radius: f32,
    pub alpha: f32,
    /// Boxed: few layers filter, and inline the 4x5 matrix took 84 of a
    /// run draw's 184 bytes.
    pub color_filter: Option<Arc<ColorFilter>>,
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
            color_filter: layer.color_filter.as_ref().map(|filter| Arc::new(*filter)),
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
        let bounds = recorder.bounds().map_or(
            Rect {
                x: placement.offset.x,
                y: placement.offset.y,
                width: 0.0,
                height: 0.0,
            },
            |bounds| placement.translated_bounds(bounds),
        );
        Self {
            recorder: Arc::clone(recorder),
            command,
            segments,
            placement,
            bounds,
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

/// A text's string as its draws carry it, with what drawing a cached glyph
/// run reads of it and of its style on every frame: the present thread then
/// loads neither for a text whose run it holds.
#[derive(Clone)]
pub(crate) struct DrawnText {
    string: Arc<RenderString>,
    hash: u64,
    empty: bool,
    gradient: bool,
    static_motion: bool,
}

impl DrawnText {
    /// `string` drawn in `style`, its facts read from both.
    pub(crate) fn of(string: Arc<RenderString>, style: &TextStyle) -> Self {
        let paints_gradient = |span: &cranpose_ui::text::SpanStyle| {
            span.brush
                .as_ref()
                .is_some_and(|brush| !matches!(brush, cranpose_ui_graphics::Brush::Solid(_)))
        };
        let gradient = paints_gradient(&style.span_style)
            || string
                .span_styles()
                .iter()
                .any(|span| paints_gradient(&span.item));
        Self {
            hash: string.render_hash(),
            empty: string.is_empty(),
            gradient,
            static_motion: style
                .paragraph_style
                .text_motion
                .unwrap_or(cranpose_ui::text::TextMotion::Static)
                == cranpose_ui::text::TextMotion::Static,
            string,
        }
    }

    /// The string of a plain text (see [`TextPaint::plain`]), its facts
    /// taken from the paint its node worked out: a plain text paints no
    /// gradient, in its style or in a span.
    pub(crate) fn plain(string: Arc<RenderString>, paint: &TextPaint) -> Self {
        Self {
            string,
            hash: paint.text_hash,
            empty: paint.text_empty,
            gradient: false,
            static_motion: paint.static_motion,
        }
    }

    pub(crate) fn string(&self) -> &Arc<RenderString> {
        &self.string
    }

    /// [`RenderString::render_hash`] of the string.
    pub(crate) fn render_hash(&self) -> u64 {
        self.hash
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.empty
    }

    /// Whether the style or a span of the string paints with a gradient
    /// brush.
    pub(crate) fn gradient(&self) -> bool {
        self.gradient
    }

    /// Whether the style's motion is static, so its glyphs stay on whole
    /// pixels.
    pub(crate) fn static_motion(&self) -> bool {
        self.static_motion
    }
}

#[derive(Clone)]
pub(crate) struct TextDraw {
    pub node_id: NodeId,
    pub rect: Rect,
    pub snap_anchor: Option<SnapAnchor>,
    pub text: DrawnText,
    pub color: Color,
    pub text_style: Arc<TextStyle>,
    /// `text_style`'s [`TextStyle::render_hash`], hashed once per text.
    pub style_hash: u64,
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
    /// The corner radius of `clip` when the image takes a rounded clip in
    /// place; `0.0` for a rect clip.
    pub clip_radius: f32,
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
    RRectShadow(usize),
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

/// One pass of a layer's elevation shadow, drawn straight into the pass:
/// Skia's round rect shadow in the scene's logical space, its color, the
/// clip it is cut to and the rigid anchor it snaps with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RRectShadowDraw {
    pub shadow: ShadowRRect,
    pub color: Color,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<SnapAnchor>,
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
    pub rrect_shadows: Vec<RRectShadowDraw>,
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
    pub rrect_shadows: usize,
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
    rrect_shadows: Vec<RRectShadowDraw>,
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
                rrect_shadows: std::mem::take(&mut self.rrect_shadows),
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
                rrect_shadows: buffers.rrect_shadows,
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
            rrect_shadows: Vec::with_capacity(hint.rrect_shadows),
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
            rrect_shadows: self.rrect_shadows.len(),
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
        self.rrect_shadows.clear();
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
            self.loose.placement.clone(),
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
        (clip, clip_radius): (Option<Rect>, f32),
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
            clip_radius,
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
        text: DrawnText,
        color: Color,
        (text_style, style_hash): (Arc<TextStyle>, u64),
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
            style_hash,
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

    pub(crate) fn push_rrect_shadow(&mut self, draw: RRectShadowDraw) {
        self.flush_loose();
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.rrect_shadows.len();
        self.rrect_shadows.push(draw);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::RRectShadow(index),
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

impl Default for CompositorScene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests/scene_tests.rs"]
mod tests;
