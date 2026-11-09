use std::{cell::OnceCell, rc::Rc, sync::Arc};

use cranpose_core::PackedU64;
use cranpose_text::TextLayout;
use cranpose_ui_graphics::{Size, TextLayoutHandle};

use super::{
    AnnotatedString, RenderString, TextAlign,
    measure::{TextFrame, TextMetrics},
    style::TextStyle,
};
use crate::text_layout_result::TextLayoutResult;

/// A text laid out between two widths: its glyphs and lines, which every
/// query, measurement and renderer reads, and the size it measures.
#[derive(Clone, Debug)]
pub struct PreparedTextLayout {
    /// The text that was laid out, with its spans, shared with the text it
    /// came from.
    pub text: Arc<RenderString>,
    /// The style the text draws in, shared with every render node drawn
    /// from this layout.
    pub visual_style: Arc<TextStyle>,
    /// The lines and glyphs.
    pub layout: TextLayout,
    // The widest the text could be, and the fewest lines its height covers,
    // which its metrics follow from with its layout.
    max_width: f32,
    min_lines: u32,
    pub(crate) visual_style_hash: OnceCell<PackedU64>,
    // The layout as a draw scope carries it, made on first use.
    layout_handle: OnceCell<TextLayoutHandle>,
}

impl PreparedTextLayout {
    // The prepared layout of `text` in `style` laid out as `layout`, `max_width`
    // the widest it could be.
    pub(super) fn new(
        text: &AnnotatedString,
        style: Arc<TextStyle>,
        layout: TextLayout,
        (max_width, min_lines): (f32, usize),
    ) -> Self {
        Self {
            max_width,
            min_lines: u32::try_from(min_lines).unwrap_or(u32::MAX),
            text: Arc::clone(text.render_string()),
            visual_style: style,
            layout,
            visual_style_hash: OnceCell::new(),
            layout_handle: OnceCell::new(),
        }
    }

    // Takes `text` in `style`, which its layout holds laid out already, in
    // place of what it held.
    pub(super) fn retext(
        &mut self,
        text: &AnnotatedString,
        style: Arc<TextStyle>,
        frame: TextFrame,
    ) {
        let min_lines = frame.options.normalized().min_lines;
        self.max_width = frame.max_width;
        self.min_lines = u32::try_from(min_lines).unwrap_or(u32::MAX);
        self.text = Arc::clone(text.render_string());
        self.visual_style = style;
        self.visual_style_hash = OnceCell::new();
        self.layout_handle = OnceCell::new();
    }

    /// The paragraph's width, within the widths it was laid out between, and
    /// the height of all its lines together.
    pub fn size(&self) -> Size {
        Size::new(
            self.layout.width().min(self.max_width),
            self.layout.height(),
        )
    }

    // The max widths at which the text laid out from `min_width` gives this
    // layout again: its lines, its width and its size the same.
    pub(crate) fn max_widths_kept(&self, min_width: f32) -> cranpose_ui_layout::BoundRange {
        let width = self.layout.width();
        // A paragraph wider than its max measures as the max, which moves
        // with it.
        if self.max_width < width {
            return cranpose_ui_layout::BoundRange::exactly(self.max_width);
        }
        let (low, high) = self.layout.max_widths_kept(min_width);
        cranpose_ui_layout::BoundRange {
            low: low.max(width),
            high,
        }
    }

    /// Size and line count.
    pub fn metrics(&self) -> TextMetrics {
        let layout = &self.layout;
        let size = self.size();
        TextMetrics {
            width: size.width,
            height: size.height,
            line_height: layout.line_advance(),
            line_count: layout
                .line_count()
                .max(usize::try_from(self.min_lines).unwrap_or(usize::MAX)),
        }
    }

    /// The first and last baselines.
    pub fn alignment_lines(&self) -> cranpose_ui_layout::AlignmentLines {
        let layout = &self.layout;
        cranpose_ui_layout::AlignmentLines::new(
            Some(layout.line_baseline(0)),
            Some(layout.line_baseline(layout.line_count() - 1)),
        )
    }

    /// True when the text did not fit: it had more lines than the line limit,
    /// or was wider than the widest it could be.
    pub fn did_overflow(&self) -> bool {
        self.layout.did_overflow_height() || self.layout.did_overflow_width()
    }

    // The layout as a draw scope carries it to a renderer, made once for
    // every frame that draws the same text.
    pub(crate) fn layout_handle(&self) -> TextLayoutHandle {
        self.layout_handle
            .get_or_init(|| TextLayoutHandle::from(Arc::new(self.layout.clone())))
            .clone()
    }

    // This layout at `widths`, at which it breaks into the same lines: itself when its width there is its own, else a copy whose
    // lines `align` places across that width. The copy keeps the text as a
    // renderer draws it and the style's hash, and takes the allocation of
    // `spare` when nothing else holds it.
    pub(crate) fn at_widths(
        self: &Rc<Self>,
        widths: (f32, f32),
        align: TextAlign,
        spare: Option<Rc<Self>>,
    ) -> Rc<Self> {
        let width = self.layout.width_between(widths.0, widths.1);
        if width.to_bits() == self.layout.width().to_bits() {
            return Rc::clone(self);
        }
        self.aligned_at(widths, align, spare)
    }

    // The copy `at_widths` makes. Out of line: the copy costs more than the
    // call, and a layout held at its own width takes none.
    #[inline(never)]
    fn aligned_at(
        self: &Rc<Self>,
        widths: (f32, f32),
        align: TextAlign,
        spare: Option<Rc<Self>>,
    ) -> Rc<Self> {
        let mut aligned = Self {
            text: Arc::clone(&self.text),
            visual_style: Arc::clone(&self.visual_style),
            layout: self.layout.clone(),
            max_width: self.max_width,
            min_lines: self.min_lines,
            visual_style_hash: self.visual_style_hash.clone(),
            layout_handle: OnceCell::new(),
        };
        aligned.realign(widths, align);
        if let Some(mut spare) = spare
            && let Some(slot) = Rc::get_mut(&mut spare)
        {
            *slot = aligned;
            return spare;
        }
        Rc::new(aligned)
    }

    // Places the lines across `(min_width, max_width)`, at which the layout
    // breaks into the same lines, as `align` says.
    pub(crate) fn realign(&mut self, (min_width, max_width): (f32, f32), align: TextAlign) {
        self.layout.align(min_width, max_width, align);
        self.max_width = max_width;
        self.layout_handle = OnceCell::new();
    }

    // `realign` when the layout breaks into the same lines between
    // `(min_width, max_width)`; `false`, leaving it as it was, when not.
    pub(crate) fn try_realign(
        &mut self,
        (min_width, max_width): (f32, f32),
        align: TextAlign,
    ) -> bool {
        let held = self.layout.try_align(min_width, max_width, align);
        if held {
            self.max_width = max_width;
            self.layout_handle = OnceCell::new();
        }
        held
    }

    /// The display text as a renderer draws it: the text's own content, so
    /// every frame and scene rebuild drawing this layout hands over the same
    /// allocation.
    pub fn render_text(&self) -> Arc<RenderString> {
        Arc::clone(&self.text)
    }

    /// [`RenderString::render_hash`](crate::text::RenderString::render_hash)
    /// of [`PreparedTextLayout::render_text`].
    pub fn render_text_hash(&self) -> u64 {
        self.text.render_hash()
    }

    /// [`TextStyle::render_hash`] of `visual_style`, hashed once for the
    /// layout, or handed over by the node that knows its style's hash.
    pub fn visual_style_hash(&self) -> u64 {
        if let Some(hash) = self.visual_style_hash.get() {
            return hash.get();
        }
        let hash = self.visual_style.render_hash();
        let _ = self.visual_style_hash.set(PackedU64::new(hash));
        hash
    }

    /// The layout's queries, as Compose's `TextLayoutResult` gives them.
    pub fn result(&self) -> TextLayoutResult {
        TextLayoutResult::new(self.layout.clone())
    }
}

impl PartialEq for PreparedTextLayout {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.visual_style == other.visual_style
            && self.metrics() == other.metrics()
            && self.alignment_lines() == other.alignment_lines()
            && self.did_overflow() == other.did_overflow()
    }
}
