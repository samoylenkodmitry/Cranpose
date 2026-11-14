use std::{ops::Range, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui::{
    GraphicsLayer, ModifierNodeSlices, Rect, TextOverflow, TextStyle, text::RenderString,
};
pub use cranpose_ui_graphics::transform::{FrontPolygon, ProjectiveTransform, quad_bounds};
use cranpose_ui_graphics::{CommandRecording, DrawPlacement, DrawPrimitive, RecordingSummary};

use crate::{
    glyph_reach::glyph_bounds,
    text_paint::{TextPaint, TextPaintCache},
};

#[cfg(feature = "layer-tree")]
mod inline_node;
#[cfg(feature = "layer-tree")]
mod layer_node;
#[cfg(feature = "layer-tree")]
mod layer_properties;
#[cfg(feature = "layer-tree")]
pub use inline_node::InlineNode;
#[cfg(feature = "layer-tree")]
pub use layer_node::{LayerNode, PrimitiveEntry, RenderNode, for_each_in_draw_order};
#[cfg(feature = "layer-tree")]
pub use layer_properties::LayerProperties;

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

// Whether graphics layers `a` and `b` read alike, `None` reading as the
// default.
#[inline]
pub(crate) fn graphics_layers_read_alike(
    a: Option<&GraphicsLayer>,
    b: Option<&GraphicsLayer>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (a, b) => a.unwrap_or(&GraphicsLayer::DEFAULT) == b.unwrap_or(&GraphicsLayer::DEFAULT),
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
    /// The string and its spans, as the renderer's draws carry them across
    /// threads, shared with the prepared layout it came from so no frame
    /// converts or copies it.
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
    /// Rectangle that cuts the glyphs: the node's box when the layout overflows it and overflow
    /// is not visible, or when the text pans; `None` otherwise, as Compose cuts a text.
    pub clip: Option<Rect>,
    /// What a renderer reads of `text_style` and `render_text`'s spans to draw the
    /// glyphs, so that drawing a plain text does not load the style.
    pub paint: TextPaintCache,
}

impl TextPrimitiveNode {
    /// The paint of the node's glyphs.
    pub fn paint(&self) -> &TextPaint {
        self.paint.get_or(|| {
            TextPaint::of(
                &self.text_style,
                self.text_style.render_hash(),
                self.render_text.span_styles(),
                (self.font_size, &self.layout),
            )
        })
    }

    /// Conservative bounds for the emitted glyphs and paint: the clip, or
    /// the layout rectangle and wherever the ink of a glyph, a stroke's
    /// included, reaches past it. Returns `None` when an unclipped layout or
    /// style can draw beyond both: a visible overflow, a shadow of the style
    /// or a span, or a baseline shift.
    pub fn draw_bounds(&self) -> Option<Rect> {
        if let Some(clip) = self.clip {
            return Some(clip);
        }
        if matches!(self.overflow, TextOverflow::Visible) {
            return None;
        }
        // The paint, kept in the node, tells this without loading the style.
        let paint = self.paint();
        (!paint.casts_shadow && paint.baseline_shift == 0.0)
            .then(|| glyph_bounds(self.rect, &self.layout, paint.spread))
    }

    #[cfg(feature = "window-damage")]
    pub(crate) fn draws_within(&self, bounds: Rect) -> bool {
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

/// A draw or a text that a layer draws.
#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    /// A draw primitive with its clip.
    Draw(Box<DrawPrimitiveNode>),
    /// A text run.
    Text(Box<TextPrimitiveNode>),
}

/// How far past its bounds a layer that draws within them may still put
/// pixels: glyph and edge antialiasing.
pub const CONTAINED_DRAW_SLACK: f32 = 1.0;

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
    /// The store keeps the recording when it lets go of the run, so the
    /// command can record into its buffers on a later update. A redraw
    /// records into it again only while the run is its one holder, which
    /// is what makes sharing sound.
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

    // Whether the run puts nothing outside `bounds` of its layer.
    #[cfg(feature = "window-damage")]
    pub(crate) fn draws_within(&self, bounds: Rect) -> bool {
        !self.summary.has_shadow
            && self
                .recording
                .bounds()
                .is_none_or(|drawn| rect_within(drawn, bounds))
    }
}

#[cfg(feature = "window-damage")]
pub(crate) fn inflate_rect(rect: Rect, by: f32) -> Rect {
    Rect {
        x: rect.x - by,
        y: rect.y - by,
        width: rect.width + by * 2.0,
        height: rect.height + by * 2.0,
    }
}

#[cfg(feature = "window-damage")]
pub(crate) fn rect_within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[cfg(test)]
#[path = "tests/graph_tests.rs"]
mod tests;
