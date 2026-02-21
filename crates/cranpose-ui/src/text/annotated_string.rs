use std::ops::Range;

use crate::{ParagraphStyle, SpanStyle};

/// The basic data structure of text with multiple styles.
///
/// To construct an `AnnotatedString` you can use `AnnotatedString::builder()`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotatedString {
    pub text: String,
    pub span_styles: Vec<RangeStyle<SpanStyle>>,
    pub paragraph_styles: Vec<RangeStyle<ParagraphStyle>>,
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
            let mut dummy = crate::text::TextStyle::default();
            dummy.span_style = span.item.clone();
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

        Self {
            text: self.text[start..end].to_string(),
            span_styles: new_spans,
            paragraph_styles: new_paragraphs,
        }
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
}
