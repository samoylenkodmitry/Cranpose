use std::{
    cell::Cell,
    ops::{Deref, DerefMut, Range},
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};

use cranpose_core::{NodeId, collections::map::HashSet};
use cranpose_ui::{
    GraphicsLayer, ModifierNodeSlices, Point, Rect, RenderEffect, TextOverflow, TextStyle,
    text::{AnnotatedString, RenderString, SpanStyle, TextDrawStyle},
};
pub use cranpose_ui_graphics::transform::{FrontPolygon, ProjectiveTransform, quad_bounds};
use cranpose_ui_graphics::{
    BlendMode, ColorFilter, CommandRecording, DrawPlacement, DrawPrimitive, RecordingSummary,
};

use crate::{
    raster_cache::LayerRasterCacheHashes,
    text_paint::{TextPaint, TextPaintCache},
};

mod heap_bytes;
use heap_bytes::layer_heap_bytes;

/// Reasons a layer is drawn into its own offscreen buffer instead of directly into its parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsolationReasons {
    /// The layer asked for offscreen compositing.
    pub explicit_offscreen: bool,
    /// The layer clips to a shape that is not a rectangle.
    pub shape_clip: bool,
    /// The layer applies a render effect to its content.
    pub effect: bool,
    /// The layer applies an effect to what lies behind it.
    pub backdrop: bool,
    /// The layer's alpha is below one and must apply to its content as a group.
    pub group_opacity: bool,
    /// The layer uses a blend mode other than source-over.
    pub blend_mode: bool,
}

impl IsolationReasons {
    /// Whether at least one reason requires the layer to be isolated.
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.shape_clip
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
    }
}

/// Whether a renderer may keep the rasterized pixels of a layer between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    /// The layer is drawn directly each frame.
    #[default]
    None,
    /// The layer is a candidate for a raster cache because it is isolated, scaled or rotated.
    Auto,
}

/// Pointer handling attached to a layer.
#[derive(Clone)]
pub struct HitTestNode {
    /// The node's modifier slices, shared rather than copied: they hold its
    /// pointer inputs and the pointer icon it asks for while hovered. A node
    /// that only names an icon is still a hit target, which is how a
    /// decorative panel carries a cursor without handling clicks.
    pub handlers: Rc<ModifierNodeSlices>,
}

/// One draw primitive together with its clip.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawPrimitiveNode {
    /// The shape, image or other primitive to draw.
    pub primitive: DrawPrimitive,
    /// Rectangle that cuts the primitive, or `None` when it is not clipped.
    pub clip: Option<Rect>,
}

/// A run of text to draw, with the layout facts that a renderer needs.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitiveNode {
    /// Layout node that owns the text.
    pub node_id: NodeId,
    /// Box of the laid-out text in logical pixels, after alignment and panning.
    pub rect: Rect,
    /// Shared so the renderer can hand the same allocation to every draw it
    /// emits for this node instead of deep-copying the string once per emit.
    pub text: Rc<AnnotatedString>,
    /// `text` as the renderer's draws carry it across threads, shared with
    /// the prepared layout it came from so no frame converts it again.
    pub render_text: std::sync::Arc<RenderString>,
    /// The lines and glyphs the text draws, from its top left corner at
    /// `rect`'s.
    pub layout: cranpose_text::TextLayout,
    /// Shared so every frame's draws of this text hand over the same style
    /// instead of copying it.
    pub text_style: std::sync::Arc<TextStyle>,
    /// Resolved font size in logical pixels.
    pub font_size: f32,
    /// How the text was laid out to handle text that does not fit.
    pub overflow: TextOverflow,
    /// Rectangle that cuts the glyphs, or `None` when overflow is visible and the text does not
    /// pan.
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
                (self.render_text.render_hash(), self.render_text.is_empty()),
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
        (!matches!(self.overflow, TextOverflow::Visible)
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

/// Whether a primitive draws before or after the children of its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitivePhase {
    /// Drawn under the children, like a background.
    BeforeChildren,
    /// Drawn over the children, like a foreground or overlay.
    AfterChildren,
}

/// The content of a [`PrimitiveEntry`].
#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    /// A draw primitive with its clip.
    Draw(Box<DrawPrimitiveNode>),
    /// A text run.
    Text(Box<TextPrimitiveNode>),
}

/// One primitive of a layer together with the phase that orders it against the children.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveEntry {
    /// Whether the primitive draws before or after the children of its layer.
    pub phase: PrimitivePhase,
    /// The primitive.
    pub node: PrimitiveNode,
}

/// Scene properties with implicit defaults and independently mutable clones.
#[derive(Clone, Debug, Default)]
pub struct LayerProperties(Option<Box<GraphicsLayer>>);

impl LayerProperties {
    /// The properties that were set, or `None` when they read as defaults.
    pub fn get(&self) -> Option<&GraphicsLayer> {
        self.0.as_deref()
    }

    /// Whether these properties read as `value`, `None` for the defaults:
    /// a node without a layer compares nothing.
    #[inline]
    pub(crate) fn reads_as(&self, value: Option<&GraphicsLayer>) -> bool {
        match (self.get(), value) {
            (None, None) => true,
            (stored, value) => {
                stored.unwrap_or(&GraphicsLayer::DEFAULT)
                    == value.unwrap_or(&GraphicsLayer::DEFAULT)
            }
        }
    }

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
        self.reads_as(other.get())
    }
}

/// A node of the render graph that groups content under one set of layer properties: bounds,
/// transform, clip, graphics layer and children.
#[derive(Clone)]
pub struct LayerNode {
    /// Layout node of this layer, or `None` for the root and the synthetic wrapper layers.
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
    /// Maps the layer's local space to its parent's content space, with the placement, the graphics
    /// layer transform and the parent's content offset applied.
    pub transform_to_parent: ProjectiveTransform,
    /// Offset in logical pixels of the node's content box from the node origin, for example from
    /// padding. Children are placed relative to it.
    pub content_offset: Point,
    /// Whether the layer is inside a motion context such as an animated scroll, where a renderer
    /// must not snap its content rigidly to the pixel grid.
    pub motion_context_animated: bool,
    /// Whether the layer moves its content as one translated block, as a scroll container does,
    /// so a renderer may snap that content to the pixel grid as a whole.
    pub translated_content_context: bool,
    /// Translation in logical pixels of the content when `translated_content_context` is set.
    pub translated_content_offset: Point,
    /// Where layout placed this layer within its parent's content, before the
    /// parent's content offset. Scene updates add these up from the root, with
    /// each layer's content offset and graphics-layer translation, to find the
    /// window origin of a subtree they rebuild.
    pub origin_in_parent: Point,
    /// The graphics layer properties; they read as defaults when none were set.
    pub graphics_layer: LayerProperties,
    /// Whether the layer cuts its content to `local_bounds`.
    pub clip_to_bounds: bool,
    /// Pointer handling of the node, or `None` when the node has none.
    pub hit_test: Option<HitTestNode>,
    /// Whether this layer or a layer beneath it has a hit test node. Scene building keeps it
    /// current.
    pub has_hit_targets: bool,
    /// [`Self::has_space`], worked out the first time a walk asks and
    /// forgotten when the layer or its children change.
    pub space: Cell<Option<bool>>,
    /// Whether this subtree publishes live window origins (a text field's
    /// popup anchor, a scroll container's viewport rect). Those sinks are
    /// written during a full lowering, so the scroll fast path may translate
    /// a retained subtree in place only when this is false.
    pub has_origin_sinks: bool,
    /// [`Self::draws_within_bounds`], worked out the first time a renderer
    /// asks and forgotten when the layer's children change.
    pub contained: Cell<Option<bool>>,
    /// The reasons this layer is drawn through its own offscreen buffer.
    pub isolation: IsolationReasons,
    /// Set on a layer that holds part of one draw command's draws, those a
    /// `DrawScope` transform or clip groups: the phase they paint in among
    /// the parent's own draws, which they belong to. `None` on the layer of
    /// a layout node and on the root.
    pub draw_phase: Option<PrimitivePhase>,
    /// Whether a renderer may keep this layer's pixels between frames.
    pub cache_policy: CachePolicy,
    /// The raster cache hashes, computed the first time a renderer asks and
    /// kept until the layer or a layer beneath it changes.
    pub cache_hashes: Cell<Option<LayerRasterCacheHashes>>,
    /// The scene updates that changed what the layer draws.
    pub content_changes: ContentChanges,
    /// The number the layer took when it or a layer beneath it last changed.
    pub change: LayerChange,
    /// The number the layer took when it last changed itself: its own
    /// properties, draws or list of children. A change beneath it leaves
    /// this number, so a renderer that kept the layer's own items knows they
    /// hold.
    pub own_change: LayerChange,
    /// Content in draw order: primitives, draw runs and child layers.
    pub children: Vec<RenderNode>,
}

/// A number a layer takes when it is made and whenever it or a layer beneath
/// it changes, as its raster cache hashes are forgotten: a renderer that
/// kept what it built from a subtree knows from an equal number that nothing
/// in it changed since, save the subtree's placement in its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerChange(u32);

impl LayerChange {
    /// A number no layer took in the last 2^32 changes.
    fn next() -> Self {
        static LAST: AtomicU32 = AtomicU32::new(0);
        Self(LAST.fetch_add(1, Ordering::Relaxed).wrapping_add(1))
    }
}

impl Default for LayerChange {
    fn default() -> Self {
        Self::next()
    }
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
            space: Cell::new(None),
            has_origin_sinks: false,
            contained: Cell::new(None),
            isolation: IsolationReasons::default(),
            draw_phase: None,
            cache_policy: CachePolicy::None,
            cache_hashes: Cell::new(None),
            content_changes: ContentChanges::default(),
            change: LayerChange::default(),
            own_change: LayerChange::default(),
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

    /// Whether everything this layer and its subtree draw lies within its
    /// `local_bounds`, give or take [`CONTAINED_DRAW_SLACK`], so a renderer
    /// may skip the whole subtree where those bounds are clipped away.
    /// Worked out at the first call after a change and kept until the next.
    pub fn draws_within_bounds(&self) -> bool {
        if let Some(contained) = self.contained.get() {
            return contained;
        }
        let contained = self.content_draws_within_bounds();
        self.contained.set(Some(contained));
        contained
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
        self.contained.set(None);
        self.space.set(None);
    }

    /// Whether this layer or a layer beneath it is a scene, a mesh surface
    /// or a 3D transform, so a walk for what lies in 3D skips the subtrees
    /// without one.
    pub fn has_space(&self) -> bool {
        if let Some(known) = self.space.get() {
            return known;
        }
        let found = self.graphics_layer.space.is_some()
            || self.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => child_layer.has_space(),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
            });
        self.space.set(Some(found));
        found
    }

    /// Whether this layer, drawn as a child, puts nothing outside `bounds`:
    /// it draws within its own bounds, casts no shadow, applies no effect,
    /// and its bounds placed in its parent lie inside.
    fn draws_within_parent(&self, bounds: Rect) -> bool {
        self.draws_within_bounds()
            && self.graphics_layer.shadow_elevation <= 0.0
            && self.effect().is_none()
            && self.backdrop().is_none()
            && rect_within(
                self.transform_to_parent
                    .bounds_for_rect(inflate_rect(self.local_bounds, CONTAINED_DRAW_SLACK)),
                bounds,
            )
    }

    /// The node's own rect, where its pointer handlers are.
    pub fn node_rect(&self) -> Rect {
        self.node_bounds.unwrap_or(self.local_bounds)
    }

    /// The rectangle that cuts the layer's content and pointer hits: its bounds when the layer
    /// clips to bounds or its graphics layer clips, otherwise `None`.
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
    #[inline]
    pub fn visual_clip_rect(&self) -> Option<Rect> {
        self.clip_rect().or_else(|| {
            (self.isolation.group_opacity || self.isolation.explicit_offscreen)
                .then_some(self.local_bounds)
        })
    }

    /// The render effect applied to the layer's content, if any.
    pub fn effect(&self) -> Option<&RenderEffect> {
        self.graphics_layer.render_effect.as_ref()
    }

    /// The effect applied to what lies behind the layer, if any.
    pub fn backdrop(&self) -> Option<&RenderEffect> {
        self.graphics_layer.backdrop_effect.as_ref()
    }

    /// Alpha of the layer from 0.0 (transparent) to 1.0 (opaque).
    pub fn opacity(&self) -> f32 {
        self.graphics_layer.alpha
    }

    /// How the layer composites onto what lies beneath it.
    pub fn blend_mode(&self) -> BlendMode {
        self.graphics_layer.blend_mode
    }

    /// The color filter applied to the layer's content, if any.
    pub fn color_filter(&self) -> Option<&ColorFilter> {
        self.graphics_layer.color_filter.as_ref()
    }

    /// Hash of everything the layer draws into its target: bounds, clip, alpha, children and their
    /// placement. Equal hashes mean cached pixels are still valid. Computed on first use.
    pub fn target_content_hash(&self) -> u64 {
        self.raster_cache_hashes().target_content
    }

    /// Hash of the layer's content like `target_content_hash`, except that the translated content
    /// offset is left out, so the hash stays equal while the content only moves.
    pub fn motion_source_content_hash(&self) -> u64 {
        crate::graph_hash::layer_motion_source_content_hash(self)
    }

    /// Hash of the layer's render effect, for keying cached effect output.
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
        self.forget_own_raster_cache_hashes();
        for child in &mut self.children {
            if let RenderNode::Layer(child) = child {
                child.forget_raster_cache_hashes();
            }
        }
    }

    /// Forgets this layer's own raster cache hashes and takes new change
    /// numbers: a property its hash covers changed.
    pub(crate) fn forget_own_raster_cache_hashes(&mut self) {
        self.cache_hashes.set(None);
        self.change = LayerChange::next();
        self.own_change = LayerChange::next();
    }

    /// Records that scene update `update` changed a layer beneath this one,
    /// which it holds in its place: the hashes and the subtree's change
    /// number go, the layer's own number stays.
    pub(crate) fn note_descendant_change(&mut self, update: u64) {
        self.cache_hashes.set(None);
        self.change = LayerChange::next();
        self.content_changes.note(update);
    }

    /// Records that scene update `update` changed what this layer draws, and
    /// forgets its hashes.
    pub(crate) fn note_content_change(&mut self, update: u64) {
        self.forget_own_raster_cache_hashes();
        self.content_changes.note(update);
    }

    /// Whether what this layer draws changed in scene update `update` and in
    /// the one before it. A raster cache keyed on its content would miss
    /// again on the next frame, so a renderer neither hashes nor caches it.
    pub fn content_churns(&self, update: u64) -> bool {
        self.content_changes.run >= 2 && self.content_changes.last_update == update
    }
}

/// A child of a layer in the render graph.
#[derive(Clone)]
pub enum RenderNode {
    /// A single draw or text primitive.
    Primitive(PrimitiveEntry),
    /// A whole draw command's primitives as one node. A heavy canvas records
    /// thousands of primitives per frame; wrapping each in its own
    /// [`RenderNode`] made the graph rebuild move every one of them twice and
    /// free seventeen thousand nodes per frame on a stress scene. The run
    /// keeps the recorded vector intact — semantically it is exactly that
    /// many consecutive `Primitive` draw entries with no per-primitive clip.
    DrawRun(DrawRunNode),
    /// A nested layer.
    Layer(Box<LayerNode>),
}

impl RenderNode {
    /// The phase the node paints in among its layer's own draws: a
    /// primitive's or run's, or a draw group layer's. `None` for the layer
    /// of a layout node, which is a child of the layer.
    pub fn draw_phase(&self) -> Option<PrimitivePhase> {
        match self {
            Self::Primitive(entry) => Some(entry.phase),
            Self::DrawRun(run) => Some(run.phase),
            Self::Layer(layer) => layer.draw_phase,
        }
    }

    fn draws_after_children(&self) -> bool {
        self.draw_phase() == Some(PrimitivePhase::AfterChildren)
    }
}

/// Visits `children`, a layer's child list, in the order they draw: in list
/// order, save that the draws after the children wait until every other
/// node has drawn.
pub fn for_each_in_draw_order(children: &[RenderNode], mut visit: impl FnMut(&RenderNode)) {
    let mut first_deferred = children.len();
    for (index, child) in children.iter().enumerate() {
        if child.draws_after_children() {
            first_deferred = first_deferred.min(index);
        } else {
            visit(child);
        }
    }
    children[first_deferred..]
        .iter()
        .filter(|child| child.draws_after_children())
        .for_each(visit);
}

/// Stable identity of one placement of a draw command: its layout node,
/// command index and placement pass. A `WithContent` command shares one CPU
/// recording between its two runs, while each placement retains independent
/// renderer state under this identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawCommandId {
    /// Layout node that owns the draw command.
    pub node_id: NodeId,
    /// Index of the command among the draw commands of the node.
    pub command_index: u32,
    /// Whether the command draws behind the node's content or over it.
    pub placement: DrawPlacement,
}

/// A run of recorded draw primitives from one draw command, kept together as one node.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawRunNode {
    /// Whether the run draws before or after the children of its layer.
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
    /// Creates a run for `primitives` that has no draw command provenance.
    pub fn new(phase: PrimitivePhase, primitives: Vec<DrawPrimitive>) -> Self {
        Self::for_command(phase, None, primitives)
    }

    /// Creates a run for `primitives` recorded by the draw command `command`.
    pub fn for_command(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: Vec<DrawPrimitive>,
    ) -> Self {
        let recording = CommandRecording::from_primitives(primitives);
        let segments = recording.all_segments();
        Self::for_command_shared(phase, command, Rc::new(recording), segments)
    }

    /// Creates a run over `segments` of an existing shared `recording`, without copying it.
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

    /// Runs `visit` on the run's primitives in order, as [`Self::primitives`]
    /// yields them, without the iterator's step between segments.
    pub fn for_each_primitive(&self, visit: impl FnMut(DrawPrimitive)) {
        self.recording
            .for_each_primitive(self.segments.clone(), visit);
    }

    /// The rect every entry of the run can reach.
    pub fn coverage_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.recording.coverage_rects(self.segments.clone())
    }

    /// Number of primitives in the run.
    pub fn len(&self) -> usize {
        self.recording.len_in(&self.segments)
    }

    /// Whether the run holds no primitives.
    pub fn is_empty(&self) -> bool {
        self.recording.is_empty_in(&self.segments)
    }
}

/// The scene as a tree of layers that a renderer draws.
#[derive(Clone)]
pub struct RenderGraph {
    /// The top layer; every other node of the graph is beneath it.
    pub root: LayerNode,
    /// How many scene updates the graph took: the number of the latest.
    pub update: u64,
    /// The way to the layer the last scene updates changed alone, for the
    /// next update that changes it alone.
    pub(crate) retained_path: Option<crate::scene_builder::RetainedPath>,
    // The marks of the last walk to the graph's dirty layers.
    pub(crate) dirty_ancestry: crate::scene_builder::DirtyAncestry,
}

impl RenderGraph {
    /// Creates a graph around `root` at update zero.
    pub fn new(root: LayerNode) -> Self {
        Self {
            root,
            update: 0,
            retained_path: None,
            dirty_ancestry: crate::scene_builder::DirtyAncestry::default(),
        }
    }

    /// Number of nodes in the graph. A layer counts once and each primitive of a draw run counts on
    /// its own.
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

    /// Estimated heap memory held beneath the root layer, in bytes, for memory reports.
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
