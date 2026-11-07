use std::{ops::Range, rc::Rc, sync::Arc};

use cranpose_core::NodeId;
use cranpose_render_common::{
    glyph_reach::{InkSpread, glyph_part},
    graph::{HitTestNode, InlineNode, LayerNode, ProjectiveTransform, RenderNode},
    layer_shadow::{ShadowLight, ShadowRRect},
    primitive_emit::ShapeDrawParams,
    scene_builder::KeptHead,
};
use cranpose_ui::TextStyle;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, LineGeometry,
    Point, Rect, RoundedCornerShape, Size, Stroke, Trapezoid,
};

use crate::{
    draw::RenderItem,
    rewrite::Rewrite,
    scene_item::{ItemRef, SceneItem},
};

mod item_counts;
pub(crate) use item_counts::{ItemCounts, PackedCounts};

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
            outline: self.outline.translated(dx, dy),
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
    pub text: Rc<cranpose_ui::text::AnnotatedString>,
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
    // The part of `rect`, where the text draws from its top left corner,
    // in which its glyphs and their shadows draw.
    pub(crate) fn glyph_reach(&self, rect: Rect) -> Rect {
        let spread = InkSpread::of_text(&self.text_style, &self.text.span_styles, &self.layout);
        glyph_part(rect, &self.layout, self.scale, spread)
    }

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

/// How much of a scene a layer's subtree holds: its records, its items of
/// each kind, and the entries of what its layers passed down. Subtrees of
/// one extent fit each other's places wherever each lies in its scene.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubtreeExtent {
    pub records: usize,
    pub items: ItemCounts,
    pub passed_down: usize,
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

impl PassedDown {
    // What a window's root passes down at `scale`: its map to device pixels,
    // and no style, clip or snap anchor.
    pub(crate) fn root(scale: f32) -> Self {
        Self {
            transform: ProjectiveTransform::uniform_scale(scale),
            alpha: 1.0,
            color_filter: None,
            clip: None,
            snap_anchor: None,
            translated: false,
        }
    }
}

// A node of the graph a scene keeps a record of: a layer with its subtree,
// or a node drawn in its parent's layer.
#[derive(Clone, Copy)]
pub(crate) enum Subtree<'a> {
    Layer(&'a LayerNode),
    Inline(&'a InlineNode),
}

impl<'a> Subtree<'a> {
    // The node `child`, a child of a layer, draws, when it draws one.
    pub(crate) fn of_child(child: &'a RenderNode) -> Option<Self> {
        match child {
            RenderNode::Layer(layer) => Some(Self::Layer(layer)),
            RenderNode::Inline(inline) => Some(Self::Inline(inline)),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
        }
    }

    // The node and the node whose outer draws it wraps.
    pub(crate) fn identity(self) -> (Option<NodeId>, Option<NodeId>) {
        match self {
            Self::Layer(layer) => (layer.node_id, layer.wraps),
            Self::Inline(inline) => (Some(inline.node_id), None),
        }
    }

    // The size of the node's own rect.
    pub(crate) fn node_size(self) -> Size {
        let rect = match self {
            Self::Layer(layer) => layer.node_rect(),
            Self::Inline(inline) => inline.node_rect(),
        };
        Size {
            width: rect.width,
            height: rect.height,
        }
    }

    // The node's layer, when it cuts the hit targets beneath it or handles
    // a pointer.
    pub(crate) fn hit_layer(self) -> Option<&'a LayerNode> {
        let Self::Layer(layer) = self else {
            return None;
        };
        (layer.clip_rect().is_some() || layer.hit_test.is_some()).then_some(layer)
    }

    // A record of the node whose items start at `counts`, before the record
    // `end`, which closing it moves past its descendants', with what the hit
    // targets beneath take from `hits`, its `hit_layer`.
    pub(crate) fn record(
        self,
        end: usize,
        counts: ItemCounts,
        hits: Option<&LayerNode>,
    ) -> SubtreeItems {
        SubtreeItems {
            identity: self.identity(),
            transform: self.transform_to_parent(),
            node_size: self.node_size(),
            hits: hits.map(|layer| Box::new(HitLayer::of(layer))),
            passed_down: None,
            end,
            start: counts.into(),
            stop: counts.into(),
            head: None,
        }
    }

    // The node's placement in its parent's layer.
    pub(crate) fn transform_to_parent(self) -> ProjectiveTransform {
        match self {
            Self::Layer(layer) => layer.transform_to_parent,
            Self::Inline(inline) => inline.transform_to_parent(),
        }
    }
}

// What the hit targets beneath a layer take from it: its bounds, whether
// it cuts them, and its node's own pointer handling.
#[derive(Clone)]
pub(crate) struct HitLayer {
    pub local_bounds: Rect,
    pub node_bounds: Option<Rect>,
    pub clips: bool,
    pub hit_test: Option<HitTestNode>,
}

impl HitLayer {
    // What the hit targets beneath `layer` take from it.
    pub(crate) fn of(layer: &LayerNode) -> Self {
        Self {
            local_bounds: layer.local_bounds,
            node_bounds: layer.node_bounds,
            clips: layer.clip_rect().is_some(),
            hit_test: layer.hit_test.clone(),
        }
    }

    // Takes what the hit targets beneath `layer` take from it, in place.
    pub(crate) fn take_from(&mut self, layer: &LayerNode) {
        self.local_bounds = layer.local_bounds;
        self.node_bounds = layer.node_bounds;
        self.clips = layer.clip_rect().is_some();
        self.hit_test.clone_from(&layer.hit_test);
    }
}

/// The items a layer's subtree pushed into a scene, in a run of each list,
/// so the next scene can copy them when nothing they depend on changed.
pub(crate) struct SubtreeItems {
    /// The layer's node and the node whose outer draws it wraps.
    pub identity: (Option<NodeId>, Option<NodeId>),
    /// The layer's placement in its parent.
    pub transform: ProjectiveTransform,
    // The size of the node's own rect, from the layer's origin.
    pub node_size: Size,
    // What the hit targets beneath a layer that cuts them or handles a
    // pointer take from it; `None` for a layer that does neither and for a
    // node drawn in its parent's layer.
    pub hits: Option<Box<HitLayer>>,
    /// Where [`RasterScene::passed_down`] holds what the layer passed to its
    /// child layers, when it has any.
    pub passed_down: Option<usize>,
    /// The index of the first record after the subtree's, its descendants'
    /// included.
    pub end: usize,
    pub start: PackedCounts,
    pub stop: PackedCounts,
    // The node lowered alone, kept after an update pushed it, for the next
    // updates to push again while the node does not change.
    pub head: Option<KeptHead>,
}

impl SubtreeItems {
    // A copy of the record for another scene, which keeps no head.
    fn copied(&self) -> Self {
        Self {
            identity: self.identity,
            transform: self.transform,
            node_size: self.node_size,
            hits: self.hits.clone(),
            passed_down: self.passed_down,
            end: self.end,
            start: self.start,
            stop: self.stop,
            head: None,
        }
    }

    // The layout node of the record: its layer's or inline node's, or the
    // node whose outer draws it wraps. `None` for a draw group.
    pub(crate) fn node(&self) -> Option<NodeId> {
        self.identity.0.or(self.identity.1)
    }
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
    // The snap anchor of the layer whose draws push now, which their items
    // take.
    pub(crate) snap_anchor: Option<Point>,
    // While a changed subtree pushes again over its old items: where.
    pub(crate) rewrite: Option<Box<Rewrite>>,
    // Whether a record that hit targets read was pushed, replaced or let go
    // since the scene's hit targets were collected.
    pub(crate) hits_changed: bool,
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
            hits_changed: false,
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

    /// Opens the record of `subtree`, whose items come next, and returns its
    /// index for [`Self::close_subtree`].
    pub(crate) fn open_subtree(&mut self, subtree: Subtree<'_>) -> usize {
        let hits = subtree.hit_layer();
        self.hits_changed |= hits.is_some();
        if let Some(rewrite) = self.rewrite.as_deref_mut() {
            return rewrite.place_record(&mut self.subtrees, (subtree, hits));
        }
        let index = self.subtrees.len();
        let counts = self.counts();
        self.subtrees.push(subtree.record(index + 1, counts, hits));
        index
    }

    // Notes in the record `index` what its layer passes down. While a
    // subtree is rewritten, returns whether that is what the old record of
    // the layer passed down; `None` otherwise.
    pub(crate) fn note_passed_down(
        &mut self,
        index: usize,
        passed_down: PassedDown,
    ) -> Option<bool> {
        let (at, same) = match self.rewrite.as_deref_mut() {
            Some(rewrite) => {
                let (at, same) = rewrite.place_passed_down(&mut self.passed_down, passed_down);
                (at, Some(same))
            }
            None => {
                self.passed_down.push(passed_down);
                (self.passed_down.len() - 1, None)
            }
        };
        if let Some(record) = self.record_mut(index) {
            record.passed_down = Some(at);
        }
        same
    }

    /// Closes the record `index`: the subtree pushed everything since it
    /// opened.
    pub(crate) fn close_subtree(&mut self, index: usize) {
        if self
            .rewrite
            .as_deref_mut()
            .is_some_and(Rewrite::closes_in_whole)
        {
            return;
        }
        let (end, stop) = match self.rewrite.as_deref() {
            Some(rewrite) => rewrite.record_end(),
            None => (self.subtrees.len(), self.counts()),
        };
        if let Some(record) = self.record_mut(index) {
            record.end = end;
            record.stop = stop.into();
        }
        if let Some(rewrite) = self.rewrite.as_deref_mut() {
            rewrite.close_record(&self.subtrees);
        }
    }

    // How many records the splices of the rewrite in progress added, less
    // those they removed; none without one.
    pub(crate) fn records_moved(&self) -> isize {
        self.rewrite.as_deref().map_or(0, Rewrite::records_moved)
    }

    // Readies the rewrite in progress to push the child whose old record is
    // `at` over its old places, as `Rewrite::rejoin` does. Returns where
    // that record now lies. The check that it lies where the rewrite pushes
    // next is read in line: a child mostly does.
    #[inline]
    pub(crate) fn rejoin(&mut self, at: usize) -> usize {
        match self.rewrite.as_deref() {
            Some(rewrite) if !rewrite.pushes_next_at(at) => self.rejoin_apart(at),
            _ => at,
        }
    }

    // What `rejoin` does for a child whose old record lies apart from where
    // the rewrite pushes next.
    #[cold]
    #[inline(never)]
    fn rejoin_apart(&mut self, at: usize) -> usize {
        let Some(mut rewrite) = self.rewrite.take() else {
            return at;
        };
        let at = rewrite.rejoin(self, at);
        self.rewrite = Some(rewrite);
        at
    }

    // The index of the record that opens next, wherever a rewrite puts it.
    #[inline]
    pub(crate) fn next_record(&self) -> usize {
        match self.rewrite.as_deref() {
            Some(rewrite) => rewrite.next_record(),
            None => self.subtrees.len(),
        }
    }

    // Keeps the old subtree whose record is `at` in the rewrite in progress,
    // as `Rewrite::keep` does. False without a rewrite.
    pub(crate) fn keep_old(&mut self, at: usize) -> bool {
        let Some(mut rewrite) = self.rewrite.take() else {
            return false;
        };
        rewrite.keep(self, at);
        self.rewrite = Some(rewrite);
        true
    }

    // Makes the next record to open the root of a whole subtree, as
    // `Rewrite::begin_whole` does. Nothing without a rewrite.
    pub(crate) fn begin_whole(&mut self) {
        if let Some(rewrite) = self.rewrite.as_deref_mut() {
            rewrite.begin_whole();
        }
    }

    // Lets go the head the old record `at` keeps, before its node lowers
    // again: the new draws record into the recordings the head held.
    pub(crate) fn drop_head(&mut self, at: usize) {
        if let Some(head) = self.take_head(at) {
            head.recycle_to_redraw();
        }
    }

    // Lets go every head the records keep, before a build lowers the
    // scene's nodes again: the build's nodes take the heads' boxes, and
    // their draws record into the heads' recordings.
    pub(crate) fn drop_heads(&mut self) {
        for head in self
            .subtrees
            .iter_mut()
            .filter_map(|record| record.head.take())
        {
            head.recycle_to_redraw();
        }
    }

    // Takes the head the old record `at` keeps.
    pub(crate) fn take_head(&mut self, at: usize) -> Option<KeptHead> {
        self.subtrees.get_mut(at)?.head.take()
    }

    // Keeps `head` with the record `index`, which its node's push opened.
    pub(crate) fn keep_head(&mut self, index: usize, head: KeptHead) {
        match self.record_mut(index) {
            Some(record) => {
                if let Some(stale) = record.head.replace(head) {
                    stale.recycle();
                }
            }
            None => head.recycle(),
        }
    }

    // The record `index`, wherever a rewrite put it.
    fn record_mut(&mut self, index: usize) -> Option<&mut SubtreeItems> {
        match self.rewrite.as_deref_mut() {
            Some(rewrite) => rewrite.record_mut(&mut self.subtrees, index),
            None => self.subtrees.get_mut(index),
        }
    }

    /// Pushes the items and records of the subtrees `old` recorded at
    /// `records`, siblings one after another, which pushed their items there
    /// up to `stop`.
    pub(crate) fn copy_subtrees(&mut self, old: &Self, records: Range<usize>, stop: ItemCounts) {
        let Some(from) = old
            .subtrees
            .get(records.start)
            .map(|first| first.start.get())
        else {
            return;
        };
        let to = self.counts();
        self.extend_items(old, (from, stop));
        let moved = |index: usize, from: usize, to: usize| to + index.saturating_sub(from);
        // A run that lands where it was in the old scene, as every run
        // before the first change does, keeps its indices.
        let base = self.subtrees.len();
        let first = records.start;
        let in_place = from == to && first == base;
        let order = old
            .order
            .get(from.order..stop.order.max(from.order))
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
            self.order
                .extend(order.iter().map(|item| item.moved(from, to)));
        }
        self.subtrees.extend(
            old.subtrees
                .get(records)
                .unwrap_or_default()
                .iter()
                .map(SubtreeItems::copied),
        );
        let passed_down = &mut self.passed_down;
        for record in self.subtrees.get_mut(base..).unwrap_or_default() {
            if !in_place {
                record.end = moved(record.end, first, base);
                record.start = record.start.get().moved(from, to).into();
                record.stop = record.stop.get().moved(from, to).into();
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

    // Pushes copies of the items, records and passed-down entries of the
    // subtree `old` holds at `records`, whose items end at `stop` and whose
    // entries start at `first_entry`, as a spill whose items, records and
    // entries would start at `to` in the scene it is spliced into.
    pub(crate) fn copy_subtrees_to(
        &mut self,
        old: &mut Self,
        records: Range<usize>,
        (stop, first_entry): (ItemCounts, usize),
        to: (ItemCounts, usize, usize),
    ) {
        let Some(from) = old
            .subtrees
            .get(records.start)
            .map(|first| first.start.get())
        else {
            return;
        };
        let (to_items, to_records, to_entries) = to;
        // A spill takes no rewrite: its counts are its lists' lengths.
        let base = self.counts();
        self.extend_items(old, (from, stop));
        self.order.extend(
            old.order[from.order..stop.order]
                .iter()
                .map(|item| item.moved(from, base)),
        );
        let moved = |index: usize, from: usize, to: usize| to + (index - from);
        let first = records.start;
        let mut entries = 0;
        // The old records give way to the copies when the spill is spliced
        // in: their heads go with the copies.
        for record in &mut old.subtrees[records] {
            entries += usize::from(record.passed_down.is_some());
            self.subtrees.push(SubtreeItems {
                end: moved(record.end, first, to_records),
                start: record.start.get().moved(from, to_items).into(),
                stop: record.stop.get().moved(from, to_items).into(),
                passed_down: record
                    .passed_down
                    .map(|at| moved(at, first_entry, to_entries)),
                head: record.head.take(),
                ..record.copied()
            });
        }
        self.passed_down
            .extend_from_slice(&old.passed_down[first_entry..first_entry + entries]);
    }

    // Appends the shapes, images, texts and shadows of `old` from `from` up
    // to `stop`, each list's own run.
    fn extend_items(&mut self, old: &Self, (from, stop): (ItemCounts, ItemCounts)) {
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
    }

    /// The extent of the subtree whose record is `record`, and the index of
    /// its first entry of what its layers passed down; `None` when its
    /// layers' entries of what they passed down do not lie in one run.
    pub(crate) fn subtree_span(&self, record: usize) -> Option<(SubtreeExtent, usize)> {
        let head = self.subtrees.get(record)?;
        let records = self.subtrees.get(record..head.end)?;
        let (mut first, mut last, mut count) = (None, None, 0);
        for at in records.iter().filter_map(|record| record.passed_down) {
            first.get_or_insert(at);
            last = Some(at);
            count += 1;
        }
        if let (Some(first), Some(last)) = (first, last)
            && last + 1 != first + count
        {
            return None;
        }
        let extent = SubtreeExtent {
            records: records.len(),
            items: head.stop.get().since(head.start.get()),
            passed_down: count,
        };
        Some((extent, first.unwrap_or(0)))
    }

    /// Queues `shape` to draw over everything queued before.
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
