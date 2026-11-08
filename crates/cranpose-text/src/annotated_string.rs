use std::{ops::Range, rc::Rc, sync::Arc};

use crate::{ParagraphStyle, SpanStyle};

/// Mirrors Jetpack Compose's `LinkAnnotation` sealed class.
///
/// Attach to a text range via [`Builder::push_link`] or [`Builder::with_link`].
/// `cranpose_ui::widgets::LinkedText` automatically opens URLs and invokes handlers
/// when the user taps the annotated text.
///
/// # JC ref
/// `androidx.compose.foundation.text.input.internal.selection.LinkAnnotation`
///
/// # Example
///
/// ```rust,ignore
/// let text = AnnotatedString::builder()
///     .append("Visit ")
///     .with_link(
///         LinkAnnotation::Url("https://developer.android.com".into()),
///         |b| b.append("Android Developers"),
///     )
///     .to_annotated_string();
/// ```
#[derive(Clone)]
pub enum LinkAnnotation {
    /// Opens the given URL via the platform URI handler when clicked.
    ///
    /// JC parity: `LinkAnnotation.Url(url)`
    Url(String),

    /// Calls an arbitrary handler when clicked.
    ///
    /// JC parity: `LinkAnnotation.Clickable(tag, linkInteractionListener)`
    Clickable {
        /// Name that identifies the link.
        tag: String,
        /// Called when the user clicks the link.
        handler: Rc<dyn Fn()>,
    },
}

impl std::fmt::Debug for LinkAnnotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(url) => f.debug_tuple("Url").field(url).finish(),
            Self::Clickable { tag, .. } => f.debug_struct("Clickable").field("tag", tag).finish(),
        }
    }
}

impl PartialEq for LinkAnnotation {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Url(a), Self::Url(b)) => a == b,
            (
                Self::Clickable {
                    tag: ta,
                    handler: ha,
                },
                Self::Clickable {
                    tag: tb,
                    handler: hb,
                },
            ) => ta == tb && Rc::ptr_eq(ha, hb),
            _ => false,
        }
    }
}

/// Mirrors Jetpack Compose's `AnnotatedString.Range<String>` — a tag+value
/// annotation covering a byte range.
///
/// JC ref: `androidx.compose.ui.text.AnnotatedString.Range`
#[derive(Debug, Clone, PartialEq)]
pub struct StringAnnotation {
    /// Name that groups annotations. Look annotations up by it with
    /// `AnnotatedString::get_string_annotations`.
    pub tag: String,
    /// The value stored for the tag.
    pub annotation: String,
}

/// Link identity without the link behavior: what rendering may know about a
/// [`LinkAnnotation`]. URL links keep their URL, clickable links keep their
/// tag — the handler stays UI-side.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkKey {
    /// Identity of a [`LinkAnnotation::Url`].
    Url(String),
    /// Identity of a [`LinkAnnotation::Clickable`] — its tag.
    Clickable(String),
}

/// What rendering reads from an [`AnnotatedString`]: content, styles, and
/// link identity — never the link handlers, which live UI-side only.
///
/// Unlike `AnnotatedString` this is plain owned data (`Send + Sync`), so a
/// lowered scene that carries it can cross threads. An `AnnotatedString`
/// keeps its content in one of these, which the renderer shares.
///
/// Built once and never changed, it hashes itself when built: renderers key
/// caches by [`RenderString::render_hash`] every frame.
#[derive(Debug, Clone)]
pub struct RenderString {
    text: String,
    span_styles: Box<[RangeStyle<SpanStyle>]>,
    // Paragraph styles, string annotations and links, which most text lacks.
    extras: Option<Box<RenderExtras>>,
    hash: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RenderExtras {
    paragraph_styles: Box<[RangeStyle<ParagraphStyle>]>,
    string_annotations: Box<[RangeStyle<StringAnnotation>]>,
    links: Box<[RangeStyle<LinkKey>]>,
}

impl Default for RenderString {
    fn default() -> Self {
        Self::plain(String::new())
    }
}

impl PartialEq for RenderString {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.text == other.text
            && self.span_styles == other.span_styles
            && self.extras == other.extras
    }
}

const _: () = {
    fn assert_send<T: Send + Sync>() {}
    #[expect(dead_code, reason = "a compile-time check that is never called")]
    fn assert_render_string_is_send_sync() {
        assert_send::<RenderString>();
    }
};

impl RenderString {
    fn from_parts(
        text: String,
        span_styles: Vec<RangeStyle<SpanStyle>>,
        paragraph_styles: Vec<RangeStyle<ParagraphStyle>>,
        string_annotations: Vec<RangeStyle<StringAnnotation>>,
        links: Vec<RangeStyle<LinkKey>>,
    ) -> Self {
        let hash = render_hash_impl(&text, &span_styles, &paragraph_styles);
        let extras =
            (!paragraph_styles.is_empty() || !string_annotations.is_empty() || !links.is_empty())
                .then(|| {
                    Box::new(RenderExtras {
                        paragraph_styles: paragraph_styles.into_boxed_slice(),
                        string_annotations: string_annotations.into_boxed_slice(),
                        links: links.into_boxed_slice(),
                    })
                });
        Self {
            text,
            span_styles: span_styles.into_boxed_slice(),
            extras,
            hash,
        }
    }

    // `text` with no styles, annotations or links.
    fn plain(text: String) -> Self {
        let hash = render_hash_impl(&text, &[], &[]);
        Self {
            text,
            span_styles: Box::default(),
            extras: None,
            hash,
        }
    }

    /// The characters.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Character styles with the byte ranges they cover.
    pub fn span_styles(&self) -> &[RangeStyle<SpanStyle>] {
        &self.span_styles
    }

    /// Paragraph styles with the byte ranges they cover.
    pub fn paragraph_styles(&self) -> &[RangeStyle<ParagraphStyle>] {
        self.extras
            .as_deref()
            .map_or(&[], |extras| &extras.paragraph_styles)
    }

    /// Tag and value annotations with the byte ranges they cover.
    pub fn string_annotations(&self) -> &[RangeStyle<StringAnnotation>] {
        self.extras
            .as_deref()
            .map_or(&[], |extras| &extras.string_annotations)
    }

    /// Link ranges by identity (tag/url): enough to hash and to key caches,
    /// never enough to invoke a link.
    pub fn links(&self) -> &[RangeStyle<LinkKey>] {
        self.extras.as_deref().map_or(&[], |extras| &extras.links)
    }

    /// Length of the text in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Returns true when the text is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns a sorted list of unique byte indices where styles change.
    pub fn span_boundaries(&self) -> SpanBoundaries {
        span_boundaries_impl(&self.text, &self.span_styles)
    }

    /// Hash of the text and of the span and paragraph styles, as renderers
    /// use it to key caches.
    pub fn render_hash(&self) -> u64 {
        self.hash
    }

    /// Returns a new `RenderString` containing a substring of the original
    /// text and any styles that overlap with the new range, with indices
    /// adjusted.
    pub fn subsequence(&self, range: Range<usize>) -> Self {
        let start = range.start.min(self.text.len());
        let end = range.end.max(start).min(self.text.len());
        if start == end {
            return Self::default();
        }

        Self::from_parts(
            self.text[start..end].to_string(),
            clip_range_styles(&self.span_styles, start, end),
            clip_range_styles(self.paragraph_styles(), start, end),
            clip_range_styles(self.string_annotations(), start, end),
            clip_range_styles(self.links(), start, end),
        )
    }
}

// Compose's `intersect`: whether two ranges share a character, or one is
// empty and the other contains it.
fn intersect(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start.max(b.start) < a.end.min(b.end) || contains(a, b) || contains(b, a)
}

// Compose's `contains`: whether `base` holds `target`; equal ends count only
// when both ranges are empty or both are not.
fn contains(base: &Range<usize>, target: &Range<usize>) -> bool {
    base.start <= target.start
        && target.end <= base.end
        && (base.end != target.end || (target.start == target.end) == (base.start == base.end))
}

fn clip_range_styles<T: Clone>(
    styles: &[RangeStyle<T>],
    start: usize,
    end: usize,
) -> Vec<RangeStyle<T>> {
    let mut clipped = Vec::new();
    for style in styles {
        let intersection_start = style.range.start.max(start);
        let intersection_end = style.range.end.min(end);
        if intersection_start < intersection_end {
            clipped.push(RangeStyle {
                item: style.item.clone(),
                range: (intersection_start - start)..(intersection_end - start),
            });
        }
    }
    clipped
}

/// Byte offsets where a string's styles change, held inline for the few
/// spans most text carries.
pub type SpanBoundaries = smallvec::SmallVec<[usize; 8]>;

fn span_boundaries_impl(text: &str, span_styles: &[RangeStyle<SpanStyle>]) -> SpanBoundaries {
    let mut boundaries: SpanBoundaries = smallvec::smallvec![0, text.len()];
    for span in span_styles {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.retain(|boundary| *boundary <= text.len() && text.is_char_boundary(*boundary));
    boundaries
}

fn render_hash_impl(
    text: &str,
    span_styles: &[RangeStyle<SpanStyle>],
    paragraph_styles: &[RangeStyle<ParagraphStyle>],
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = cranpose_ui_graphics::FxHasher::default();
    text.hash(&mut hasher);
    span_styles.len().hash(&mut hasher);
    for span in span_styles {
        span.range.start.hash(&mut hasher);
        span.range.end.hash(&mut hasher);
        span.item.render_hash().hash(&mut hasher);
    }
    paragraph_styles.len().hash(&mut hasher);
    for paragraph in paragraph_styles {
        paragraph.range.start.hash(&mut hasher);
        paragraph.range.end.hash(&mut hasher);
        paragraph.item.render_hash().hash(&mut hasher);
    }
    hasher.finish()
}

// What a clickable link calls when it is clicked.
type LinkHandler = Rc<dyn Fn()>;

// The content every empty string shares, so an empty string allocates
// nothing.
static EMPTY: std::sync::LazyLock<Arc<RenderString>> =
    std::sync::LazyLock::new(|| Arc::new(RenderString::plain(String::new())));

/// The basic data structure of text with multiple styles: Compose's
/// `AnnotatedString`. Its content, the text, styles, annotations and link
/// identities, is shared with every layout and draw made from it, so a clone
/// copies no text. The handlers of its clickable links stay with it.
///
/// To construct an `AnnotatedString` with styles you can use
/// `AnnotatedString::builder()`.
#[derive(Clone)]
pub struct AnnotatedString {
    content: Arc<RenderString>,
    // The handler of each clickable link of `content`, in the links' order;
    // `None` when it has none.
    handlers: Option<Rc<[LinkHandler]>>,
}

impl Default for AnnotatedString {
    fn default() -> Self {
        Self {
            content: Arc::clone(&EMPTY),
            handlers: None,
        }
    }
}

impl std::fmt::Debug for AnnotatedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnnotatedString")
            .field("content", &self.content)
            .field(
                "handlers",
                &self.handlers.as_ref().map_or(0, |handlers| handlers.len()),
            )
            .finish()
    }
}

impl PartialEq for AnnotatedString {
    fn eq(&self, other: &Self) -> bool {
        let same_handlers = match (&self.handlers, &other.handlers) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| Rc::ptr_eq(a, b))
            }
            _ => false,
        };
        (Arc::ptr_eq(&self.content, &other.content) || *self.content == *other.content)
            && same_handlers
    }
}

/// A style applied to a range of an `AnnotatedString`.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeStyle<T> {
    /// The style or annotation that applies to the range.
    pub item: T,
    /// Byte range of the text the item covers. Both ends lie on character boundaries.
    pub range: Range<usize>,
}

impl AnnotatedString {
    /// Creates a string holding `text` with no styles and no annotations.
    pub fn new(text: String) -> Self {
        if text.is_empty() {
            return Self::default();
        }
        Self {
            content: Arc::new(RenderString::plain(text)),
            handlers: None,
        }
    }

    // The string of `content`'s links, with `handlers` for its clickable
    // ones in their order.
    fn of_parts(content: RenderString, handlers: Vec<LinkHandler>) -> Self {
        Self {
            content: Arc::new(content),
            handlers: (!handlers.is_empty()).then(|| Rc::from(handlers)),
        }
    }

    /// Starts a [`Builder`] for a string with styles and annotations.
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// The characters.
    pub fn text(&self) -> &str {
        &self.content.text
    }

    /// Character styles with the byte ranges they cover.
    pub fn span_styles(&self) -> &[RangeStyle<SpanStyle>] {
        &self.content.span_styles
    }

    /// Paragraph styles with the byte ranges they cover.
    pub fn paragraph_styles(&self) -> &[RangeStyle<ParagraphStyle>] {
        self.content.paragraph_styles()
    }

    /// Tag and value annotations with the byte ranges they cover.
    pub fn string_annotations(&self) -> &[RangeStyle<StringAnnotation>] {
        self.content.string_annotations()
    }

    /// Whether a link annotation covers part of `[start, end)`.
    ///
    /// JC parity: `AnnotatedString.hasLinkAnnotations(start, end)`
    pub fn has_link_annotations(&self, start: usize, end: usize) -> bool {
        self.content
            .links()
            .iter()
            .any(|link| intersect(&link.range, &(start..end)))
    }

    /// Length of the text in bytes.
    pub fn len(&self) -> usize {
        self.content.text.len()
    }

    /// Returns true when the text is empty.
    pub fn is_empty(&self) -> bool {
        self.content.text.is_empty()
    }

    /// Returns a sorted list of unique byte indices where styles change.
    pub fn span_boundaries(&self) -> SpanBoundaries {
        self.content.span_boundaries()
    }

    /// The content as rendering reads it (text, styles, link identity), never
    /// the link handlers. Shared: every clone of this string holds the same.
    pub fn render_string(&self) -> &Arc<RenderString> {
        &self.content
    }

    /// Hash of the span styles' ranges and the attributes that change how
    /// the text measures: two strings with the same text and hash measure
    /// alike, whatever their spans' colors, backgrounds or decorations.
    pub fn span_measurement_hash(&self) -> u64 {
        use std::hash::Hasher;
        let spans = self.span_styles();
        let mut hasher = cranpose_ui_graphics::FxHasher::default();
        hasher.write_usize(spans.len());
        for span in spans {
            hasher.write_usize(span.range.start);
            hasher.write_usize(span.range.end);
            crate::style::hash_span_measurement(&span.item, &mut hasher);
        }
        hasher.finish()
    }

    /// Hash of the text and of the span and paragraph styles, as renderers use it to key caches.
    pub fn render_hash(&self) -> u64 {
        self.content.hash
    }

    /// This string with `map` applied to each of its span styles: the same
    /// text, ranges, annotations and links.
    pub fn map_span_styles(&self, mut map: impl FnMut(&mut SpanStyle)) -> Self {
        let mut content = RenderString::clone(&self.content);
        for span in &mut content.span_styles {
            map(&mut span.item);
        }
        content.hash = render_hash_impl(
            &content.text,
            &content.span_styles,
            content.paragraph_styles(),
        );
        Self {
            content: Arc::new(content),
            handlers: self.handlers.clone(),
        }
    }

    /// Returns a new `AnnotatedString` containing a substring of the original text
    /// and any styles that overlap with the new range, with indices adjusted.
    pub fn subsequence(&self, range: Range<usize>) -> Self {
        let range = clamp_subsequence_range(self.text(), range);
        if range.is_empty() {
            return Self::default();
        }
        Builder::new()
            .append_annotated_subsequence(self, range)
            .to_annotated_string()
    }

    /// Returns all string annotations with the given `tag` whose range overlaps `[start, end)`.
    ///
    /// JC parity: `AnnotatedString.getStringAnnotations(tag, start, end) -> List<Range<String>>`
    pub fn get_string_annotations(
        &self,
        tag: &str,
        start: usize,
        end: usize,
    ) -> Vec<&RangeStyle<StringAnnotation>> {
        self.string_annotations()
            .iter()
            .filter(|ann| ann.item.tag == tag && intersect(&ann.range, &(start..end)))
            .collect()
    }

    /// Returns all link annotations whose range overlaps `[start, end)`.
    ///
    /// JC parity: `AnnotatedString.getLinkAnnotations(start, end)`
    pub fn get_link_annotations(
        &self,
        start: usize,
        end: usize,
    ) -> Vec<RangeStyle<LinkAnnotation>> {
        self.link_annotations()
            .filter(|ann| intersect(&ann.range, &(start..end)))
            .collect()
    }

    // Every link annotation in order, each clickable one with its handler.
    fn link_annotations(&self) -> impl Iterator<Item = RangeStyle<LinkAnnotation>> + '_ {
        let mut handlers = self.handlers.iter().flat_map(|handlers| handlers.iter());
        self.content.links().iter().map(move |link| RangeStyle {
            item: match &link.item {
                LinkKey::Url(url) => LinkAnnotation::Url(url.clone()),
                LinkKey::Clickable(tag) => LinkAnnotation::Clickable {
                    tag: tag.clone(),
                    handler: Rc::clone(
                        handlers.next().expect("a clickable link holds its handler"),
                    ),
                },
            },
            range: link.range.clone(),
        })
    }
}

impl From<String> for AnnotatedString {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for AnnotatedString {
    fn from(text: &str) -> Self {
        Self::new(text.to_owned())
    }
}

impl From<&String> for AnnotatedString {
    fn from(text: &String) -> Self {
        Self::new(text.clone())
    }
}

impl From<&mut String> for AnnotatedString {
    fn from(text: &mut String) -> Self {
        Self::new(text.clone())
    }
}

/// A builder to construct `AnnotatedString`.
#[derive(Debug, Default, Clone)]
pub struct Builder {
    text: String,
    span_styles: Vec<MutableRange<SpanStyle>>,
    paragraph_styles: Vec<MutableRange<ParagraphStyle>>,
    string_annotations: Vec<MutableRange<StringAnnotation>>,
    link_annotations: Vec<MutableRange<LinkAnnotation>>,
    style_stack: Vec<StyleStackRecord>,
}

#[derive(Debug, Clone)]
struct MutableRange<T> {
    item: T,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct StyleStackRecord {
    style_type: StyleType,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleType {
    Span,
    Paragraph,
    StringAnnotation,
    LinkAnnotation,
}

fn clamp_subsequence_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = range.start.min(text.len());
    let end = range.end.max(start).min(text.len());
    start..end
}

// The ranges a builder collected, each from its start to its end.
fn finished_ranges<T>(ranges: Vec<MutableRange<T>>) -> Vec<RangeStyle<T>> {
    ranges
        .into_iter()
        .map(|range| RangeStyle {
            item: range.item,
            range: range.start..range.end,
        })
        .collect()
}

fn append_clipped_ranges<T: Clone>(
    target: &mut Vec<MutableRange<T>>,
    source: &[RangeStyle<T>],
    source_range: Range<usize>,
    target_offset: usize,
) {
    for style in source {
        let intersection_start = style.range.start.max(source_range.start);
        let intersection_end = style.range.end.min(source_range.end);
        if intersection_start < intersection_end {
            target.push(MutableRange {
                item: style.item.clone(),
                start: (intersection_start - source_range.start) + target_offset,
                end: (intersection_end - source_range.start) + target_offset,
            });
        }
    }
}

impl Builder {
    /// Creates a builder with no text.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the given String to this Builder.
    pub fn append(mut self, text: &str) -> Self {
        self.text.push_str(text);
        self
    }

    /// Appends the text of `annotated` together with its styles and annotations.
    pub fn append_annotated(self, annotated: &AnnotatedString) -> Self {
        self.append_annotated_subsequence(annotated, 0..annotated.len())
    }

    /// Appends the bytes of `annotated` in `range` with the styles and annotations that overlap
    /// them. The range is clamped to the text and both ends must lie on character boundaries.
    pub fn append_annotated_subsequence(
        mut self,
        annotated: &AnnotatedString,
        range: Range<usize>,
    ) -> Self {
        let range = clamp_subsequence_range(annotated.text(), range);
        if range.is_empty() {
            return self;
        }

        debug_assert!(annotated.text().is_char_boundary(range.start));
        debug_assert!(annotated.text().is_char_boundary(range.end));

        let target_offset = self.text.len();
        self.text.push_str(&annotated.text()[range.clone()]);
        append_clipped_ranges(
            &mut self.span_styles,
            annotated.span_styles(),
            range.clone(),
            target_offset,
        );
        append_clipped_ranges(
            &mut self.paragraph_styles,
            annotated.paragraph_styles(),
            range.clone(),
            target_offset,
        );
        append_clipped_ranges(
            &mut self.string_annotations,
            annotated.string_annotations(),
            range.clone(),
            target_offset,
        );
        for link in annotated.link_annotations() {
            let start = link.range.start.max(range.start);
            let end = link.range.end.min(range.end);
            if start < end {
                self.link_annotations.push(MutableRange {
                    item: link.item,
                    start: start - range.start + target_offset,
                    end: end - range.start + target_offset,
                });
            }
        }
        self
    }

    /// Applies the given `SpanStyle` to any appended text until a corresponding `pop` is called.
    ///
    /// Returns the index of the pushed style, which can be passed to `pop_to` or used as an ID.
    pub fn push_style(mut self, style: SpanStyle) -> Self {
        let index = self.span_styles.len();
        self.span_styles.push(MutableRange {
            item: style,
            start: self.text.len(),
            end: usize::MAX,
        });
        self.style_stack.push(StyleStackRecord {
            style_type: StyleType::Span,
            index,
        });
        self
    }

    /// Applies the given `ParagraphStyle` to any appended text until a corresponding `pop` is called.
    pub fn push_paragraph_style(mut self, style: ParagraphStyle) -> Self {
        let index = self.paragraph_styles.len();
        self.paragraph_styles.push(MutableRange {
            item: style,
            start: self.text.len(),
            end: usize::MAX,
        });
        self.style_stack.push(StyleStackRecord {
            style_type: StyleType::Paragraph,
            index,
        });
        self
    }

    /// Pushes a string annotation covering subsequent appended text until the matching `pop`.
    ///
    /// JC parity: `Builder.pushStringAnnotation(tag, annotation)`
    pub fn push_string_annotation(mut self, tag: &str, annotation: &str) -> Self {
        let index = self.string_annotations.len();
        self.string_annotations.push(MutableRange {
            item: StringAnnotation {
                tag: tag.to_string(),
                annotation: annotation.to_string(),
            },
            start: self.text.len(),
            end: usize::MAX,
        });
        self.style_stack.push(StyleStackRecord {
            style_type: StyleType::StringAnnotation,
            index,
        });
        self
    }

    /// Pushes a [`LinkAnnotation`] covering subsequent appended text.
    /// Call `pop` when done, or use `with_link` for the block form.
    ///
    /// JC parity: `Builder.pushLink(link)`
    pub fn push_link(mut self, link: LinkAnnotation) -> Self {
        let index = self.link_annotations.len();
        self.link_annotations.push(MutableRange {
            item: link,
            start: self.text.len(),
            end: usize::MAX,
        });
        self.style_stack.push(StyleStackRecord {
            style_type: StyleType::LinkAnnotation,
            index,
        });
        self
    }

    /// Block form of `push_link` — mirrors JC's `withLink(link) { ... }` DSL.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder
    ///     .append("Visit ")
    ///     .with_link(
    ///         LinkAnnotation::Url("https://developer.android.com".into()),
    ///         |b| b.append("Android Developers"),
    ///     )
    ///     .append(".")
    ///     .to_annotated_string()
    /// ```
    pub fn with_link(self, link: LinkAnnotation, block: impl FnOnce(Self) -> Self) -> Self {
        let b = self.push_link(link);
        let b = block(b);
        b.pop()
    }

    /// Ends the style that was most recently pushed.
    pub fn pop(mut self) -> Self {
        if let Some(record) = self.style_stack.pop() {
            match record.style_type {
                StyleType::Span => {
                    self.span_styles[record.index].end = self.text.len();
                }
                StyleType::Paragraph => {
                    self.paragraph_styles[record.index].end = self.text.len();
                }
                StyleType::StringAnnotation => {
                    self.string_annotations[record.index].end = self.text.len();
                }
                StyleType::LinkAnnotation => {
                    self.link_annotations[record.index].end = self.text.len();
                }
            }
        }
        self
    }

    /// Completes the builder, resolving open styles to the end of the text.
    pub fn to_annotated_string(mut self) -> AnnotatedString {
        while let Some(record) = self.style_stack.pop() {
            match record.style_type {
                StyleType::Span => {
                    self.span_styles[record.index].end = self.text.len();
                }
                StyleType::Paragraph => {
                    self.paragraph_styles[record.index].end = self.text.len();
                }
                StyleType::StringAnnotation => {
                    self.string_annotations[record.index].end = self.text.len();
                }
                StyleType::LinkAnnotation => {
                    self.link_annotations[record.index].end = self.text.len();
                }
            }
        }

        let mut handlers = Vec::new();
        let links = self
            .link_annotations
            .into_iter()
            .map(|link| RangeStyle {
                item: match link.item {
                    LinkAnnotation::Url(url) => LinkKey::Url(url),
                    LinkAnnotation::Clickable { tag, handler } => {
                        handlers.push(handler);
                        LinkKey::Clickable(tag)
                    }
                },
                range: link.start..link.end,
            })
            .collect();
        AnnotatedString::of_parts(
            RenderString::from_parts(
                self.text,
                finished_ranges(self.span_styles),
                finished_ranges(self.paragraph_styles),
                finished_ranges(self.string_annotations),
                links,
            ),
            handlers,
        )
    }

    /// Applies `style` to the bytes of the text in `range`, as Compose's
    /// `addStyle(style, start, end)`.
    pub fn add_style(mut self, style: SpanStyle, range: Range<usize>) -> Self {
        self.span_styles.push(MutableRange {
            item: style,
            start: range.start,
            end: range.end,
        });
        self
    }
}

#[cfg(test)]
#[path = "tests/annotated_string_tests.rs"]
mod tests;
