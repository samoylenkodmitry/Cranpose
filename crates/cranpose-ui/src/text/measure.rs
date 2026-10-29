use std::{
    cell::{Cell, OnceCell, RefCell},
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use cranpose_core::collections::pass_aged::PassAgedCache;
use cranpose_text::{
    CaretDirection, FontExtent, FontFamily, FontFamilyResolver, FontRequest, FontStyle, FontWeight,
    LayoutInput, ParagraphIntrinsics, TextLayout, TextLayouter, paragraph_line_box,
};
use cranpose_ui_graphics::{FxHasher, HashBits, MemoryHeadroom, TextLayoutHandle};
use smallvec::SmallVec;

use super::{
    AnnotatedString, LineBox, RenderString, SpanStyle, TextAlign, TextDrawStyle, TextUnit,
    layout_input::{DEFAULT_FONT_SIZE_SP, ParagraphSpans, TextUnits},
    layout_options::{TextLayoutOptions, TextOverflow},
    style::TextStyle,
};
use crate::{density::Density, font_scale::FontScaleCurve, text_layout_result::TextLayoutResult};

/// Prepared layouts kept across nodes. Each node keeps its own, so this
/// serves the items a list composes again, a few screens of them.
const TEXT_PREPARED_CACHE_CAPACITY: usize = 1024;
const TEXT_INTRINSICS_CACHE_CAPACITY: usize = 1024;
// Layouts kept for one text at different widths.
const WIDTHS_PER_TEXT: usize = 2;
const SCALE_DOWN_SEARCH_STEPS: usize = 14;

/// Size of a laid-out text. Distances are in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    /// The paragraph's width, within the widths it was laid out between.
    pub width: f32,
    /// Height of all lines together.
    pub height: f32,
    /// Height of the first line.
    pub line_height: f32,
    /// Number of lines, at least the minimum the options ask for.
    pub line_count: usize,
}

/// A text laid out between two widths: its glyphs and lines, which every
/// query, measurement and renderer reads, and the size it measures.
#[derive(Clone, Debug)]
pub struct PreparedTextLayout {
    /// The text that was laid out.
    pub text: Rc<AnnotatedString>,
    /// The style the text draws in, shared with every render node drawn
    /// from this layout.
    pub visual_style: Arc<TextStyle>,
    /// The lines and glyphs.
    pub layout: TextLayout,
    /// Size and line count.
    pub metrics: TextMetrics,
    /// The first and last baselines.
    pub alignment_lines: cranpose_ui_layout::AlignmentLines,
    /// True when the text did not fit: it had more lines than the line limit,
    /// or was wider than the widest it could be.
    pub did_overflow: bool,
    /// `text` as a renderer draws it, and its
    /// [`RenderString::render_hash`](crate::text::RenderString::render_hash),
    /// converted on first use: see [`PreparedTextLayout::render_text`].
    pub render_text: OnceCell<(Arc<RenderString>, u64)>,
    pub(crate) visual_style_hash: OnceCell<u64>,
    // The layout as a draw scope carries it, made on first use.
    layout_handle: OnceCell<TextLayoutHandle>,
}

impl PreparedTextLayout {
    // The prepared layout of `text` in `style` laid out as `layout`, `max_width`
    // the widest it could be.
    fn new(
        text: Rc<AnnotatedString>,
        style: Arc<TextStyle>,
        layout: TextLayout,
        (max_width, min_lines): (f32, usize),
    ) -> Self {
        let last = layout.line_count() - 1;
        let metrics = TextMetrics {
            width: layout.width().min(max_width),
            height: layout.height(),
            line_height: layout.line_bottom(0) - layout.line_top(0),
            line_count: layout.line_count().max(min_lines),
        };
        Self {
            alignment_lines: cranpose_ui_layout::AlignmentLines::new(
                Some(layout.line_baseline(0)),
                Some(layout.line_baseline(last)),
            ),
            did_overflow: layout.did_overflow_height() || layout.did_overflow_width(),
            metrics,
            text,
            visual_style: style,
            layout,
            render_text: OnceCell::new(),
            visual_style_hash: OnceCell::new(),
            layout_handle: OnceCell::new(),
        }
    }

    // The layout as a draw scope carries it to a renderer, made once for
    // every frame that draws the same text.
    pub(crate) fn layout_handle(&self) -> TextLayoutHandle {
        self.layout_handle
            .get_or_init(|| TextLayoutHandle::from(Arc::new(self.layout.clone())))
            .clone()
    }

    // This layout at the widths of `frame`, at which it breaks into the
    // same lines: itself when its width there is its own, else a copy whose
    // lines `align` places across that width. The copy keeps the text as a
    // renderer draws it and the style's hash.
    pub(crate) fn at_widths(self: &Rc<Self>, frame: TextFrame, align: TextAlign) -> Rc<Self> {
        let width = self.layout.width_between(frame.min_width, frame.max_width);
        if width.to_bits() == self.layout.width().to_bits() {
            return Rc::clone(self);
        }
        let mut layout = self.layout.clone();
        layout.align(frame.min_width, frame.max_width, align);
        let min_lines = frame.options.normalized().min_lines;
        Rc::new(Self {
            render_text: self.render_text.clone(),
            visual_style_hash: self.visual_style_hash.clone(),
            ..Self::new(
                Rc::clone(&self.text),
                Arc::clone(&self.visual_style),
                layout,
                (frame.max_width, min_lines),
            )
        })
    }

    /// The display text as a renderer draws it. Converted once for the layout
    /// and shared after, so every frame and scene rebuild drawing this layout
    /// hands over the same allocation.
    pub fn render_text(&self) -> Arc<RenderString> {
        if let Some((converted, _)) = self.render_text.get() {
            return Arc::clone(converted);
        }
        self.convert_render_text().0
    }

    /// [`RenderString::render_hash`](crate::text::RenderString::render_hash)
    /// of [`PreparedTextLayout::render_text`], kept beside it so that reading
    /// it never loads the string.
    pub fn render_text_hash(&self) -> u64 {
        if let Some((_, hash)) = self.render_text.get() {
            return *hash;
        }
        self.convert_render_text().1
    }

    fn convert_render_text(&self) -> (Arc<RenderString>, u64) {
        let converted = Arc::new(self.text.render_string());
        let hash = converted.render_hash();
        let _ = self.render_text.set((Arc::clone(&converted), hash));
        (converted, hash)
    }

    /// [`TextStyle::render_hash`] of `visual_style`, hashed once for the
    /// layout, or handed over by the node that knows its style's hash.
    pub fn visual_style_hash(&self) -> u64 {
        if let Some(hash) = self.visual_style_hash.get() {
            return *hash;
        }
        let hash = self.visual_style.render_hash();
        let _ = self.visual_style_hash.set(hash);
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
            && self.metrics == other.metrics
            && self.alignment_lines == other.alignment_lines
            && self.did_overflow == other.did_overflow
    }
}

// What a layout is cached by: the text and style hashes, the options and the
// units its sizes became pixels in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextKey {
    text_hash: u64,
    style_hash: u64,
    options: TextLayoutOptions,
    // The density's bits, the font scale curve's fingerprint and the size
    // factor's bits.
    units: (u32, u32, u32),
}

impl TextKey {
    // The key of `text` in `style` by `style_hash`; `None` for a text with
    // annotations, which a layout of another text with the same look must
    // not stand in for.
    fn of(
        text: &AnnotatedString,
        style_hash: u64,
        options: TextLayoutOptions,
        units: TextUnits,
    ) -> Option<Self> {
        (text.string_annotations.is_empty() && text.link_annotations.is_empty()).then(|| Self {
            text_hash: text.render_hash(),
            style_hash,
            options: options.normalized(),
            units: (
                units.density.density().to_bits(),
                units.density.font_scale_curve().fingerprint(),
                units.factor.to_bits(),
            ),
        })
    }
}

// What the text service's shared cache holds of a text for a frame.
enum Cached {
    // A layout that breaks into the same lines, aligned to the frame.
    Held(Rc<PreparedTextLayout>),
    // Layouts at other widths, which a new one joins.
    OtherWidths,
    Missing,
}

// When the text service's shared cache keeps a layout it missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAdmission {
    // At once: a node's first text, which a list composes again.
    First,
    // Once asked for again since the doorkeeper last forgot: text changed
    // in place or drawn by a draw scope, whose strings come back only
    // sometimes, as a stat's do, or never, as a ticker's.
    Second,
}

/// The text and style a layout is prepared from: borrowed, which the layout
/// copies, or shared with the node that holds them.
#[derive(Clone, Copy)]
enum LayoutSource<'a> {
    Borrowed {
        text: &'a AnnotatedString,
        style: &'a TextStyle,
    },
    Shared {
        text: &'a SharedText<'a>,
        // A layout of the text in the style at other widths, whose glyphs a
        // new layout takes in place of shaping the text again.
        shaped: Option<&'a TextLayout>,
    },
}

// A text and style a node holds, and the style's render hash.
pub(crate) struct SharedText<'a> {
    pub(crate) text: &'a Rc<AnnotatedString>,
    pub(crate) style: &'a Arc<TextStyle>,
    pub(crate) style_hash: u64,
}

impl LayoutSource<'_> {
    fn text(&self) -> &AnnotatedString {
        match self {
            Self::Borrowed { text, .. } => text,
            Self::Shared { text, .. } => text.text,
        }
    }

    fn style(&self) -> &TextStyle {
        match self {
            Self::Borrowed { style, .. } => style,
            Self::Shared { text, .. } => text.style,
        }
    }

    fn shared(&self) -> (Rc<AnnotatedString>, Arc<TextStyle>) {
        match self {
            Self::Borrowed { text, style } => {
                (Rc::new((*text).clone()), Arc::new((*style).clone()))
            }
            Self::Shared { text, .. } => (Rc::clone(text.text), Arc::clone(text.style)),
        }
    }

    fn style_hash(&self) -> u64 {
        match self {
            Self::Borrowed { style, .. } => style.render_hash(),
            Self::Shared { text, .. } => text.style_hash,
        }
    }

    fn shaped(&self) -> Option<&TextLayout> {
        match self {
            Self::Borrowed { .. } => None,
            Self::Shared { shaped, .. } => *shaped,
        }
    }
}

/// The widths a text lays out between, and its options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextFrame {
    pub(crate) min_width: f32,
    pub(crate) max_width: f32,
    pub(crate) options: TextLayoutOptions,
}

impl TextFrame {
    // No width limit, lines ending only at line breaks.
    fn unbounded(options: TextLayoutOptions) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            options,
        }
    }

    // From zero up to `max_width`; `None`, and a width that is not positive
    // and finite, is no limit.
    pub(crate) fn up_to(max_width: Option<f32>, options: TextLayoutOptions) -> Self {
        Self {
            max_width: max_width
                .filter(|width| width.is_finite() && *width > 0.0)
                .unwrap_or(f32::INFINITY),
            ..Self::unbounded(options)
        }
    }
}

/// The app's fonts and the text engine its text lays out with, and the
/// layouts and intrinsic widths recent passes used.
pub(crate) struct TextService {
    generation: Cell<u64>,
    /// Whether a frame started since the last pass began: the first use of
    /// the service outside a layout pass then begins one.
    pass_owed: Cell<bool>,
    fonts: RefCell<FontFamilyResolver>,
    layouter: RefCell<TextLayouter>,
    prepared: RefCell<PassAgedCache<TextKey, SmallVec<[Rc<PreparedTextLayout>; WIDTHS_PER_TEXT]>>>,
    intrinsics: RefCell<PassAgedCache<TextKey, ParagraphIntrinsics>>,
    // The keys of layouts missed once, by which a text missed again goes
    // into the cache: a string drawn every frame keeps its second layout,
    // and one drawn once keeps nothing.
    seen: RefCell<HashBits>,
    // The host's memory headroom when the pass began. A tight heap keeps
    // no layout of a text that changes in place.
    headroom: Cell<MemoryHeadroom>,
}

impl TextService {
    pub(crate) fn new() -> Self {
        Self {
            generation: Cell::new(1),
            pass_owed: Cell::new(false),
            fonts: RefCell::new(default_fonts()),
            layouter: RefCell::new(TextLayouter::default()),
            prepared: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_PREPARED_CACHE_CAPACITY,
            )),
            intrinsics: RefCell::new(PassAgedCache::with_capacity_at_least_one(
                TEXT_INTRINSICS_CACHE_CAPACITY,
            )),
            seen: RefCell::default(),
            headroom: Cell::default(),
        }
    }

    /// Lays text out with `fonts` from now on.
    pub(crate) fn set_fonts(&self, fonts: FontFamilyResolver) {
        *self.fonts.borrow_mut() = fonts;
        self.generation
            .set(self.generation.get().wrapping_add(1).max(1));
        self.prepared.borrow_mut().clear();
        *self.seen.borrow_mut() = HashBits::default();
        self.intrinsics.borrow_mut().clear();
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    // The layout of `source` in `frame` with sizes in `units`: a cached one
    // that breaks into the same lines, aligned to the frame's widths, or a
    // new one.
    fn prepare(
        &self,
        source: LayoutSource<'_>,
        frame: TextFrame,
        (units, admission): (TextUnits, CacheAdmission),
    ) -> Rc<PreparedTextLayout> {
        let (text, style) = (source.text(), source.style());
        let key = TextKey::of(text, source.style_hash(), frame.options, units);
        let cached = key.map_or(Cached::Missing, |key| self.cached(key, frame, style));
        let other_widths = match cached {
            Cached::Held(prepared) => return prepared,
            Cached::OtherWidths => true,
            Cached::Missing => false,
        };
        let (text, style) = source.shared();
        let prepared = Rc::new(self.lay_out(text, style, frame, (units, source.shaped())));
        if let Some(key) = key {
            if other_widths {
                if let Some(widths) = self.prepared.borrow_mut().get_mut(&key) {
                    remember(widths, &prepared);
                }
            } else {
                let admitted = match admission {
                    CacheAdmission::First => true,
                    CacheAdmission::Second if self.headroom.get() == MemoryHeadroom::Ample => {
                        let mut hasher = FxHasher::default();
                        key.hash(&mut hasher);
                        let hash = hasher.finish();
                        let mut seen = self.seen.borrow_mut();
                        seen.contains(hash) || {
                            seen.insert(hash);
                            false
                        }
                    }
                    CacheAdmission::Second => false,
                };
                if admitted {
                    let widths = SmallVec::from_elem(Rc::clone(&prepared), 1);
                    self.prepared.borrow_mut().push(key, widths);
                }
            }
        }
        prepared
    }

    // What the shared cache holds of `key` for `frame`: a layout that
    // breaks into the same lines, aligned to its widths, or layouts at
    // other widths only, or nothing.
    fn cached(&self, key: TextKey, frame: TextFrame, style: &TextStyle) -> Cached {
        let mut cache = self.prepared.borrow_mut();
        let Some(widths) = cache.get_mut(&key) else {
            return Cached::Missing;
        };
        let Some(held) = widths
            .iter()
            .find(|prepared| prepared.layout.holds(frame.min_width, frame.max_width))
            .map(Rc::clone)
        else {
            return Cached::OtherWidths;
        };
        let aligned = held.at_widths(frame, style.paragraph_style.text_align);
        if !Rc::ptr_eq(&aligned, &held) {
            remember(widths, &aligned);
        }
        Cached::Held(aligned)
    }

    // Lays `text` out in `style`, auto-sizing it when its options ask, with
    // the glyphs of `shaped` when given.
    fn lay_out(
        &self,
        text: Rc<AnnotatedString>,
        style: Arc<TextStyle>,
        frame: TextFrame,
        (units, shaped): (TextUnits, Option<&TextLayout>),
    ) -> PreparedTextLayout {
        let options = frame.options.normalized();
        let min_lines = options.min_lines;
        let Some(min_font_size_sp) = options.overflow.scale_down_min_font_size_sp() else {
            let layout = self.layout(&text, &style, frame, (units, shaped));
            return PreparedTextLayout::new(text, style, layout, (frame.max_width, min_lines));
        };
        let frame = TextFrame {
            options: TextLayoutOptions {
                overflow: TextOverflow::Clip,
                ..options
            },
            ..frame
        };
        let fits =
            |layout: &TextLayout| !layout.did_overflow_height() && !layout.did_overflow_width();
        let full = self.layout(&text, &style, frame, (units, shaped));
        let base = style.resolve_font_size(DEFAULT_FONT_SIZE_SP);
        let min_factor = (min_font_size_sp.min(base) / base).clamp(0.0, 1.0);
        if fits(&full) || !frame.max_width.is_finite() || min_factor >= 1.0 {
            return PreparedTextLayout::new(text, style, full, (frame.max_width, min_lines));
        }
        // Each size takes the glyphs of the first: shaping is the same at
        // every size.
        let at = |factor: f32| {
            let units = TextUnits {
                factor: units.factor * factor,
                ..units
            };
            self.layout(&text, &style, frame, (units, Some(&full)))
        };
        let (mut best, mut factor) = (at(min_factor), min_factor);
        if fits(&best) {
            let (mut low, mut high) = (min_factor, 1.0);
            for _ in 0..SCALE_DOWN_SEARCH_STEPS {
                let mid = (low + high) * 0.5;
                let candidate = at(mid);
                if fits(&candidate) {
                    low = mid;
                    (best, factor) = (candidate, mid);
                } else {
                    high = mid;
                }
            }
        }
        let (text, style) = scaled_drawing(&text, &style, factor);
        PreparedTextLayout::new(text, style, best, (frame.max_width, min_lines))
    }

    // One layout of `text` in `style` in `frame`, with sizes in `units`, with
    // the glyphs of `shaped` when given.
    fn layout(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        frame: TextFrame,
        (units, shaped): (TextUnits, Option<&TextLayout>),
    ) -> TextLayout {
        let paragraph = ParagraphSpans::new(text, style, units);
        let fonts = self.fonts.borrow();
        let spans = paragraph.spans(&fonts);
        let input = LayoutInput {
            text: &text.text,
            spans: &spans,
            paragraph: &style.paragraph_style,
            lengths: paragraph.lengths,
            min_width: frame.min_width,
            max_width: frame.max_width,
            options: frame.options,
        };
        let mut layouter = self.layouter.borrow_mut();
        match shaped {
            Some(shaped) => layouter.relayout(&input, &fonts, shaped),
            None => layouter.layout(&input, &fonts),
        }
    }

    // The intrinsic widths of `text` in `style`, with sizes in `units`.
    fn intrinsics(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        units: TextUnits,
    ) -> ParagraphIntrinsics {
        let key = TextKey::of(
            text,
            style.measurement_hash(),
            TextLayoutOptions::default(),
            units,
        );
        if let Some(key) = key
            && let Some(found) = self.intrinsics.borrow_mut().get(&key).copied()
        {
            return found;
        }
        let paragraph = ParagraphSpans::new(text, style, units);
        let fonts = self.fonts.borrow();
        let spans = paragraph.spans(&fonts);
        let input = LayoutInput {
            text: &text.text,
            spans: &spans,
            paragraph: &style.paragraph_style,
            lengths: paragraph.lengths,
            min_width: 0.0,
            max_width: f32::INFINITY,
            options: TextLayoutOptions::default(),
        };
        let found = self.layouter.borrow_mut().intrinsics(&input, &fonts);
        if let Some(key) = key {
            self.intrinsics.borrow_mut().push(key, found);
        }
        found
    }

    // The extent of `style`'s own font with sizes in `units`: its ascent,
    // descent and line gap in pixels. `None` with no font.
    fn extent(&self, style: &TextStyle, units: TextUnits) -> Option<FontExtent> {
        let span = &style.span_style;
        let size = units
            .density
            .font_scale_curve()
            .sp_to_dp(style.resolve_font_size(DEFAULT_FONT_SIZE_SP))
            * units.factor;
        let fonts = self.fonts.borrow();
        let typeface = fonts.resolve(FontRequest {
            family: span.font_family.as_ref().unwrap_or(&FontFamily::Default),
            weight: span.font_weight.unwrap_or(FontWeight::NORMAL),
            style: span.font_style.unwrap_or(FontStyle::Normal),
        })?;
        let scale = size / f32::from(typeface.units_per_em());
        let metrics = typeface.line_metrics();
        Some(FontExtent::new(
            metrics.ascent * scale,
            -metrics.descent * scale,
            metrics.line_gap * scale,
        ))
    }

    // The box of a line of `style`'s own font with sizes in `units`, before
    // any text: its advance, baseline and edges. `None` with no font.
    fn line_box(&self, style: &TextStyle, units: TextUnits) -> Option<LineBox> {
        let extent = self.extent(style, units)?;
        let empty = AnnotatedString::from("");
        let paragraph = ParagraphSpans::new(&empty, style, units);
        let asked = paragraph.lengths.line_height.unwrap_or(f32::NAN);
        Some(paragraph_line_box(
            &style.paragraph_style,
            extent,
            asked,
            units.density.density(),
        ))
    }

    /// Starts a layout pass: the caches drop what recent passes did not use.
    pub(crate) fn begin_layout_pass(&self) {
        self.pass_owed.set(false);
        self.prepared.borrow_mut().begin_pass(drop);
        self.intrinsics.borrow_mut().begin_pass(drop);
        let headroom = crate::current_memory_headroom();
        self.headroom.set(headroom);
        self.layouter.borrow_mut().set_memory_headroom(headroom);
    }

    /// A frame starts: the first use of the service in it outside a layout
    /// pass begins a pass.
    pub(crate) fn owe_pass(&self) {
        self.pass_owed.set(true);
    }

    /// Begins the pass a frame owes, if no layout pass began one: what a
    /// frame that only draws measures then ages out as a layout pass's does.
    pub(crate) fn pay_owed_pass(&self) {
        if self.pass_owed.get() {
            self.begin_layout_pass();
        }
    }
}

// The fonts text lays out with before a renderer hands the app its own:
// none, and Cranpose Test Mono in tests.
fn default_fonts() -> FontFamilyResolver {
    #[cfg(any(test, feature = "test-helpers"))]
    {
        super::test_fonts()
    }
    #[cfg(not(any(test, feature = "test-helpers")))]
    {
        FontFamilyResolver::new([])
    }
}

// `text` and `style` as a text scaled down by `factor` draws them: sizes,
// shadows and strokes `factor` of what they say, as its glyphs are. A text
// whose spans scale nothing keeps its own string.
fn scaled_drawing(
    text: &Rc<AnnotatedString>,
    style: &TextStyle,
    factor: f32,
) -> (Rc<AnnotatedString>, Arc<TextStyle>) {
    let mut scaled_style = style.clone();
    scaled_style.span_style.font_size =
        TextUnit::Sp(style.resolve_font_size(DEFAULT_FONT_SIZE_SP) * factor);
    scale_span_drawing(&mut scaled_style.span_style, factor);
    let text = if text.span_styles.is_empty() {
        Rc::clone(text)
    } else {
        let mut scaled = AnnotatedString::clone(text);
        for span in &mut scaled.span_styles {
            if let TextUnit::Sp(size) = span.item.font_size {
                span.item.font_size = TextUnit::Sp(size * factor);
            }
            scale_span_drawing(&mut span.item, factor);
        }
        Rc::new(scaled)
    };
    (text, Arc::new(scaled_style))
}

// Scales the letter spacing, shadow and stroke of `span` by `factor`.
fn scale_span_drawing(span: &mut SpanStyle, factor: f32) {
    if let TextUnit::Sp(spacing) = span.letter_spacing {
        span.letter_spacing = TextUnit::Sp(spacing * factor);
    }
    if let Some(shadow) = &mut span.shadow {
        shadow.offset.x *= factor;
        shadow.offset.y *= factor;
        shadow.blur_radius *= factor;
    }
    if let Some(TextDrawStyle::Stroke { width }) = &mut span.draw_style {
        *width *= factor;
    }
}

// Puts `prepared` first among the layouts of one text, keeping the newest.
fn remember(
    widths: &mut SmallVec<[Rc<PreparedTextLayout>; WIDTHS_PER_TEXT]>,
    prepared: &Rc<PreparedTextLayout>,
) {
    widths.insert(0, Rc::clone(prepared));
    widths.truncate(WIDTHS_PER_TEXT);
}

// Text sizes at the host's density and font scale.
fn host_units() -> TextUnits {
    TextUnits {
        density: Density::from_host(),
        factor: 1.0,
    }
}

// Text sizes taken as stated, on the host's pixel grid: a draw scope's text
// style has its sizes resolved already.
fn resolved_units() -> TextUnits {
    TextUnits {
        density: Density::with_curve(Density::from_host().density(), FontScaleCurve::linear(1.0)),
        factor: 1.0,
    }
}

fn prepare_borrowed(
    text: &AnnotatedString,
    style: &TextStyle,
    frame: TextFrame,
    units: TextUnits,
) -> Rc<PreparedTextLayout> {
    crate::render_state::with_text_service(|service| {
        service.prepare(
            LayoutSource::Borrowed { text, style },
            frame,
            (units, CacheAdmission::First),
        )
    })
}

/// Lays text out with `fonts` in the active app context from now on. Panics
/// when no app context is active.
pub fn set_text_fonts(fonts: FontFamilyResolver) {
    let Some(context) = crate::render_state::current_app_context() else {
        panic!("set_text_fonts requires an active AppContext");
    };
    context.set_text_fonts(fonts);
}

/// Measures `text` in `style` with line breaks only at its line breaks, at
/// the system font scale.
pub fn measure_text(text: &AnnotatedString, style: &TextStyle) -> TextMetrics {
    prepare_borrowed(
        text,
        style,
        TextFrame::unbounded(TextLayoutOptions::default()),
        host_units(),
    )
    .metrics
}

// `text` laid out in `style` with lines ending only at its line breaks,
// `style`'s sizes taken as stated, not scaled by the system font scale, as
// a draw scope draws it: the cache keeps it once it comes back, so a
// number drawn anew each frame takes no room in it.
pub(crate) fn prepare_resolved(
    text: &AnnotatedString,
    style: &TextStyle,
) -> Rc<PreparedTextLayout> {
    crate::render_state::with_text_service(|service| {
        service.prepare(
            LayoutSource::Borrowed { text, style },
            TextFrame::unbounded(TextLayoutOptions::default()),
            (resolved_units(), CacheAdmission::Second),
        )
    })
}

/// Measures `text` wrapped to `max_width` under `options`. `None` and widths
/// that are not positive and finite mean no width limit.
pub fn measure_text_with_options(
    text: &AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    prepare_borrowed(
        text,
        style,
        TextFrame::up_to(max_width, options),
        host_units(),
    )
    .metrics
}

/// Lays `text` out in `style` wrapped to `max_width` under `options`.
pub fn prepare_text_layout(
    text: &AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Rc<PreparedTextLayout> {
    prepare_borrowed(
        text,
        style,
        TextFrame::up_to(max_width, options),
        host_units(),
    )
}

// Lays out `text` in `style` between the widths of `frame`, sharing them,
// kept by the shared cache as `admission` says.
pub(crate) fn prepare_shared(
    text: &SharedText<'_>,
    frame: TextFrame,
    admission: CacheAdmission,
    shaped: Option<&TextLayout>,
) -> Rc<PreparedTextLayout> {
    crate::render_state::with_text_service(|service| {
        service.prepare(
            LayoutSource::Shared { text, shaped },
            frame,
            (host_units(), admission),
        )
    })
}

/// The byte offset of the caret nearest to the point (`x`, `y`) of `text`
/// laid out in `style` with lines ending only at its line breaks, in logical
/// pixels from its top left corner.
pub fn get_offset_for_position(text: &AnnotatedString, style: &TextStyle, x: f32, y: f32) -> usize {
    prepare_borrowed(
        text,
        style,
        TextFrame::unbounded(TextLayoutOptions::default()),
        host_units(),
    )
    .layout
    .offset_for_position(x, y)
}

/// The byte offset of the caret nearest to the point (`x`, `y`) of `text`
/// laid out in `style` and wrapped to `wrap_width`: the hit test of every
/// editable text's pointer path. `x` and `y` are in text space, padding and
/// scrolling taken off.
pub fn offset_for_position_wrapped(
    text: &str,
    style: &TextStyle,
    wrap_width: Option<f32>,
    x: f32,
    y: f32,
) -> usize {
    prepare_borrowed(
        &AnnotatedString::from(text),
        style,
        TextFrame::up_to(wrap_width, TextLayoutOptions::default()),
        host_units(),
    )
    .layout
    .offset_for_position(x, y)
}

/// The x of the caret at byte `offset` of `text` laid out in `style` with
/// lines ending only at its line breaks.
pub fn get_cursor_x_for_offset(text: &AnnotatedString, style: &TextStyle, offset: usize) -> f32 {
    prepare_borrowed(
        text,
        style,
        TextFrame::unbounded(TextLayoutOptions::default()),
        host_units(),
    )
    .layout
    .horizontal_position(offset, CaretDirection::Primary)
}

/// Lays `text` out in `style` with lines ending only at its line breaks.
pub fn layout_text(text: &AnnotatedString, style: &TextStyle) -> TextLayoutResult {
    prepare_borrowed(
        text,
        style,
        TextFrame::unbounded(TextLayoutOptions::default()),
        host_units(),
    )
    .result()
}

/// The byte range of each line of `text` laid out at `max_width` with
/// `options`, without its line break.
pub fn wrapped_line_ranges(
    text: &AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> Vec<Range<usize>> {
    let prepared = prepare_borrowed(
        text,
        style,
        TextFrame::up_to(max_width, options),
        host_units(),
    );
    let layout = &prepared.layout;
    (0..layout.line_count())
        .map(|line| {
            let (start, end) = (layout.line_start(line), layout.line_end(line));
            let line_text = text.text.get(start..end).unwrap_or_default();
            let without_break = line_text.trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}']);
            start..start + without_break.len()
        })
        .collect()
}

/// The widest word of `text` in `style`: Compose's
/// `ParagraphIntrinsics.minIntrinsicWidth`.
pub(crate) fn min_intrinsic_text_width(text: &AnnotatedString, style: &TextStyle) -> f32 {
    crate::render_state::with_text_service(|service| service.intrinsics(text, style, host_units()))
        .min_intrinsic_width
}

/// The line box of `style`'s own font: its advance, its baseline and what a
/// paragraph gives back at its edges. `None` with no font.
pub fn text_line_box(style: &TextStyle) -> Option<LineBox> {
    crate::render_state::with_text_service(|service| service.line_box(style, host_units()))
}

/// Like [`text_line_box`] with `style`'s sizes taken as stated.
pub(crate) fn resolved_line_box(style: &TextStyle) -> Option<LineBox> {
    crate::render_state::current_app_context()?;
    crate::render_state::with_text_service(|service| service.line_box(style, resolved_units()))
}

/// Distance from the top of a `style` paragraph down to its first baseline.
/// `None` with no font.
pub fn first_baseline(style: &TextStyle) -> Option<f32> {
    text_line_box(style).map(LineBox::first_baseline)
}

/// The glyph box `(top_offset, height)` of a `style` line inside a line
/// `line_height` tall: the font's ascent and descent under the line's
/// baseline. The whole line with no font.
pub fn glyph_line_box(style: &TextStyle, line_height: f32) -> (f32, f32) {
    let units = host_units();
    crate::render_state::with_text_service(|service| {
        let extent = service.extent(style, units)?;
        let line_box = service.line_box(style, units)?;
        Some((
            line_box.baseline - extent.ascent,
            extent.ascent + extent.descent,
        ))
    })
    .map_or((0.0, line_height), |(top, height)| {
        (top.clamp(0.0, line_height), height.min(line_height))
    })
}

#[cfg(test)]
#[path = "tests/measure_tests.rs"]
mod tests;
