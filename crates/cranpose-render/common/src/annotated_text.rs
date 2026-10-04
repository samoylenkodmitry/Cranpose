use std::ops::Range;

use cranpose_ui::text::{FontExtent, LineBox, TextMetrics, TextStyle, line_box};
use cranpose_ui_graphics::{Brush, Point, Rect};
use smallvec::SmallVec;

use crate::software_text_raster::{
    SoftwareTextFont, SoftwareTextFontSet, StyledTextRef, asked_line_height,
    effective_style_for_range, font_extent, measure_text_with_font,
};

#[derive(Clone, Copy)]
pub(crate) struct AnnotatedBrushExtent {
    pub style_index: usize,
    pub line_index: usize,
    pub rect: Rect,
}

struct ResolvedSpan<'a> {
    range: Range<usize>,
    style_index: usize,
    font: &'a SoftwareTextFont,
    font_size: f32,
    extent: FontExtent,
    line_box: LineBox,
}

pub(crate) struct ResolvedLine {
    pub range: Range<usize>,
    spans: Range<usize>,
    pub line_box: LineBox,
    pub top: f32,
}

pub(crate) struct AnnotatedTextSegment<'a> {
    pub text: &'a str,
    pub range: Range<usize>,
    pub style_index: usize,
    pub style: &'a TextStyle,
    pub font: &'a SoftwareTextFont,
    pub font_size: f32,
    pub origin: Point,
    pub line_index: usize,
    pub line_top: f32,
    pub line_height: f32,
}

pub(crate) struct AnnotatedTextLayout<'a> {
    text: &'a str,
    base_style: &'a TextStyle,
    styles: SmallVec<[Option<TextStyle>; 2]>,
    spans: SmallVec<[ResolvedSpan<'a>; 2]>,
    pub lines: SmallVec<[ResolvedLine; 4]>,
}

impl<'a> AnnotatedTextLayout<'a> {
    pub fn new(
        text: StyledTextRef<'a>,
        style: &'a TextStyle,
        font_size: f32,
        scale: f32,
        grid: f32,
        fonts: &'a SoftwareTextFontSet,
    ) -> Option<Self> {
        let base_font = fonts.resolve(style)?;
        let base_extent = font_extent(base_font, font_size * scale);
        let mut styles: SmallVec<[Option<TextStyle>; 2]> = SmallVec::new();
        let mut spans = SmallVec::new();
        for range in text.span_boundaries().windows(2) {
            let (start, end) = (range[0], range[1]);
            if start == end {
                continue;
            }
            let resolved_style = if text.span_styles.is_empty() {
                None
            } else {
                Some(effective_style_for_range(
                    text.span_styles,
                    style,
                    start,
                    end,
                ))
            };
            let style_index = styles.len();
            styles.push(resolved_style);
            let span_style = styles[style_index]
                .as_ref()
                .map_or(style, |resolved| resolved);
            let font_size = span_style.resolve_font_size(font_size);
            let span_text = &text.text[start..end];
            fonts.visit_font_runs(span_text, span_style, |run, font| {
                let extent = font_extent(font, font_size * scale);
                let line_box = line_box(
                    span_style,
                    extent,
                    asked_line_height(span_style, scale),
                    grid,
                );
                spans.push(ResolvedSpan {
                    range: start + run.start..start + run.end,
                    style_index,
                    font,
                    font_size,
                    extent,
                    line_box,
                });
            })?;
        }
        let mut layout = Self {
            text: text.text,
            base_style: style,
            styles,
            spans,
            lines: SmallVec::new(),
        };
        layout.resolve_lines(style, scale, grid, base_extent);
        Some(layout)
    }

    fn resolve_lines(&mut self, style: &TextStyle, scale: f32, grid: f32, base: FontExtent) {
        let mut start = 0;
        let mut first_span = 0;
        let mut top = 0.0;
        let mut fixed_line_box = None;
        for content in self.text.split('\n') {
            let end = start + content.len();
            let extent_end = (end + 1).min(self.text.len());
            while self
                .spans
                .get(first_span)
                .is_some_and(|span| span.range.end <= start)
            {
                first_span += 1;
            }
            let mut last_span = first_span;
            let mut extent = None;
            while let Some(span) = self
                .spans
                .get(last_span)
                .filter(|span| span.range.start < extent_end)
            {
                extent = Some(match extent {
                    Some(previous) => merge_extents(previous, span.extent),
                    None => span.extent,
                });
                last_span += 1;
            }
            let resolved = fixed_line_box.unwrap_or_else(|| {
                line_box(
                    style,
                    extent.unwrap_or(base),
                    asked_line_height(style, scale),
                    grid,
                )
            });
            if !style.paragraph_style.line_height.is_unspecified() {
                fixed_line_box = Some(resolved);
            }
            if self.lines.is_empty() {
                top = -resolved.trim_top;
            }
            self.lines.push(ResolvedLine {
                range: start..end,
                spans: first_span..last_span,
                line_box: resolved,
                top,
            });
            top += resolved.height;
            start = end + 1;
        }
    }

    pub fn walk(
        &self,
        line_offsets: Option<&[f32]>,
        mut visit: impl FnMut(AnnotatedTextSegment<'_>) -> Option<f32>,
    ) -> Option<TextMetrics> {
        let mut width = 0.0_f32;
        for line_index in 0..self.lines.len() {
            let offset = line_offsets
                .and_then(|offsets| offsets.get(line_index))
                .copied()
                .unwrap_or(0.0);
            width = width.max(self.walk_line(line_index, offset, &mut visit)?);
        }
        Some(self.metrics(width))
    }

    pub fn walk_line(
        &self,
        line_index: usize,
        offset: f32,
        mut visit: impl FnMut(AnnotatedTextSegment<'_>) -> Option<f32>,
    ) -> Option<f32> {
        let line = self.lines.get(line_index)?;
        let mut x = offset;
        for span in &self.spans[line.spans.clone()] {
            let start = span.range.start.max(line.range.start);
            let end = span.range.end.min(line.range.end);
            if start >= end {
                continue;
            }
            x += visit(AnnotatedTextSegment {
                text: &self.text[start..end],
                range: start..end,
                style_index: span.style_index,
                style: self.style(span.style_index),
                font: span.font,
                font_size: span.font_size,
                origin: Point::new(
                    x,
                    line.top + line.line_box.baseline - span.line_box.first_baseline(),
                ),
                line_index,
                line_top: line.top,
                line_height: line.line_box.height,
            })?;
        }
        Some(x - offset)
    }

    pub fn all_styles(&self, matches: impl Fn(&TextStyle) -> bool) -> bool {
        self.styles
            .iter()
            .all(|style| matches(style.as_ref().unwrap_or(self.base_style)))
    }

    fn style(&self, index: usize) -> &TextStyle {
        self.styles[index]
            .as_ref()
            .map_or(self.base_style, |style| style)
    }

    pub fn has_non_solid_brush(&self) -> bool {
        self.styles.iter().any(|style| {
            style
                .as_ref()
                .map_or(self.base_style, |style| style)
                .span_style
                .brush
                .as_ref()
                .is_some_and(|brush| !matches!(brush, Brush::Solid(_)))
        })
    }

    pub fn brush_extents(
        &self,
        line_offsets: Option<&[f32]>,
        scale: f32,
        mut segment_advance: impl FnMut(&AnnotatedTextSegment<'_>) -> f32,
    ) -> SmallVec<[AnnotatedBrushExtent; 4]> {
        let mut extents: SmallVec<[AnnotatedBrushExtent; 4]> = SmallVec::new();
        for line_index in 0..self.lines.len() {
            let offset = line_offsets
                .and_then(|offsets| offsets.get(line_index))
                .copied()
                .unwrap_or(0.0);
            self.walk_line(line_index, offset, |segment| {
                if segment
                    .style
                    .span_style
                    .brush
                    .as_ref()
                    .is_some_and(|brush| !matches!(brush, Brush::Solid(_)))
                {
                    let metrics = measure_text_with_font(
                        segment.text,
                        segment.style,
                        segment.font_size,
                        segment.font,
                    );
                    let rect = Rect {
                        x: segment.origin.x,
                        y: segment.origin.y,
                        width: (metrics.width * scale).ceil().max(1.0),
                        height: (metrics.height * scale).ceil().max(1.0),
                    };
                    if let Some(extent) = extents.last_mut().filter(|extent| {
                        extent.style_index == segment.style_index
                            && extent.line_index == segment.line_index
                    }) {
                        let right = (extent.rect.x + extent.rect.width).max(rect.x + rect.width);
                        let bottom = (extent.rect.y + extent.rect.height).max(rect.y + rect.height);
                        extent.rect.x = extent.rect.x.min(rect.x);
                        extent.rect.y = extent.rect.y.min(rect.y);
                        extent.rect.width = right - extent.rect.x;
                        extent.rect.height = bottom - extent.rect.y;
                    } else {
                        extents.push(AnnotatedBrushExtent {
                            style_index: segment.style_index,
                            line_index: segment.line_index,
                            rect,
                        });
                    }
                }
                Some(segment_advance(&segment))
            });
        }
        extents
    }

    pub fn metrics(&self, width: f32) -> TextMetrics {
        TextMetrics {
            width,
            height: self.lines.last().map_or(0.0, |line| {
                (line.top + line.line_box.height - line.line_box.trim_bottom).max(1.0)
            }),
            line_height: self
                .lines
                .iter()
                .map(|line| line.line_box.height)
                .fold(0.0, f32::max),
            line_count: self.lines.len(),
        }
    }
}

fn merge_extents(left: FontExtent, right: FontExtent) -> FontExtent {
    FontExtent::new(
        left.ascent.max(right.ascent),
        left.descent.max(right.descent),
        left.line_gap.max(right.line_gap),
    )
}
