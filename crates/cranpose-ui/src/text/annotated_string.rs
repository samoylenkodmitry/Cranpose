use std::ops::Range;
use std::rc::Rc;

use crate::{ParagraphStyle, SpanStyle};

/// Mirrors Jetpack Compose's `LinkAnnotation` sealed class.
///
/// Attach to a text range via [`Builder::push_link`] or [`Builder::with_link`].
/// [`crate::widgets::LinkedText`] automatically opens URLs and invokes handlers
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
    Clickable { tag: String, handler: Rc<dyn Fn()> },
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
    pub tag: String,
    pub annotation: String,
}

/// The basic data structure of text with multiple styles.
///
/// To construct an `AnnotatedString` you can use `AnnotatedString::builder()`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotatedString {
    pub text: String,
    pub span_styles: Vec<RangeStyle<SpanStyle>>,
    pub paragraph_styles: Vec<RangeStyle<ParagraphStyle>>,
    /// Arbitrary tag+value annotations. Used for e.g. clickable link URLs.
    /// Mirrors JC `AnnotatedString.getStringAnnotations(tag, start, end)`.
    pub string_annotations: Vec<RangeStyle<StringAnnotation>>,
    /// Link annotations — URLs and clickable actions.
    /// Mirrors JC `AnnotatedString.getLinkAnnotations(start, end)`.
    pub link_annotations: Vec<RangeStyle<LinkAnnotation>>,
}

/// A style applied to a range of an `AnnotatedString`.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeStyle<T> {
    pub item: T,
    pub range: Range<usize>,
}

impl AnnotatedString {
    pub fn new(text: String) -> Self {
        Self {
            text,
            span_styles: vec![],
            paragraph_styles: vec![],
            string_annotations: vec![],
            link_annotations: vec![],
        }
    }

    pub fn builder() -> Builder {
        Builder::new()
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns a sorted list of unique byte indices where styles change.
    pub fn span_boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0, self.text.len()];
        for span in &self.span_styles {
            boundaries.push(span.range.start);
            boundaries.push(span.range.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
            .into_iter()
            .filter(|&b| b <= self.text.len() && self.text.is_char_boundary(b))
            .collect()
    }

    /// Computes a hash representing the contents of the span styles, suitable for cache invalidation.
    pub fn span_styles_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hasher.write_usize(self.span_styles.len());
        for span in &self.span_styles {
            hasher.write_usize(span.range.start);
            hasher.write_usize(span.range.end);

            // Hash measurement-affecting fields
            let dummy = crate::text::TextStyle {
                span_style: span.item.clone(),
                ..Default::default()
            };
            hasher.write_u64(dummy.measurement_hash());

            // Hash visually-affecting fields ignored by measurement
            if let Some(c) = &span.item.color {
                hasher.write_u32(c.0.to_bits());
                hasher.write_u32(c.1.to_bits());
                hasher.write_u32(c.2.to_bits());
                hasher.write_u32(c.3.to_bits());
            }
            if let Some(bg) = &span.item.background {
                hasher.write_u32(bg.0.to_bits());
                hasher.write_u32(bg.1.to_bits());
                hasher.write_u32(bg.2.to_bits());
                hasher.write_u32(bg.3.to_bits());
            }
            if let Some(d) = &span.item.text_decoration {
                d.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub fn render_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.text.hash(&mut hasher);
        self.span_styles.len().hash(&mut hasher);
        for span in &self.span_styles {
            span.range.start.hash(&mut hasher);
            span.range.end.hash(&mut hasher);
            span.item.render_hash().hash(&mut hasher);
        }
        self.paragraph_styles.len().hash(&mut hasher);
        for paragraph in &self.paragraph_styles {
            paragraph.range.start.hash(&mut hasher);
            paragraph.range.end.hash(&mut hasher);
            paragraph.item.render_hash().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Returns a new `AnnotatedString` containing a substring of the original text
    /// and any styles that overlap with the new range, with indices adjusted.
    pub fn subsequence(&self, range: std::ops::Range<usize>) -> Self {
        if range.is_empty() {
            return Self::new(String::new());
        }

        let start = range.start.min(self.text.len());
        let end = range.end.max(start).min(self.text.len());

        if start == end {
            return Self::new(String::new());
        }

        let mut new_spans = Vec::new();
        for span in &self.span_styles {
            let intersection_start = span.range.start.max(start);
            let intersection_end = span.range.end.min(end);
            if intersection_start < intersection_end {
                new_spans.push(RangeStyle {
                    item: span.item.clone(),
                    range: (intersection_start - start)..(intersection_end - start),
                });
            }
        }

        let mut new_paragraphs = Vec::new();
        for span in &self.paragraph_styles {
            let intersection_start = span.range.start.max(start);
            let intersection_end = span.range.end.min(end);
            if intersection_start < intersection_end {
                new_paragraphs.push(RangeStyle {
                    item: span.item.clone(),
                    range: (intersection_start - start)..(intersection_end - start),
                });
            }
        }

        let mut new_string_annotations = Vec::new();
        for ann in &self.string_annotations {
            let intersection_start = ann.range.start.max(start);
            let intersection_end = ann.range.end.min(end);
            if intersection_start < intersection_end {
                new_string_annotations.push(RangeStyle {
                    item: ann.item.clone(),
                    range: (intersection_start - start)..(intersection_end - start),
                });
            }
        }

        let mut new_link_annotations = Vec::new();
        for ann in &self.link_annotations {
            let intersection_start = ann.range.start.max(start);
            let intersection_end = ann.range.end.min(end);
            if intersection_start < intersection_end {
                new_link_annotations.push(RangeStyle {
                    item: ann.item.clone(),
                    range: (intersection_start - start)..(intersection_end - start),
                });
            }
        }

        Self {
            text: self.text[start..end].to_string(),
            span_styles: new_spans,
            paragraph_styles: new_paragraphs,
            string_annotations: new_string_annotations,
            link_annotations: new_link_annotations,
        }
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
        self.string_annotations
            .iter()
            .filter(|ann| ann.item.tag == tag && ann.range.start < end && ann.range.end > start)
            .collect()
    }

    /// Returns all link annotations whose range overlaps `[start, end)`.
    ///
    /// JC parity: `AnnotatedString.getLinkAnnotations(start, end)`
    pub fn get_link_annotations(
        &self,
        start: usize,
        end: usize,
    ) -> Vec<&RangeStyle<LinkAnnotation>> {
        self.link_annotations
            .iter()
            .filter(|ann| ann.range.start < end && ann.range.end > start)
            .collect()
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the given String to this Builder.
    pub fn append(mut self, text: &str) -> Self {
        self.text.push_str(text);
        self
    }

    pub fn append_annotated(self, annotated: &AnnotatedString) -> Self {
        self.append_annotated_subsequence(annotated, 0..annotated.text.len())
    }

    pub fn append_annotated_subsequence(
        mut self,
        annotated: &AnnotatedString,
        range: Range<usize>,
    ) -> Self {
        let range = clamp_subsequence_range(annotated.text.as_str(), range);
        if range.is_empty() {
            return self;
        }

        debug_assert!(annotated.text.is_char_boundary(range.start));
        debug_assert!(annotated.text.is_char_boundary(range.end));

        let target_offset = self.text.len();
        self.text.push_str(&annotated.text[range.clone()]);
        append_clipped_ranges(
            &mut self.span_styles,
            &annotated.span_styles,
            range.clone(),
            target_offset,
        );
        append_clipped_ranges(
            &mut self.paragraph_styles,
            &annotated.paragraph_styles,
            range.clone(),
            target_offset,
        );
        append_clipped_ranges(
            &mut self.string_annotations,
            &annotated.string_annotations,
            range.clone(),
            target_offset,
        );
        append_clipped_ranges(
            &mut self.link_annotations,
            &annotated.link_annotations,
            range,
            target_offset,
        );
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
    /// Call [`pop`] when done, or use [`with_link`] for the block form.
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

    /// Block form of [`push_link`] — mirrors JC's `withLink(link) { ... }` DSL.
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
        // Resolve unclosed styles
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

        AnnotatedString {
            text: self.text,
            span_styles: self
                .span_styles
                .into_iter()
                .map(|s| RangeStyle {
                    item: s.item,
                    range: s.start..s.end,
                })
                .collect(),
            paragraph_styles: self
                .paragraph_styles
                .into_iter()
                .map(|s| RangeStyle {
                    item: s.item,
                    range: s.start..s.end,
                })
                .collect(),
            string_annotations: self
                .string_annotations
                .into_iter()
                .map(|s| RangeStyle {
                    item: s.item,
                    range: s.start..s.end,
                })
                .collect(),
            link_annotations: self
                .link_annotations
                .into_iter()
                .map(|s| RangeStyle {
                    item: s.item,
                    range: s.start..s.end,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_span() {
        let span1 = SpanStyle {
            alpha: Some(0.5),
            ..Default::default()
        };

        let span2 = SpanStyle {
            alpha: Some(1.0),
            ..Default::default()
        };

        let annotated = AnnotatedString::builder()
            .append("Hello ")
            .push_style(span1.clone())
            .append("World")
            .push_style(span2.clone())
            .append("!")
            .pop()
            .pop()
            .to_annotated_string();

        assert_eq!(annotated.text, "Hello World!");
        assert_eq!(annotated.span_styles.len(), 2);
        assert_eq!(annotated.span_styles[0].range, 6..12);
        assert_eq!(annotated.span_styles[0].item, span1);
        assert_eq!(annotated.span_styles[1].range, 11..12);
        assert_eq!(annotated.span_styles[1].item, span2);
    }

    #[test]
    fn with_link_url_roundtrips() {
        let url = "https://developer.android.com";
        let annotated = AnnotatedString::builder()
            .append("Visit ")
            .with_link(LinkAnnotation::Url(url.into()), |b| {
                b.append("Android Developers")
            })
            .append(".")
            .to_annotated_string();

        assert_eq!(annotated.text, "Visit Android Developers.");
        assert_eq!(annotated.link_annotations.len(), 1);
        let ann = &annotated.link_annotations[0];
        // "Android Developers" starts at byte 6
        assert_eq!(ann.range, 6..24);
        assert_eq!(ann.item, LinkAnnotation::Url(url.into()));
    }

    #[test]
    fn with_link_clickable_calls_handler() {
        use std::cell::Cell;
        let called = Rc::new(Cell::new(false));
        let called_clone = Rc::clone(&called);

        let annotated = AnnotatedString::builder()
            .with_link(
                LinkAnnotation::Clickable {
                    tag: "action".into(),
                    handler: Rc::new(move || called_clone.set(true)),
                },
                |b| b.append("click me"),
            )
            .to_annotated_string();

        assert_eq!(annotated.link_annotations.len(), 1);
        // Invoke the handler
        let ann = &annotated.link_annotations[0];
        if let LinkAnnotation::Clickable { handler, .. } = &ann.item {
            handler();
        }
        assert!(called.get(), "Clickable handler should have been called");
    }

    #[test]
    fn with_link_subsequence_trims_range() {
        let annotated = AnnotatedString::builder()
            .append("pre ")
            .with_link(LinkAnnotation::Url("http://x.com".into()), |b| {
                b.append("link")
            })
            .append(" post")
            .to_annotated_string();

        // Take subsequence covering only the link text
        let sub = annotated.subsequence(4..8); // "link"
        assert_eq!(sub.link_annotations.len(), 1);
        assert_eq!(sub.link_annotations[0].range, 0..4);
    }

    #[test]
    fn append_annotated_preserves_ranges_with_existing_prefix() {
        let annotated = AnnotatedString::builder()
            .append("Hello ")
            .push_style(SpanStyle {
                alpha: Some(0.5),
                ..Default::default()
            })
            .append("World")
            .pop()
            .push_string_annotation("kind", "planet")
            .append("!")
            .pop()
            .to_annotated_string();

        let combined = AnnotatedString::builder()
            .append("Prefix ")
            .append_annotated(&annotated)
            .to_annotated_string();

        assert_eq!(combined.text, "Prefix Hello World!");
        assert_eq!(combined.span_styles.len(), 1);
        assert_eq!(combined.span_styles[0].range, 13..18);
        assert_eq!(combined.string_annotations.len(), 1);
        assert_eq!(combined.string_annotations[0].range, 18..19);
    }

    #[test]
    fn append_annotated_subsequence_clips_ranges_to_slice() {
        let annotated = AnnotatedString::builder()
            .append("Before ")
            .push_style(SpanStyle {
                alpha: Some(0.5),
                ..Default::default()
            })
            .append("Styled")
            .pop()
            .with_link(LinkAnnotation::Url("https://example.com".into()), |b| {
                b.append(" Link")
            })
            .to_annotated_string();

        let slice = AnnotatedString::builder()
            .append("-> ")
            .append_annotated_subsequence(&annotated, 7..18)
            .to_annotated_string();

        assert_eq!(slice.text, "-> Styled Link");
        assert_eq!(slice.span_styles.len(), 1);
        assert_eq!(slice.span_styles[0].range, 3..9);
        assert_eq!(slice.link_annotations.len(), 1);
        assert_eq!(slice.link_annotations[0].range, 9..14);
    }

    #[test]
    fn render_hash_changes_for_visual_style_ranges() {
        let plain = AnnotatedString::builder()
            .append("Hello")
            .to_annotated_string();
        let styled = AnnotatedString::builder()
            .push_style(SpanStyle {
                color: Some(crate::modifier::Color(1.0, 0.0, 0.0, 1.0)),
                ..Default::default()
            })
            .append("Hello")
            .pop()
            .to_annotated_string();

        assert_ne!(plain.render_hash(), styled.render_hash());
    }
}
