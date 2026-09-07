//! Scene structures for GPU rendering

use std::{ops::Range, rc::Rc, sync::Arc};

use cranpose_core::NodeId;
use cranpose_render_common::graph::DrawCommandId;
pub use cranpose_render_common::graph_scene::{ClickAction, HitRegion, Scene};
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
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
}

impl Placement {
    pub(crate) fn at(offset: Point, snap_anchor: Option<SnapAnchor>, clip: Option<Rect>) -> Self {
        Self {
            offset,
            snap_anchor,
            clip,
            alpha: 1.0,
            color_filter: None,
        }
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
            bounds: recorder
                .bounds()
                .map(|bounds| placement.translated_bounds(bounds))
                .unwrap_or(Rect {
                    x: placement.offset.x,
                    y: placement.offset.y,
                    width: 0.0,
                    height: 0.0,
                }),
        }
    }

    /// The whole recorder as one run, when it recorded anything.
    pub(crate) fn whole(recorder: ShapeRecorder, placement: Placement) -> Option<Self> {
        (!recorder.is_empty()).then(|| {
            let segments = recorder.all_segments();
            Self::of_recorder(&Arc::new(recorder), None, segments, placement)
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
    pub text_style: TextStyle,
    pub font_size: f32,
    pub scale: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

const RENDER_STRING_MEMO_CAPACITY: usize = 2048;

thread_local! {
    static RENDER_STRING_MEMO: std::cell::RefCell<
        cranpose_core::collections::map::HashMap<usize, RenderStringMemoEntry>,
    > = std::cell::RefCell::new(cranpose_core::collections::map::HashMap::default());
}

struct RenderStringMemoEntry {
    text: std::rc::Weak<cranpose_ui::text::AnnotatedString>,
    render: std::sync::Arc<cranpose_ui::text::RenderString>,
}

pub(crate) fn render_string_for(
    text: &Rc<cranpose_ui::text::AnnotatedString>,
) -> std::sync::Arc<cranpose_ui::text::RenderString> {
    RENDER_STRING_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        let key = Rc::as_ptr(text) as usize;
        if let Some(entry) = memo.get(&key)
            && entry.text.strong_count() > 0
            && entry.text.as_ptr() == Rc::as_ptr(text)
        {
            return std::sync::Arc::clone(&entry.render);
        }

        let render = std::sync::Arc::new(text.render_string());
        if memo.len() >= RENDER_STRING_MEMO_CAPACITY {
            memo.retain(|_, entry| entry.text.strong_count() > 0);
            if memo.len() >= RENDER_STRING_MEMO_CAPACITY {
                memo.clear();
            }
        }
        memo.insert(
            key,
            RenderStringMemoEntry {
                text: Rc::downgrade(text),
                render: std::sync::Arc::clone(&render),
            },
        );
        render
    })
}

#[derive(Clone)]
pub(crate) struct ImageDraw {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub snap_anchor: Option<SnapAnchor>,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub z_index: usize,
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
    pub rect: Rect,
    pub clip: Option<Rect>,
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
                return;
            }
            self.clear();
            pool.push(SceneBuffers {
                runs: std::mem::take(&mut self.runs),
                loose: std::mem::take(&mut self.loose.recorder),
                images: std::mem::take(&mut self.images),
                texts: std::mem::take(&mut self.texts),
                shadow_draws: std::mem::take(&mut self.shadow_draws),
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
        self.images.clear();
        self.texts.clear();
        self.shadow_draws.clear();
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
        let Some(run) = RunDraw::whole(
            std::mem::take(&mut self.loose.recorder),
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

    #[allow(clippy::too_many_arguments)]
    pub fn push_image_with_geometry(
        &mut self,
        rect: Rect,
        local_rect: Rect,
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
            local_rect,
            quad,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
            z_index,
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

    #[allow(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        text: Rc<cranpose_ui::text::AnnotatedString>,
        color: Color,
        text_style: TextStyle,
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
            text: render_string_for(&text),
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

    #[allow(clippy::too_many_arguments)]
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
mod tests {
    use cranpose_ui_graphics::Brush;

    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn loose_rect(x: f32) -> DrawPrimitive {
        DrawPrimitive::Rect {
            rect: rect(x, 0.0, 10.0, 10.0),
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }
    }

    #[test]
    fn loose_primitives_under_one_placement_share_a_run_and_close_before_anything_else() {
        let mut scene = CompositorScene::new();
        let placement = Placement::at(Point::new(5.0, 5.0), None, None);
        scene.push_loose(loose_rect(0.0), placement);
        scene.push_loose(loose_rect(20.0), placement);
        assert!(scene.runs.is_empty());
        let z = scene.next_z();
        assert_eq!(scene.runs.len(), 1);
        assert_eq!(z, 1);
        assert_eq!(scene.runs[0].record_count(), 2);
        assert_eq!(scene.runs[0].bounds, rect(5.0, 5.0, 30.0, 10.0));
        scene.push_loose(
            loose_rect(40.0),
            Placement::at(Point::default(), None, None),
        );
        scene.push_loose(
            loose_rect(60.0),
            Placement::at(Point::default(), None, Some(rect(0.0, 0.0, 1.0, 1.0))),
        );
        scene.flush_loose();
        assert_eq!(scene.runs.len(), 3);
        assert_eq!(
            scene
                .draw_ops
                .iter()
                .map(|op| op.z_index)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn a_run_pushed_after_loose_primitives_draws_above_them() {
        let mut scene = CompositorScene::new();
        scene.push_loose(loose_rect(0.0), Placement::at(Point::default(), None, None));
        let recording = CommandRecording::from_primitives(vec![loose_rect(1.0)]);
        scene.push_run(RunDraw::of(
            &recording,
            None,
            recording.all_segments(),
            Placement::at(Point::default(), None, None),
        ));
        assert_eq!(scene.runs.len(), 2);
        assert!(matches!(scene.draw_ops[0].kind, DrawOpKind::Run(0)));
        assert!(matches!(scene.draw_ops[1].kind, DrawOpKind::Run(1)));
    }

    #[test]
    fn scene_drop_skips_pooling_when_the_pool_is_held() {
        let scene = CompositorScene::new();
        SCENE_BUFFER_POOL.with(|pool| {
            let _held = pool.borrow_mut();
            let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(scene)));
            assert!(
                dropped.is_ok(),
                "dropping a scene under a held pool borrow must not panic"
            );
        });
    }
}
