use std::{ops::Range, sync::Arc};

use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_render_common::{
    glyph_reach::{InkSpread, glyph_part},
    graph::ProjectiveTransform,
    layer_shadow::{ShadowLight, ShadowRRect},
    primitive_emit::ShapeDrawParams,
    scene_store::{ItemRef as RecordItem, LayerRef, RecordChange},
};
use cranpose_ui::TextStyle;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, LineGeometry,
    Point, Rect, RoundedCornerShape, Stroke, Trapezoid,
};

use crate::{
    draw::RenderItem,
    rewrite::Rewrite,
    scene_item::{ItemRef, SceneItem},
};

#[derive(Clone, Debug)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub snap_to_pixel_grid: bool,
    pub brush: Brush,
    pub outline: Outline,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

// What a shape draws of its rect. The parameters a shape comes from hold
// one geometry at a time, so one enum keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Outline {
    // The rect, or the shape rounded within it, filled.
    Fill(Option<RoundedCornerShape>),
    // The edge of the rect, or of the shape rounded within it.
    Stroke(Option<RoundedCornerShape>, Stroke),
    Arc(ArcGeometry),
    Line(LineGeometry),
    Trapezoid(Trapezoid),
}

impl Outline {
    // The rounded shape a fill or a stroke follows.
    pub(crate) fn rounded(self) -> Option<RoundedCornerShape> {
        match self {
            Self::Fill(shape) | Self::Stroke(shape, _) => shape,
            Self::Arc(_) | Self::Line(_) | Self::Trapezoid(_) => None,
        }
    }

    fn translated(self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Arc(arc) => Self::Arc(ArcGeometry {
                center: shifted(arc.center, dx, dy),
                ..arc
            }),
            Self::Line(line) => {
                Self::Line(line.placed(shifted(line.start, dx, dy), shifted(line.end, dx, dy), 1.0))
            }
            Self::Trapezoid(trapezoid) => Self::Trapezoid(trapezoid.translate(dx, dy)),
            Self::Fill(_) | Self::Stroke(..) => self,
        }
    }

    // The geometry of `params`: a line, a slice and an arc each stand for
    // the whole shape, and a stroke or a fill follows its rounded shape.
    fn of_params(params: &ShapeDrawParams) -> Self {
        match (params.line, params.trapezoid, params.arc, params.stroke) {
            (Some(line), ..) => Self::Line(line),
            (None, Some(trapezoid), ..) => Self::Trapezoid(trapezoid),
            (None, None, Some(arc), _) => Self::Arc(arc),
            (None, None, None, Some(stroke)) => Self::Stroke(params.shape, stroke),
            (None, None, None, None) => Self::Fill(params.shape),
        }
    }
}

fn shifted(point: Point, dx: f32, dy: f32) -> Point {
    Point::new(point.x + dx, point.y + dy)
}

impl DrawShape {
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            outline: self.outline.translated(dx, dy),
            clip: None,
            ..self.clone()
        }
    }

    pub(crate) fn of_params(params: ShapeDrawParams) -> Self {
        Self {
            rect: params.rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            outline: Outline::of_params(&params),
            brush: params.brush.into_brush(),
            clip: params.clip,
            blend_mode: params.blend_mode,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub text: Arc<cranpose_ui::text::RenderString>,
    // The lines and glyphs, from `rect`'s top left corner.
    pub layout: cranpose_text::TextLayout,
    // Whether the glyphs paint or only their shadows.
    pub ink: cranpose_render_common::text_raster::GlyphInk,
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowDraw {
    pub shadow: ShadowRRect,
    pub snap_anchor: Option<Point>,
    pub color: Color,
    pub clip: Option<Rect>,
}

impl TextDraw {
    // The part of `rect`, where the text draws from its top left corner,
    // in which its glyphs and their shadows draw.
    pub(crate) fn glyph_reach(&self, rect: Rect) -> Rect {
        let spread = InkSpread::of_text(&self.text_style, self.text.span_styles(), &self.layout);
        glyph_part(rect, &self.layout, self.scale, spread)
    }

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
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            shadow: self.shadow.translated(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..*self
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ItemCounts {
    pub order: usize,
    pub shapes: usize,
    pub images: usize,
    pub texts: usize,
    pub shadows: usize,
}

impl ItemCounts {
    pub(crate) fn since(self, start: Self) -> Self {
        Self {
            order: self.order - start.order,
            shapes: self.shapes - start.shapes,
            images: self.images - start.images,
            texts: self.texts - start.texts,
            shadows: self.shadows - start.shadows,
        }
    }

    // The counts after a run of `removed` items before them gave way to a
    // run of `added`.
    pub(crate) fn shifted(self, removed: Self, added: Self) -> Self {
        Self {
            order: self.order - removed.order + added.order,
            shapes: self.shapes - removed.shapes + added.shapes,
            images: self.images - removed.images + added.images,
            texts: self.texts - removed.texts + added.texts,
            shadows: self.shadows - removed.shadows + added.shadows,
        }
    }

    pub(crate) fn moved(self, from: Self, to: Self) -> Self {
        Self {
            order: to.order + self.order.saturating_sub(from.order),
            shapes: to.shapes + self.shapes.saturating_sub(from.shapes),
            images: to.images + self.images.saturating_sub(from.images),
            texts: to.texts + self.texts.saturating_sub(from.texts),
            shadows: to.shadows + self.shadows.saturating_sub(from.shadows),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubtreeExtent {
    pub records: usize,
    pub items: ItemCounts,
    pub passed_down: usize,
}

#[derive(Clone, PartialEq)]
pub(crate) struct PassedDown {
    pub transform: ProjectiveTransform,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<Point>,
    pub translated: bool,
}

// A node of the graph a scene keeps a record of: a layer with its subtree,
// or a node drawn in its parent's layer.
#[derive(Clone, Copy)]
pub(crate) enum Subtree<'a> {
    Layer(LayerRef<'a>),
    Inline(LayerRef<'a>),
}

impl<'a> Subtree<'a> {
    // The node `child`, an item of a record, draws, when it draws one.
    pub(crate) fn of_child(child: RecordItem<'a>) -> Option<Self> {
        match child {
            RecordItem::Layer(layer) => Some(Self::Layer(layer)),
            RecordItem::Leaf(leaf) => Some(Self::Inline(leaf)),
            RecordItem::Draw(_) | RecordItem::Text(_) | RecordItem::Run(_) => None,
        }
    }

    // The node and the node whose outer draws it wraps.
    pub(crate) fn identity(self) -> (Option<NodeId>, Option<NodeId>) {
        match self {
            Self::Layer(layer) => (layer.node_id(), layer.wraps()),
            Self::Inline(leaf) => (leaf.owner(), None),
        }
    }

    // What the subtree was at its last change, its placement left out.
    pub(crate) fn change(self) -> RecordChange {
        match self {
            Self::Layer(layer) | Self::Inline(layer) => layer.change(),
        }
    }

    // What the node itself was at its last change, which a change beneath
    // it leaves.
    pub(crate) fn own_change(self) -> RecordChange {
        match self {
            Self::Layer(layer) => layer.own_change(),
            Self::Inline(leaf) => leaf.change(),
        }
    }

    // The node's placement in its parent's layer.
    pub(crate) fn transform_to_parent(self) -> ProjectiveTransform {
        match self {
            Self::Layer(layer) | Self::Inline(layer) => layer.transform_to_parent(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SubtreeItems {
    /// The layer's node and the node whose outer draws it wraps.
    pub identity: (Option<NodeId>, Option<NodeId>),
    /// The layer's change number, which covers its subtree, when it pushed
    /// the items.
    pub change: RecordChange,
    /// The layer's own change number, which a change beneath it leaves.
    pub own_change: RecordChange,
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
    pub(crate) kept: Vec<Range<usize>>,
    // The snap anchor of the layer whose draws push now, which their items
    // take.
    pub(crate) snap_anchor: Option<Point>,
    // While a changed subtree pushes again over its old items: where.
    pub(crate) rewrite: Option<Box<Rewrite>>,
}

#[expect(clippy::multiple_inherent_impl, reason = "item reach in draw/reach")]
impl RasterScene {
    pub(crate) fn new() -> Self {
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
            snap_anchor: None,
            rewrite: None,
        }
    }

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
        if let Some(rewrite) = &self.rewrite {
            return rewrite.counts();
        }
        ItemCounts {
            order: self.order.len(),
            shapes: self.shapes.len(),
            images: self.images.len(),
            texts: self.texts.len(),
            shadows: self.shadows.len(),
        }
    }

    // The item `item` of the draw order stands for.
    pub(crate) fn item(&self, item: RenderItem) -> ItemRef<'_> {
        match item {
            RenderItem::Shape(index) => ItemRef::Shape(&self.shapes[index]),
            RenderItem::Image(index) => ItemRef::Image(&self.images[index]),
            RenderItem::Text(index) => ItemRef::Text(&self.texts[index]),
            RenderItem::Shadow(index) => ItemRef::Shadow(&self.shadows[index]),
        }
    }

    pub(crate) fn open_subtree(&mut self, subtree: Subtree<'_>) -> usize {
        let counts = self.counts();
        let record = SubtreeItems {
            identity: subtree.identity(),
            change: subtree.change(),
            own_change: subtree.own_change(),
            transform: subtree.transform_to_parent(),
            passed_down: None,
            end: 0,
            start: counts,
            stop: counts,
        };
        if let Some(rewrite) = self.rewrite.as_deref_mut() {
            return rewrite.place_record(&mut self.subtrees, record);
        }
        let index = self.subtrees.len();
        self.subtrees.push(SubtreeItems {
            end: index + 1,
            ..record
        });
        index
    }

    pub(crate) fn note_passed_down(&mut self, index: usize, passed_down: PassedDown) {
        let at = match self.rewrite.as_deref_mut() {
            Some(rewrite) => rewrite.place_passed_down(&mut self.passed_down, passed_down),
            None => {
                self.passed_down.push(passed_down);
                self.passed_down.len() - 1
            }
        };
        if let Some(record) = self.record_mut(index) {
            record.passed_down = Some(at);
        }
    }

    pub(crate) fn close_subtree(&mut self, index: usize) {
        let end = self
            .rewrite
            .as_deref()
            .map_or(self.subtrees.len(), Rewrite::record_count);
        let stop = self.counts();
        if let Some(record) = self.record_mut(index) {
            record.end = end;
            record.stop = stop;
        }
    }

    // The record `index`, wherever a rewrite put it.
    fn record_mut(&mut self, index: usize) -> Option<&mut SubtreeItems> {
        match self.rewrite.as_deref_mut() {
            Some(rewrite) => rewrite.record_mut(&mut self.subtrees, index),
            None => self.subtrees.get_mut(index),
        }
    }

    pub(crate) fn copy_subtrees(&mut self, old: &Self, records: Range<usize>, stop: ItemCounts) {
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

    pub(crate) fn subtree_span(&self, record: usize) -> Option<(SubtreeExtent, usize)> {
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

    pub(crate) fn push_draw_shape(&mut self, shape: DrawShape) {
        self.push_item(shape);
    }

    // Queues `item` to draw over everything queued before, snapped as the
    // layer whose draws push now; while a subtree is rewritten, in the
    // place it takes there.
    pub(crate) fn push_item<T: SceneItem>(&mut self, mut item: T) {
        if let Some(anchor) = self.snap_anchor {
            item.snap_to(anchor);
        }
        if let Some(mut rewrite) = self.rewrite.take() {
            rewrite.place(self, item);
            self.rewrite = Some(rewrite);
            return;
        }
        let (list, order) = T::lists(self);
        order.push(T::item(list.len()));
        list.push(item);
    }

    pub(crate) fn push_shape(
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
            outline: Outline::Fill(shape),
            clip,
            blend_mode,
        });
    }

    pub(crate) fn push_shadow(&mut self, shadow: ShadowRRect, color: Color, clip: Option<Rect>) {
        self.push_item(ShadowDraw {
            shadow,
            snap_anchor: None,
            color,
            clip,
        });
    }

    pub(crate) fn push_pixel_snapped_shape(
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
            outline: Outline::Fill(shape),
            clip,
            blend_mode,
        });
    }

    pub(crate) fn push_shape_params(&mut self, params: ShapeDrawParams) {
        self.push_draw_shape(DrawShape::of_params(params));
    }

    pub(crate) fn push_image(&mut self, draw: ImageDraw) {
        self.push_item(ImageDraw {
            alpha: draw.alpha.clamp(0.0, 1.0),
            ..draw
        });
    }

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
        self.push_item(ImageDraw {
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

    pub(crate) fn push_text(&mut self, draw: TextDraw) {
        self.push_item(draw);
    }
}

impl Default for RasterScene {
    fn default() -> Self {
        Self::new()
    }
}
