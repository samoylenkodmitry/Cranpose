use std::{ops::Range, rc::Rc, sync::Arc};

use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_render_common::{
    graph::{LayerChange, LayerNode, ProjectiveTransform},
    layer_shadow::{ShadowLight, ShadowRRect},
    primitive_emit::ShapeDrawParams,
};
use cranpose_ui::TextStyle;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, LineGeometry,
    Point, Rect, RoundedCornerShape, Stroke, Trapezoid,
};

use crate::draw::RenderItem;

#[derive(Clone, Debug)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub snap_to_pixel_grid: bool,
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    pub stroke: Option<Stroke>,
    pub arc: Option<ArcGeometry>,
    pub line: Option<LineGeometry>,
    pub trapezoid: Option<Trapezoid>,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

/// A point moved by `dx`, `dy`.
fn shifted(point: Point, dx: f32, dy: f32) -> Point {
    Point::new(point.x + dx, point.y + dy)
}

impl DrawShape {
    /// The shape moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            arc: self.arc.map(|arc| ArcGeometry {
                center: shifted(arc.center, dx, dy),
                ..arc
            }),
            line: self.line.map(|line| {
                line.placed(shifted(line.start, dx, dy), shifted(line.end, dx, dy), 1.0)
            }),
            trapezoid: self.trapezoid.map(|trapezoid| trapezoid.translate(dx, dy)),
            clip: None,
            ..self.clone()
        }
    }

    /// The shape `params` resolved.
    pub(crate) fn of_params(params: ShapeDrawParams) -> Self {
        Self {
            rect: params.rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush: params.brush.into_brush(),
            shape: params.shape,
            stroke: params.stroke,
            arc: params.arc,
            line: params.line,
            trapezoid: params.trapezoid,
            clip: params.clip,
            blend_mode: params.blend_mode,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub text: Rc<cranpose_ui::text::AnnotatedString>,
    pub color: Color,
    pub text_style: Arc<TextStyle>,
    pub font_size: f32,
    pub scale: f32,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub(crate) struct ImageDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub src_rect: Option<Rect>,
    /// Maps `rect`'s space to device pixels for an image drawn turned or
    /// mirrored; `None` when `rect` is in device pixels.
    pub transform: Option<ProjectiveTransform>,
}

/// One pass of a layer's elevation shadow: Skia's round rect shadow in
/// device pixels, its color and the clip it is cut to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowDraw {
    pub shadow: ShadowRRect,
    pub snap_anchor: Option<Point>,
    pub color: Color,
    pub clip: Option<Rect>,
}

impl TextDraw {
    /// The text moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..self.clone()
        }
    }
}

impl ImageDraw {
    /// The image moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        let (rect, transform) = match self.transform {
            Some(transform) => (
                self.rect,
                Some(transform.then(ProjectiveTransform::translation(dx, dy))),
            ),
            None => (self.rect.translate(dx, dy), None),
        };
        Self {
            rect,
            transform,
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..self.clone()
        }
    }
}

impl ShadowDraw {
    /// The shadow moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            shadow: self.shadow.translated(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..*self
        }
    }
}

/// How many items of each kind a scene holds where a run of its items starts
/// or stops.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ItemCounts {
    pub order: usize,
    pub shapes: usize,
    pub images: usize,
    pub texts: usize,
    pub shadows: usize,
}

impl ItemCounts {
    /// How many items of each kind lie from `start` up to `self`.
    fn since(self, start: Self) -> Self {
        Self {
            order: self.order - start.order,
            shapes: self.shapes - start.shapes,
            images: self.images - start.images,
            texts: self.texts - start.texts,
            shadows: self.shadows - start.shadows,
        }
    }

    /// The counts of a run copied from where `from` began in one scene to
    /// where `to` begins in another.
    fn moved(self, from: Self, to: Self) -> Self {
        Self {
            order: to.order + self.order.saturating_sub(from.order),
            shapes: to.shapes + self.shapes.saturating_sub(from.shapes),
            images: to.images + self.images.saturating_sub(from.images),
            texts: to.texts + self.texts.saturating_sub(from.texts),
            shadows: to.shadows + self.shadows.saturating_sub(from.shadows),
        }
    }
}

/// How much of a scene a layer's subtree holds: its records, its items of
/// each kind, and the entries of what its layers passed down. Subtrees of
/// one extent fit each other's places wherever each lies in its scene.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubtreeExtent {
    records: usize,
    items: ItemCounts,
    passed_down: usize,
}

/// What a layer passes down to the walks of its child layers.
#[derive(Clone, PartialEq)]
pub(crate) struct PassedDown {
    pub transform: ProjectiveTransform,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<Point>,
    pub translated: bool,
}

/// The items a layer's subtree pushed into a scene, in a run of each list,
/// so the next scene can copy them when nothing they depend on changed.
#[derive(Clone, Copy)]
pub(crate) struct SubtreeItems {
    /// The layer's node and the node whose outer draws it wraps.
    pub identity: (Option<NodeId>, Option<NodeId>),
    /// The layer's change number, which covers its subtree, when it pushed
    /// the items.
    pub change: LayerChange,
    /// The layer's own change number, which a change beneath it leaves.
    pub own_change: LayerChange,
    /// The layer's placement in its parent, which the number leaves out.
    pub transform: ProjectiveTransform,
    /// Where [`RasterScene::passed_down`] holds what the layer passed to its
    /// child layers, when it has any.
    pub passed_down: Option<usize>,
    /// The index of the first record after the subtree's, its descendants'
    /// included.
    pub end: usize,
    pub start: ItemCounts,
    pub stop: ItemCounts,
}

pub(crate) struct RasterScene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadows: Vec<ShadowDraw>,
    /// Every item in the order it was pushed, which is the order it draws in.
    pub order: Vec<RenderItem>,
    /// A record for each layer the scene was built from, in the order the
    /// layers were visited: a layer's descendants follow it.
    pub subtrees: Vec<SubtreeItems>,
    /// What the layers with child layers passed down to them.
    pub passed_down: Vec<PassedDown>,
    /// The scale and the shadow light the walk started from.
    pub start: Option<(f32, ShadowLight)>,
    /// The runs of `order` copied in place from the scene the walk started
    /// from, ascending: their items equal that scene's at the same indices.
    pub(crate) kept: Vec<Range<usize>>,
}

impl RasterScene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            shadows: Vec::new(),
            order: Vec::new(),
            subtrees: Vec::new(),
            passed_down: Vec::new(),
            start: None,
            kept: Vec::new(),
        }
    }

    /// Empties the scene and keeps its storage.
    pub(crate) fn clear(&mut self) {
        self.shapes.clear();
        self.images.clear();
        self.texts.clear();
        self.shadows.clear();
        self.order.clear();
        self.subtrees.clear();
        self.passed_down.clear();
        self.start = None;
        self.kept.clear();
    }

    pub(crate) fn counts(&self) -> ItemCounts {
        ItemCounts {
            order: self.order.len(),
            shapes: self.shapes.len(),
            images: self.images.len(),
            texts: self.texts.len(),
            shadows: self.shadows.len(),
        }
    }

    /// Opens the record of `layer`'s subtree, whose items come next, and
    /// returns its index for [`Self::close_subtree`].
    pub(crate) fn open_subtree(&mut self, layer: &LayerNode) -> usize {
        let index = self.subtrees.len();
        let counts = self.counts();
        self.subtrees.push(SubtreeItems {
            identity: (layer.node_id, layer.wraps),
            change: layer.change,
            own_change: layer.own_change,
            transform: layer.transform_to_parent,
            passed_down: None,
            end: index + 1,
            start: counts,
            stop: counts,
        });
        index
    }

    /// Notes in the record `index` what its layer passes down.
    pub(crate) fn note_passed_down(&mut self, index: usize, passed_down: PassedDown) {
        let at = self.passed_down.len();
        self.passed_down.push(passed_down);
        if let Some(record) = self.subtrees.get_mut(index) {
            record.passed_down = Some(at);
        }
    }

    /// Closes the record `index`: the subtree pushed everything since it
    /// opened.
    pub(crate) fn close_subtree(&mut self, index: usize) {
        let end = self.subtrees.len();
        let stop = self.counts();
        if let Some(record) = self.subtrees.get_mut(index) {
            record.end = end;
            record.stop = stop;
        }
    }

    /// Pushes the items and records of the subtrees `old` recorded at
    /// `records`, siblings one after another, which pushed their items there
    /// up to `stop`.
    pub(crate) fn copy_subtrees(
        &mut self,
        old: &RasterScene,
        records: Range<usize>,
        stop: ItemCounts,
    ) {
        let Some(from) = old.subtrees.get(records.start).map(|first| first.start) else {
            return;
        };
        let to = self.counts();
        let run = |start: usize, stop: usize| start..stop.max(start);
        self.shapes.extend_from_slice(
            old.shapes
                .get(run(from.shapes, stop.shapes))
                .unwrap_or_default(),
        );
        self.images.extend_from_slice(
            old.images
                .get(run(from.images, stop.images))
                .unwrap_or_default(),
        );
        self.texts.extend_from_slice(
            old.texts
                .get(run(from.texts, stop.texts))
                .unwrap_or_default(),
        );
        self.shadows.extend_from_slice(
            old.shadows
                .get(run(from.shadows, stop.shadows))
                .unwrap_or_default(),
        );
        let moved = |index: usize, from: usize, to: usize| to + index.saturating_sub(from);
        // A run that lands where it was in the old scene, as every run
        // before the first change does, keeps its indices.
        let base = self.subtrees.len();
        let first = records.start;
        let in_place = from == to && first == base;
        let order = old
            .order
            .get(run(from.order, stop.order))
            .unwrap_or_default();
        if in_place {
            let kept = self.order.len()..self.order.len() + order.len();
            match self.kept.last_mut() {
                Some(last) if last.end == kept.start => last.end = kept.end,
                _ if !kept.is_empty() => self.kept.push(kept),
                _ => {}
            }
            self.order.extend_from_slice(order);
        } else {
            self.order.extend(order.iter().map(|item| match *item {
                RenderItem::Shape(index) => RenderItem::Shape(moved(index, from.shapes, to.shapes)),
                RenderItem::Image(index) => RenderItem::Image(moved(index, from.images, to.images)),
                RenderItem::Text(index) => RenderItem::Text(moved(index, from.texts, to.texts)),
                RenderItem::Shadow(index) => {
                    RenderItem::Shadow(moved(index, from.shadows, to.shadows))
                }
            }));
        }
        self.subtrees
            .extend_from_slice(old.subtrees.get(records).unwrap_or_default());
        let passed_down = &mut self.passed_down;
        for record in self.subtrees.get_mut(base..).unwrap_or_default() {
            if !in_place {
                record.end = moved(record.end, first, base);
                record.start = record.start.moved(from, to);
                record.stop = record.stop.moved(from, to);
            }
            record.passed_down = record
                .passed_down
                .and_then(|at| old.passed_down.get(at))
                .map(|copied| {
                    passed_down.push(copied.clone());
                    passed_down.len() - 1
                });
        }
    }

    /// How much of the scene the subtree whose record is `record` holds;
    /// `None` when its layers' entries of what they passed down do not lie
    /// in one run.
    pub(crate) fn subtree_extent(&self, record: usize) -> Option<SubtreeExtent> {
        self.subtree_span(record).map(|(extent, _)| extent)
    }

    /// The extent of the subtree whose record is `record`, and the index of
    /// its first entry of what its layers passed down.
    fn subtree_span(&self, record: usize) -> Option<(SubtreeExtent, usize)> {
        let head = self.subtrees.get(record)?;
        let records = self.subtrees.get(record..head.end)?;
        let mut passed_down = records.iter().filter_map(|record| record.passed_down);
        let first = passed_down.next();
        let count = usize::from(first.is_some()) + passed_down.count();
        let last = records.iter().rev().find_map(|record| record.passed_down);
        if let (Some(first), Some(last)) = (first, last)
            && last + 1 != first + count
        {
            return None;
        }
        let extent = SubtreeExtent {
            records: records.len(),
            items: head.stop.since(head.start),
            passed_down: count,
        };
        Some((extent, first.unwrap_or(0)))
    }

    /// Puts the subtree `from` holds at its record `from_record` in place of
    /// this scene's subtree at `record`, which must have the same extent:
    /// its items, records and passed-down entries take the old ones' places,
    /// and every other part of the scene stays as it was.
    pub(crate) fn replace_subtree(
        &mut self,
        record: usize,
        from: &RasterScene,
        from_record: usize,
    ) {
        let (Some((old, old_first)), Some((new, new_first))) =
            (self.subtree_span(record), from.subtree_span(from_record))
        else {
            return;
        };
        if old != new {
            return;
        }
        let (to, old_stop) = (self.subtrees[record].start, self.subtrees[record].stop);
        let (at, new_stop) = (
            from.subtrees[from_record].start,
            from.subtrees[from_record].stop,
        );
        let lists = |start: ItemCounts, stop: ItemCounts| {
            (
                start.shapes..stop.shapes,
                start.images..stop.images,
                start.texts..stop.texts,
                start.shadows..stop.shadows,
            )
        };
        let (shapes, images, texts, shadows) = lists(to, old_stop);
        let (new_shapes, new_images, new_texts, new_shadows) = lists(at, new_stop);
        self.shapes[shapes].clone_from_slice(&from.shapes[new_shapes]);
        self.images[images].clone_from_slice(&from.images[new_images]);
        self.texts[texts].clone_from_slice(&from.texts[new_texts]);
        self.shadows[shadows].clone_from_slice(&from.shadows[new_shadows]);
        let moved = |index: usize, from: usize, to: usize| to + (index - from);
        let order = to.order..old_stop.order;
        for (slot, item) in self.order[order]
            .iter_mut()
            .zip(&from.order[at.order..new_stop.order])
        {
            *slot = match *item {
                RenderItem::Shape(index) => RenderItem::Shape(moved(index, at.shapes, to.shapes)),
                RenderItem::Image(index) => RenderItem::Image(moved(index, at.images, to.images)),
                RenderItem::Text(index) => RenderItem::Text(moved(index, at.texts, to.texts)),
                RenderItem::Shadow(index) => {
                    RenderItem::Shadow(moved(index, at.shadows, to.shadows))
                }
            };
        }
        let count = old.passed_down;
        self.passed_down[old_first..old_first + count]
            .clone_from_slice(&from.passed_down[new_first..new_first + count]);
        let records = &from.subtrees[from_record..from_record + new.records];
        for (slot, copied) in self.subtrees[record..record + old.records]
            .iter_mut()
            .zip(records)
        {
            *slot = SubtreeItems {
                end: moved(copied.end, from_record, record),
                start: copied.start.moved(at, to),
                stop: copied.stop.moved(at, to),
                passed_down: copied
                    .passed_down
                    .map(|index| moved(index, new_first, old_first)),
                ..*copied
            };
        }
    }

    /// Queues `shape` to draw over everything queued before.
    pub(crate) fn push_draw_shape(&mut self, shape: DrawShape) {
        self.order.push(RenderItem::Shape(self.shapes.len()));
        self.shapes.push(shape);
    }

    pub fn push_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        self.push_draw_shape(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            clip,
            blend_mode,
        });
    }

    pub fn push_shadow(&mut self, shadow: ShadowRRect, color: Color, clip: Option<Rect>) {
        self.order.push(RenderItem::Shadow(self.shadows.len()));
        self.shadows.push(ShadowDraw {
            shadow,
            snap_anchor: None,
            color,
            clip,
        });
    }

    pub fn push_pixel_snapped_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        self.push_draw_shape(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: true,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            clip,
            blend_mode,
        });
    }

    /// Queues a shape an emitted primitive resolved: a rect, rounded rect,
    /// stroke, arc band, line segment or slice of a path fill.
    pub fn push_shape_params(&mut self, params: ShapeDrawParams) {
        self.push_draw_shape(DrawShape::of_params(params));
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_image_with_geometry(
        &mut self,
        rect: Rect,
        _local_rect: Rect,
        _quad: [[f32; 2]; 4],
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
        clip: Option<Rect>,
        src_rect: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        self.order.push(RenderItem::Image(self.images.len()));
        self.images.push(ImageDraw {
            rect,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
            clip,
            blend_mode,
            src_rect,
            transform: None,
        });
    }

    /// Queues the pixels a layer drew into a surface of its own: `image`
    /// over `rect`, which `transform` maps to device pixels when the surface
    /// is turned, cut to `clip` and drawn at `alpha`.
    pub(crate) fn push_surface_image(
        &mut self,
        image: ImageBitmap,
        rect: Rect,
        transform: Option<ProjectiveTransform>,
        clip: Option<Rect>,
        alpha: f32,
    ) {
        let sampling = if transform.is_some() {
            ImageSampling::Linear
        } else {
            ImageSampling::Nearest
        };
        self.order.push(RenderItem::Image(self.images.len()));
        self.images.push(ImageDraw {
            rect,
            snap_anchor: None,
            image,
            alpha,
            color_filter: None,
            sampling,
            clip,
            blend_mode: BlendMode::SrcOver,
            src_rect: None,
            transform,
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        rect: Rect,
        text: Rc<cranpose_ui::text::AnnotatedString>,
        color: Color,
        text_style: Arc<TextStyle>,
        font_size: f32,
        scale: f32,
        clip: Option<Rect>,
    ) {
        self.order.push(RenderItem::Text(self.texts.len()));
        self.texts.push(TextDraw {
            rect,
            snap_anchor: None,
            text,
            color,
            text_style,
            font_size,
            scale,
            clip,
        });
    }
}

impl Default for RasterScene {
    fn default() -> Self {
        Self::new()
    }
}
