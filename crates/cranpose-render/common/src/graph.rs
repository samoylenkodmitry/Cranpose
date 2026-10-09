use std::{
    cell::Cell,
    mem::size_of,
    ops::{Deref, DerefMut, Range},
    rc::Rc,
};

use cranpose_core::{NodeId, collections::map::HashSet};
use cranpose_ui::{
    GraphicsLayer, ModifierNodeSlices, Point, Rect, RenderEffect, TextLayoutOptions, TextOverflow,
    TextStyle,
    text::{AnnotatedString, RenderString, SpanStyle, TextDrawStyle},
};
pub use cranpose_ui_graphics::transform::{ProjectiveTransform, quad_bounds};
use cranpose_ui_graphics::{
    BlendMode, ColorFilter, CommandRecording, DrawPrimitive, RecordingSummary, ShadowPrimitive,
};

use crate::{
    raster_cache::LayerRasterCacheHashes,
    style_shared::DrawPlacement,
    text_paint::{TextPaint, TextPaintCache},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsolationReasons {
    pub explicit_offscreen: bool,
    pub shape_clip: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
}

impl IsolationReasons {
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.shape_clip
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    #[default]
    None,
    Auto,
}

#[derive(Clone)]
pub struct HitTestNode {
    /// The node's modifier slices, shared rather than copied: they hold its
    /// pointer inputs and the pointer icon it asks for while hovered. A node
    /// that only names an icon is still a hit target, which is how a
    /// decorative panel carries a cursor without handling clicks.
    pub handlers: Rc<ModifierNodeSlices>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawPrimitiveNode {
    pub primitive: DrawPrimitive,
    pub clip: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitiveNode {
    pub node_id: NodeId,
    pub rect: Rect,
    /// Shared so the renderer can hand the same allocation to every draw it
    /// emits for this node instead of deep-copying the string once per emit.
    pub text: Rc<AnnotatedString>,
    /// `text` as the renderer's draws carry it across threads, shared with
    /// the prepared layout it came from so no frame converts it again.
    pub render_text: std::sync::Arc<RenderString>,
    /// Shared so every frame's draws of this text hand over the same style
    /// instead of copying it.
    pub text_style: std::sync::Arc<TextStyle>,
    pub font_size: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
    /// What a renderer reads of `text_style` and `text`'s spans to draw the
    /// glyphs, so that drawing a plain text does not load the style.
    pub paint: TextPaintCache,
}

impl TextPrimitiveNode {
    /// The paint of the node's glyphs.
    pub fn paint(&self) -> TextPaint {
        self.paint.get_or(|| {
            TextPaint::of(
                &self.text_style,
                self.text_style.render_hash(),
                self.render_text.span_styles(),
                self.font_size,
            )
        })
    }

    /// Conservative bounds for the emitted glyphs and paint. Returns `None`
    /// when an unclipped layout or style can draw beyond its layout rectangle.
    pub fn draw_bounds(&self) -> Option<Rect> {
        if let Some(clip) = self.clip {
            return Some(clip);
        }
        let style = &self.text_style.span_style;
        let stroked =
            |style: &SpanStyle| matches!(style.draw_style, Some(TextDrawStyle::Stroke { .. }));
        (!matches!(self.layout_options.overflow, TextOverflow::Visible)
            && style.shadow.is_none()
            && style
                .baseline_shift
                .is_none_or(|shift| !shift.is_specified() || shift.0 == 0.0)
            && !stroked(style)
            && self
                .text
                .span_styles
                .iter()
                .all(|span| !stroked(&span.item)))
        .then_some(self.rect)
    }

    fn draws_within(&self, bounds: Rect) -> bool {
        self.draw_bounds()
            .is_some_and(|drawn| rect_within(drawn, bounds))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitivePhase {
    BeforeChildren,
    AfterChildren,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    Draw(Box<DrawPrimitiveNode>),
    Text(Box<TextPrimitiveNode>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveEntry {
    pub phase: PrimitivePhase,
    pub node: PrimitiveNode,
}

/// Scene properties with implicit defaults and independently mutable clones.
#[derive(Clone, Debug, Default)]
pub struct LayerProperties(Option<Box<GraphicsLayer>>);

impl LayerProperties {
    pub(crate) fn replace(&mut self, value: Option<GraphicsLayer>) {
        match (&mut self.0, value) {
            (Some(current), Some(value)) => **current = value,
            (storage, value) => *storage = value.map(Box::new),
        }
    }
}

impl Deref for LayerProperties {
    type Target = GraphicsLayer;

    fn deref(&self) -> &Self::Target {
        self.0.as_deref().unwrap_or(&GraphicsLayer::DEFAULT)
    }
}

impl DerefMut for LayerProperties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.get_or_insert_with(Box::default)
    }
}

impl From<GraphicsLayer> for LayerProperties {
    fn from(value: GraphicsLayer) -> Self {
        Self(Some(Box::new(value)))
    }
}

impl From<Option<GraphicsLayer>> for LayerProperties {
    fn from(value: Option<GraphicsLayer>) -> Self {
        Self(value.map(Box::new))
    }
}

impl PartialEq for LayerProperties {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

#[derive(Clone)]
pub struct LayerNode {
    pub node_id: Option<NodeId>,
    /// Set on the synthetic layer that holds a node's outer draws, those chained
    /// before its graphics layer, around that node's own layer. Scene updates
    /// address the node through this id because the node's layer has none of the
    /// outer draws and the wrapper has no node id of its own.
    pub wraps: Option<NodeId>,
    /// The bounds of the node's layer, where its clip cuts, its transforms
    /// pivot and an offscreen pass draws it.
    pub local_bounds: Rect,
    /// The node's own rect, `(0, 0, size)`, where its pointer handlers are,
    /// when it differs from `local_bounds`: a clip or graphics layer that
    /// wraps a coordinator a later padding or offset moves bounds the layer
    /// there instead, as in Compose. `None` when the two agree.
    pub node_bounds: Option<Rect>,
    pub transform_to_parent: ProjectiveTransform,
    pub content_offset: Point,
    pub motion_context_animated: bool,
    pub translated_content_context: bool,
    pub translated_content_offset: Point,
    /// Where layout placed this layer within its parent's content, before the
    /// parent's content offset. Scene updates add these up from the root, with
    /// each layer's content offset and graphics-layer translation, to find the
    /// window origin of a subtree they rebuild.
    pub origin_in_parent: Point,
    pub graphics_layer: LayerProperties,
    pub clip_to_bounds: bool,
    pub hit_test: Option<HitTestNode>,
    pub has_hit_targets: bool,
    /// Whether this subtree publishes live window origins (a text field's
    /// popup anchor, a scroll container's viewport rect). Those sinks are
    /// written during a full lowering, so the scroll fast path may translate
    /// a retained subtree in place only when this is false.
    pub has_origin_sinks: bool,
    /// Whether everything this layer and its subtree draw lies within its
    /// `local_bounds`, give or take [`CONTAINED_DRAW_SLACK`], so a renderer
    /// may skip the whole subtree where those bounds are clipped away. Scene
    /// building keeps it current; `false` promises nothing.
    pub draws_within_bounds: bool,
    pub isolation: IsolationReasons,
    pub cache_policy: CachePolicy,
    /// The raster cache hashes, computed the first time a renderer asks and
    /// kept until the layer or a layer beneath it changes.
    pub cache_hashes: Cell<Option<LayerRasterCacheHashes>>,
    /// The scene updates that changed what the layer draws.
    pub content_changes: ContentChanges,
    pub children: Vec<RenderNode>,
}

/// The last scene update that changed what a layer draws, and how many
/// updates in a row up to it did.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentChanges {
    last_update: u64,
    run: u32,
}

impl ContentChanges {
    fn note(&mut self, update: u64) {
        if self.run > 0 && self.last_update == update {
            return;
        }
        self.run = if self.run > 0 && self.last_update + 1 == update {
            self.run + 1
        } else {
            1
        };
        self.last_update = update;
    }
}

impl Default for LayerNode {
    fn default() -> Self {
        Self {
            node_id: None,
            wraps: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            node_bounds: None,
            transform_to_parent: ProjectiveTransform::identity(),
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            origin_in_parent: Point::default(),
            graphics_layer: LayerProperties::default(),
            clip_to_bounds: false,
            hit_test: None,
            has_hit_targets: false,
            has_origin_sinks: false,
            draws_within_bounds: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: Cell::new(None),
            content_changes: ContentChanges::default(),
            children: Vec::new(),
        }
    }
}

/// How far past its bounds a layer that draws within them may still put
/// pixels: glyph and edge antialiasing.
pub const CONTAINED_DRAW_SLACK: f32 = 1.0;

impl LayerNode {
    /// Every string this layer and the layers below it paint, in draw order.
    pub fn painted_text(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_painted_text(&mut out);
        out
    }

    fn collect_painted_text(&self, out: &mut Vec<String>) {
        for child in &self.children {
            match child {
                RenderNode::Primitive(primitive) => {
                    if let PrimitiveNode::Text(text) = &primitive.node {
                        out.push(text.text.text.clone());
                    }
                }
                RenderNode::Layer(child) => child.collect_painted_text(out),
                RenderNode::DrawRun(run) => {
                    for primitive in run.primitives() {
                        if let DrawPrimitive::Text(text) = primitive {
                            out.push(text.text.to_string());
                        }
                    }
                }
            }
        }
    }

    /// Whether this layer's content, as [`LayerNode::draws_within_bounds`]
    /// describes it, stays within its bounds: it clips to them, or its draws,
    /// its texts and its children placed where they are all fit inside.
    pub fn content_draws_within_bounds(&self) -> bool {
        if self.visual_clip_rect().is_some() {
            return true;
        }
        let bounds = inflate_rect(self.local_bounds, CONTAINED_DRAW_SLACK);
        self.children.iter().all(|child| match child {
            RenderNode::DrawRun(run) => {
                !run.summary.has_shadow
                    && run
                        .recording
                        .bounds()
                        .is_none_or(|drawn| rect_within(drawn, bounds))
            }
            RenderNode::Primitive(entry) => match &entry.node {
                PrimitiveNode::Text(text) => text.draws_within(bounds),
                PrimitiveNode::Draw(_) => false,
            },
            RenderNode::Layer(layer) => layer.draws_within_parent(bounds),
        })
    }

    pub(crate) fn refresh_child_facts(&mut self) {
        self.has_hit_targets = self.hit_test.is_some()
            || self.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
            });
        self.draws_within_bounds = self.content_draws_within_bounds();
    }

    /// Whether this layer, drawn as a child, puts nothing outside `bounds`:
    /// it draws within its own bounds, casts no shadow, applies no effect,
    /// and its bounds placed in its parent lie inside.
    fn draws_within_parent(&self, bounds: Rect) -> bool {
        self.draws_within_bounds
            && self.graphics_layer.shadow_elevation <= 0.0
            && self.effect().is_none()
            && self.backdrop().is_none()
            && rect_within(
                quad_bounds(
                    self.transform_to_parent
                        .map_rect(inflate_rect(self.local_bounds, CONTAINED_DRAW_SLACK)),
                ),
                bounds,
            )
    }

    /// The node's own rect, where its pointer handlers are.
    pub fn node_rect(&self) -> Rect {
        self.node_bounds.unwrap_or(self.local_bounds)
    }

    pub fn clip_rect(&self) -> Option<Rect> {
        (self.clip_to_bounds || self.graphics_layer.clip).then_some(self.local_bounds)
    }

    /// Where the layer's own shadow is cut: its bounds when it clips its
    /// content to them. A graphics-layer clip leaves the shadow whole.
    pub fn shadow_clip(&self) -> Option<Rect> {
        self.clip_to_bounds.then_some(self.local_bounds)
    }

    /// Where the layer's drawing is cut: its clip or, when it composites
    /// through an offscreen buffer for its alpha or by request, its bounds,
    /// as Compose's layer-sized buffer cuts it. Hit testing takes only
    /// [`Self::clip_rect`]: alpha hides nothing from a finger.
    pub fn visual_clip_rect(&self) -> Option<Rect> {
        self.clip_rect().or_else(|| {
            (self.isolation.group_opacity || self.isolation.explicit_offscreen)
                .then_some(self.local_bounds)
        })
    }

    pub fn effect(&self) -> Option<&RenderEffect> {
        self.graphics_layer.render_effect.as_ref()
    }

    pub fn backdrop(&self) -> Option<&RenderEffect> {
        self.graphics_layer.backdrop_effect.as_ref()
    }

    pub fn opacity(&self) -> f32 {
        self.graphics_layer.alpha
    }

    pub fn blend_mode(&self) -> BlendMode {
        self.graphics_layer.blend_mode
    }

    pub fn color_filter(&self) -> Option<ColorFilter> {
        self.graphics_layer.color_filter
    }

    pub fn target_content_hash(&self) -> u64 {
        self.raster_cache_hashes().target_content
    }

    pub fn motion_source_content_hash(&self) -> u64 {
        crate::graph_hash::layer_motion_source_content_hash(self)
    }

    pub fn effect_hash(&self) -> u64 {
        self.raster_cache_hashes().effect
    }

    fn raster_cache_hashes(&self) -> LayerRasterCacheHashes {
        if let Some(hashes) = self.cache_hashes.get() {
            return hashes;
        }
        let hashes = crate::graph_hash::layer_raster_cache_hashes(self);
        self.cache_hashes.set(Some(hashes));
        hashes
    }

    /// Forgets the raster cache hashes of this layer and every layer beneath
    /// it, after a change the scene builder did not make: they are computed
    /// again when asked for.
    pub fn forget_raster_cache_hashes(&mut self) {
        self.cache_hashes.set(None);
        for child in &mut self.children {
            if let RenderNode::Layer(child) = child {
                child.forget_raster_cache_hashes();
            }
        }
    }

    /// Forgets this layer's own raster cache hashes: a property its hash
    /// covers, or a layer beneath it, changed.
    pub(crate) fn forget_own_raster_cache_hashes(&mut self) {
        self.cache_hashes.set(None);
    }

    /// Records that scene update `update` changed what this layer draws, and
    /// forgets its hashes.
    pub(crate) fn note_content_change(&mut self, update: u64) {
        self.cache_hashes.set(None);
        self.content_changes.note(update);
    }

    /// Whether what this layer draws changed in scene update `update` and in
    /// the one before it. A raster cache keyed on its content would miss
    /// again on the next frame, so a renderer neither hashes nor caches it.
    pub fn content_churns(&self, update: u64) -> bool {
        self.content_changes.run >= 2 && self.content_changes.last_update == update
    }
}

#[derive(Clone)]
pub enum RenderNode {
    Primitive(PrimitiveEntry),
    /// A whole draw command's primitives as one node. A heavy canvas records
    /// thousands of primitives per frame; wrapping each in its own
    /// [`RenderNode`] made the graph rebuild move every one of them twice and
    /// free seventeen thousand nodes per frame on a stress scene. The run
    /// keeps the recorded vector intact — semantically it is exactly that
    /// many consecutive `Primitive` draw entries with no per-primitive clip.
    DrawRun(DrawRunNode),
    Layer(Box<LayerNode>),
}

/// Stable identity of one placement of a draw command: its layout node,
/// command index and placement pass. A `WithContent` command shares one CPU
/// recording between its two runs, while each placement retains independent
/// renderer state under this identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawCommandId {
    pub node_id: NodeId,
    pub command_index: u32,
    pub placement: DrawPlacement,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawRunNode {
    pub phase: PrimitivePhase,
    /// Which draw command recorded these primitives. `None` only for runs
    /// with no per-command provenance (hand-built tests).
    pub command: Option<DrawCommandId>,
    /// The recording registry keeps a handle per layout node and command,
    /// so this recording's buffers survive the draw run being dropped and
    /// the command can record into them on a later update. Nothing mutates
    /// a recording after construction, which is
    /// what makes sharing sound.
    pub recording: Rc<CommandRecording>,
    /// The segments of the recording this run draws: one placement's part
    /// of a with-content command, or the whole recording.
    pub segments: Range<u32>,
    /// Content facts consumers keep asking per frame, answered while
    /// recording and never by rescanning.
    pub summary: DrawRunSummary,
}

/// What a run contains, answered while recording.
pub type DrawRunSummary = RecordingSummary;

impl DrawRunNode {
    pub fn new(phase: PrimitivePhase, primitives: Vec<DrawPrimitive>) -> Self {
        Self::for_command(phase, None, primitives)
    }

    pub fn for_command(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: Vec<DrawPrimitive>,
    ) -> Self {
        let recording = CommandRecording::from_primitives(primitives);
        let segments = recording.all_segments();
        Self::for_command_shared(phase, command, Rc::new(recording), segments)
    }

    pub fn for_command_shared(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        recording: Rc<CommandRecording>,
        segments: Range<u32>,
    ) -> Self {
        let summary = recording.summary_in(&segments);
        Self {
            phase,
            command,
            recording,
            segments,
            summary,
        }
    }

    /// The run's primitives, materialised from the recording in order.
    pub fn primitives(&self) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.recording.primitives(self.segments.clone())
    }

    /// The rect every entry of the run can reach.
    pub fn coverage_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.recording.coverage_rects(self.segments.clone())
    }

    pub fn len(&self) -> usize {
        self.recording.len_in(&self.segments)
    }

    pub fn is_empty(&self) -> bool {
        self.recording.is_empty_in(&self.segments)
    }
}

#[derive(Clone)]
pub struct RenderGraph {
    pub root: LayerNode,
    /// How many scene updates the graph took: the number of the latest.
    pub update: u64,
}

impl RenderGraph {
    pub fn new(root: LayerNode) -> Self {
        Self { root, update: 0 }
    }

    pub fn node_count(&self) -> usize {
        fn count_layer(layer: &LayerNode) -> usize {
            1 + layer
                .children
                .iter()
                .map(|child| match child {
                    RenderNode::Primitive(_) => 1,
                    RenderNode::DrawRun(run) => run.len(),
                    RenderNode::Layer(child_layer) => count_layer(child_layer),
                })
                .sum::<usize>()
        }

        count_layer(&self.root)
    }

    pub fn heap_bytes(&self) -> usize {
        layer_heap_bytes(&self.root)
    }

    /// Replaces the set with the graph's visual observation owners, retaining its capacity.
    pub fn collect_retained_visual_observation_nodes(&self, nodes: &mut HashSet<NodeId>) {
        fn collect(layer: &LayerNode, nodes: &mut HashSet<NodeId>) {
            if let Some(node_id) = layer.node_id {
                nodes.insert(node_id);
            }
            for child in &layer.children {
                match child {
                    RenderNode::DrawRun(run) => {
                        if let Some(command) = run.command {
                            nodes.insert(command.node_id);
                        }
                    }
                    RenderNode::Layer(child) => collect(child, nodes),
                    RenderNode::Primitive(_) => {}
                }
            }
        }

        nodes.clear();
        collect(&self.root, nodes);
    }
}

fn layer_heap_bytes(layer: &LayerNode) -> usize {
    size_of::<RenderNode>() * layer.children.capacity()
        + layer
            .graphics_layer
            .0
            .as_ref()
            .map_or(0, |_| size_of::<GraphicsLayer>())
        + layer
            .children
            .iter()
            .map(render_node_heap_bytes)
            .sum::<usize>()
}

fn render_node_heap_bytes(node: &RenderNode) -> usize {
    match node {
        RenderNode::Primitive(entry) => primitive_entry_heap_bytes(entry),
        RenderNode::DrawRun(run) => {
            run.recording.pod_heap_bytes()
                + std::mem::size_of_val(run.recording.others())
                + run
                    .recording
                    .others()
                    .iter()
                    .map(draw_primitive_heap_bytes)
                    .sum::<usize>()
        }
        RenderNode::Layer(layer) => size_of::<LayerNode>() + layer_heap_bytes(layer),
    }
}

fn primitive_entry_heap_bytes(entry: &PrimitiveEntry) -> usize {
    match &entry.node {
        PrimitiveNode::Draw(draw) => {
            size_of::<DrawPrimitiveNode>() + draw_primitive_heap_bytes(&draw.primitive)
        }
        PrimitiveNode::Text(text) => {
            size_of::<TextPrimitiveNode>() + annotated_string_heap_bytes(&text.text)
        }
    }
}

fn draw_primitive_heap_bytes(primitive: &DrawPrimitive) -> usize {
    match primitive {
        DrawPrimitive::Content
        | DrawPrimitive::Rect { .. }
        | DrawPrimitive::RoundRect { .. }
        | DrawPrimitive::Arc { .. }
        | DrawPrimitive::Line { .. }
        | DrawPrimitive::Trapezoid { .. } => 0,
        DrawPrimitive::Blend { primitive, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(primitive)
        }
        DrawPrimitive::Image { .. } => 0,
        DrawPrimitive::Text(text) => {
            size_of::<cranpose_ui_graphics::TextPrimitive>()
                + text.text.len()
                + text
                    .style
                    .font_family
                    .as_ref()
                    .map_or(0, std::string::String::capacity)
        }
        DrawPrimitive::Shadow(shadow) => shadow_primitive_heap_bytes(shadow),
    }
}

fn shadow_primitive_heap_bytes(shadow: &ShadowPrimitive) -> usize {
    match shadow {
        ShadowPrimitive::Drop { shape, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(shape)
        }
        ShadowPrimitive::Inner { fill, cutout, .. } => {
            size_of::<DrawPrimitive>() * 2
                + draw_primitive_heap_bytes(fill)
                + draw_primitive_heap_bytes(cutout)
        }
    }
}

fn annotated_string_heap_bytes(text: &AnnotatedString) -> usize {
    text.text.capacity()
        + text.span_styles.capacity() * size_of::<usize>() * 2
        + text.paragraph_styles.capacity() * size_of::<usize>() * 2
        + text.string_annotations.capacity() * size_of::<usize>() * 2
        + text.link_annotations.capacity() * size_of::<usize>() * 2
        + text
            .string_annotations
            .iter()
            .map(|annotation| {
                annotation.item.tag.capacity() + annotation.item.annotation.capacity()
            })
            .sum::<usize>()
        + text
            .link_annotations
            .iter()
            .map(|annotation| match &annotation.item {
                cranpose_ui::text::LinkAnnotation::Url(url) => url.capacity(),
                cranpose_ui::text::LinkAnnotation::Clickable { tag, .. } => tag.capacity(),
            })
            .sum::<usize>()
}

fn inflate_rect(rect: Rect, by: f32) -> Rect {
    Rect {
        x: rect.x - by,
        y: rect.y - by,
        width: rect.width + by * 2.0,
        height: rect.height + by * 2.0,
    }
}

fn rect_within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[cfg(test)]
#[path = "tests/graph_tests.rs"]
mod tests;
