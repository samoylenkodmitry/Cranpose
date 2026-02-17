use crate::text_layout_result::TextLayoutResult;
use std::cell::RefCell;

use super::layout_options::{TextLayoutOptions, TextOverflow};
use super::style::TextStyle;

const ELLIPSIS: &str = "\u{2026}";
const WRAP_EPSILON: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// Height of a single line of text
    pub line_height: f32,
    /// Number of lines in the text
    pub line_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedTextLayout {
    pub text: String,
    pub metrics: TextMetrics,
    pub did_overflow: bool,
}

pub trait TextMeasurer: 'static {
    fn measure(&self, text: &str, style: &TextStyle) -> TextMetrics;

    fn get_offset_for_position(&self, text: &str, style: &TextStyle, x: f32, y: f32) -> usize;

    fn get_cursor_x_for_offset(&self, text: &str, style: &TextStyle, offset: usize) -> f32;

    fn layout(&self, text: &str, style: &TextStyle) -> TextLayoutResult;

    fn measure_with_options(
        &self,
        text: &str,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.prepare_with_options(text, style, options, max_width)
            .metrics
    }

    fn prepare_with_options(
        &self,
        text: &str,
        style: &TextStyle,
        options: TextLayoutOptions,
        max_width: Option<f32>,
    ) -> PreparedTextLayout {
        prepare_text_layout_fallback(self, text, style, options, max_width)
    }
}

#[derive(Default)]
struct MonospacedTextMeasurer;

impl MonospacedTextMeasurer {
    const DEFAULT_SIZE: f32 = 14.0;
    const CHAR_WIDTH_RATIO: f32 = 0.6; // Width is 0.6 of Height

    fn get_metrics(style: &TextStyle) -> (f32, f32) {
        let font_size = style.resolve_font_size(Self::DEFAULT_SIZE);
        let line_height = style.resolve_line_height(Self::DEFAULT_SIZE, font_size);
        let letter_spacing = style.resolve_letter_spacing(Self::DEFAULT_SIZE).max(0.0);
        (
            (font_size * Self::CHAR_WIDTH_RATIO) + letter_spacing,
            line_height,
        )
    }
}

impl TextMeasurer for MonospacedTextMeasurer {
    fn measure(&self, text: &str, style: &TextStyle) -> TextMetrics {
        let (char_width, line_height) = Self::get_metrics(style);

        let lines: Vec<&str> = text.split('\n').collect();
        let line_count = lines.len().max(1);

        let width = lines
            .iter()
            .map(|line| line.chars().count() as f32 * char_width)
            .fold(0.0_f32, f32::max);

        TextMetrics {
            width,
            height: line_count as f32 * line_height,
            line_height,
            line_count,
        }
    }

    fn get_offset_for_position(&self, text: &str, style: &TextStyle, x: f32, y: f32) -> usize {
        let (char_width, line_height) = Self::get_metrics(style);

        if text.is_empty() {
            return 0;
        }

        let line_index = (y / line_height).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1;
        }

        let line_text = lines.get(target_line).unwrap_or(&"");
        let char_index = (x / char_width).round() as usize;
        let line_char_count = line_text.chars().count();
        let clamped_index = char_index.min(line_char_count);

        let offset_in_line = line_text
            .char_indices()
            .nth(clamped_index)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());

        line_start_byte + offset_in_line
    }

    fn get_cursor_x_for_offset(&self, text: &str, style: &TextStyle, offset: usize) -> f32 {
        let (char_width, _) = Self::get_metrics(style);

        let clamped_offset = offset.min(text.len());
        let char_count = text[..clamped_offset].chars().count();
        char_count as f32 * char_width
    }

    fn layout(&self, text: &str, style: &TextStyle) -> TextLayoutResult {
        let (char_width, line_height) = Self::get_metrics(style);
        TextLayoutResult::monospaced(text, char_width, line_height)
    }
}

thread_local! {
    static TEXT_MEASURER: RefCell<Box<dyn TextMeasurer>> = RefCell::new(Box::new(MonospacedTextMeasurer));
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    TEXT_MEASURER.with(|m| {
        *m.borrow_mut() = Box::new(measurer);
    });
}

pub fn measure_text(text: &str, style: &TextStyle) -> TextMetrics {
    TEXT_MEASURER.with(|m| m.borrow().measure(text, style))
}

pub fn measure_text_with_options(
    text: &str,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> TextMetrics {
    TEXT_MEASURER.with(|m| {
        m.borrow()
            .measure_with_options(text, style, options.normalized(), max_width)
    })
}

pub fn prepare_text_layout(
    text: &str,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    TEXT_MEASURER.with(|m| {
        m.borrow()
            .prepare_with_options(text, style, options.normalized(), max_width)
    })
}

pub fn get_offset_for_position(text: &str, style: &TextStyle, x: f32, y: f32) -> usize {
    TEXT_MEASURER.with(|m| m.borrow().get_offset_for_position(text, style, x, y))
}

pub fn get_cursor_x_for_offset(text: &str, style: &TextStyle, offset: usize) -> f32 {
    TEXT_MEASURER.with(|m| m.borrow().get_cursor_x_for_offset(text, style, offset))
}

pub fn layout_text(text: &str, style: &TextStyle) -> TextLayoutResult {
    TEXT_MEASURER.with(|m| m.borrow().layout(text, style))
}

fn prepare_text_layout_fallback<M: TextMeasurer + ?Sized>(
    measurer: &M,
    text: &str,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> PreparedTextLayout {
    let opts = options.normalized();
    let max_width = normalize_max_width(max_width);
    let wrap_width = (opts.soft_wrap && opts.overflow != TextOverflow::Visible)
        .then_some(max_width)
        .flatten();

    let mut lines = split_text_lines(text);
    if let Some(width_limit) = wrap_width {
        let mut wrapped = Vec::with_capacity(lines.len());
        for line in &lines {
            wrapped.extend(wrap_line_to_width(measurer, line, style, width_limit));
        }
        lines = wrapped;
    }

    let mut did_overflow = false;
    let mut visible_lines = lines.clone();

    if opts.overflow != TextOverflow::Visible && visible_lines.len() > opts.max_lines {
        did_overflow = true;
        visible_lines.truncate(opts.max_lines);
        if let Some(last_line) = visible_lines.last_mut() {
            *last_line =
                apply_line_overflow(measurer, last_line, style, max_width, opts, true, true);
        }
    }

    if let Some(width_limit) = max_width {
        let single_line_ellipsis = opts.max_lines == 1 || !opts.soft_wrap;
        let visible_len = visible_lines.len();
        for (line_index, line) in visible_lines.iter_mut().enumerate() {
            let width = measurer.measure(line, style).width;
            if width > width_limit + WRAP_EPSILON {
                if opts.overflow == TextOverflow::Visible {
                    continue;
                }
                did_overflow = true;
                *line = apply_line_overflow(
                    measurer,
                    line,
                    style,
                    Some(width_limit),
                    opts,
                    line_index + 1 == visible_len,
                    single_line_ellipsis,
                );
            }
        }
    }

    let display_text = visible_lines.join("\n");
    let line_height = measurer.measure("", style).line_height.max(0.0);
    let display_line_count = visible_lines.len().max(1);
    let layout_line_count = display_line_count.max(opts.min_lines);

    let measured_width = if visible_lines.is_empty() {
        0.0
    } else {
        visible_lines
            .iter()
            .map(|line| measurer.measure(line, style).width)
            .fold(0.0_f32, f32::max)
    };
    let width = if opts.overflow == TextOverflow::Visible {
        measured_width
    } else if let Some(width_limit) = max_width {
        measured_width.min(width_limit)
    } else {
        measured_width
    };

    PreparedTextLayout {
        text: display_text,
        metrics: TextMetrics {
            width,
            height: layout_line_count as f32 * line_height,
            line_height,
            line_count: layout_line_count,
        },
        did_overflow,
    }
}

fn split_text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    text.split('\n').map(ToString::to_string).collect()
}

fn normalize_max_width(max_width: Option<f32>) -> Option<f32> {
    match max_width {
        Some(width) if width.is_finite() && width > 0.0 => Some(width),
        _ => None,
    }
}

fn wrap_line_to_width<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let boundaries = char_boundaries(line);
    let mut wrapped = Vec::new();
    let mut start_idx = 0usize;

    while start_idx < boundaries.len() - 1 {
        let mut low = start_idx + 1;
        let mut high = boundaries.len() - 1;
        let mut best = start_idx + 1;

        while low <= high {
            let mid = (low + high) / 2;
            let segment = &line[boundaries[start_idx]..boundaries[mid]];
            let width = measurer.measure(segment, style).width;
            if width <= max_width + WRAP_EPSILON || mid == start_idx + 1 {
                best = mid;
                low = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }

        let wrap_idx = if best < boundaries.len() - 1 {
            find_whitespace_break(line, &boundaries, start_idx, best).unwrap_or(best)
        } else {
            best
        };
        let mut segment = line[boundaries[start_idx]..boundaries[wrap_idx]].to_string();
        if wrap_idx != best {
            segment = segment.trim_end().to_string();
        }
        wrapped.push(segment);

        start_idx = if wrap_idx != best {
            skip_leading_whitespace(line, &boundaries, wrap_idx)
        } else {
            best
        };
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

fn find_whitespace_break(
    line: &str,
    boundaries: &[usize],
    start_idx: usize,
    end_idx: usize,
) -> Option<usize> {
    if end_idx <= start_idx + 1 {
        return None;
    }

    for idx in (start_idx + 1..end_idx).rev() {
        let prev = &line[boundaries[idx - 1]..boundaries[idx]];
        if prev.chars().all(char::is_whitespace) {
            return Some(idx);
        }
    }
    None
}

fn skip_leading_whitespace(line: &str, boundaries: &[usize], mut idx: usize) -> usize {
    while idx < boundaries.len() - 1 {
        let ch = &line[boundaries[idx]..boundaries[idx + 1]];
        if !ch.chars().all(char::is_whitespace) {
            break;
        }
        idx += 1;
    }
    idx
}

fn apply_line_overflow<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: Option<f32>,
    options: TextLayoutOptions,
    is_last_visible_line: bool,
    single_line_ellipsis: bool,
) -> String {
    if options.overflow == TextOverflow::Clip || !is_last_visible_line {
        return line.to_string();
    }

    let Some(width_limit) = max_width else {
        return match options.overflow {
            TextOverflow::Ellipsis => format!("{line}{ELLIPSIS}"),
            TextOverflow::StartEllipsis => format!("{ELLIPSIS}{line}"),
            TextOverflow::MiddleEllipsis => format!("{ELLIPSIS}{line}"),
            TextOverflow::Clip | TextOverflow::Visible => line.to_string(),
        };
    };

    match options.overflow {
        TextOverflow::Clip | TextOverflow::Visible => line.to_string(),
        TextOverflow::Ellipsis => fit_end_ellipsis(measurer, line, style, width_limit),
        TextOverflow::StartEllipsis => {
            if single_line_ellipsis {
                fit_start_ellipsis(measurer, line, style, width_limit)
            } else {
                line.to_string()
            }
        }
        TextOverflow::MiddleEllipsis => {
            if single_line_ellipsis {
                fit_middle_ellipsis(measurer, line, style, width_limit)
            } else {
                line.to_string()
            }
        }
    }
}

fn fit_end_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer.measure(line, style).width <= max_width + WRAP_EPSILON {
        return line.to_string();
    }

    let ellipsis_width = measurer.measure(ELLIPSIS, style).width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    let mut best = 0usize;

    while low <= high {
        let mid = (low + high) / 2;
        let prefix = &line[..boundaries[mid]];
        let candidate = format!("{prefix}{ELLIPSIS}");
        let width = measurer.measure(&candidate, style).width;
        if width <= max_width + WRAP_EPSILON {
            best = mid;
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }

    format!("{}{}", &line[..boundaries[best]], ELLIPSIS)
}

fn fit_start_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer.measure(line, style).width <= max_width + WRAP_EPSILON {
        return line.to_string();
    }

    let ellipsis_width = measurer.measure(ELLIPSIS, style).width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    let mut best = boundaries.len() - 1;

    while low <= high {
        let mid = (low + high) / 2;
        let suffix = &line[boundaries[mid]..];
        let candidate = format!("{ELLIPSIS}{suffix}");
        let width = measurer.measure(&candidate, style).width;
        if width <= max_width + WRAP_EPSILON {
            best = mid;
            if mid == 0 {
                break;
            }
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }

    format!("{ELLIPSIS}{}", &line[boundaries[best]..])
}

fn fit_middle_ellipsis<M: TextMeasurer + ?Sized>(
    measurer: &M,
    line: &str,
    style: &TextStyle,
    max_width: f32,
) -> String {
    if measurer.measure(line, style).width <= max_width + WRAP_EPSILON {
        return line.to_string();
    }

    let ellipsis_width = measurer.measure(ELLIPSIS, style).width;
    if ellipsis_width > max_width + WRAP_EPSILON {
        return String::new();
    }

    let boundaries = char_boundaries(line);
    let total_chars = boundaries.len().saturating_sub(1);
    for keep in (0..=total_chars).rev() {
        let keep_start = keep.div_ceil(2);
        let keep_end = keep / 2;
        let start = &line[..boundaries[keep_start]];
        let end_start = boundaries[total_chars.saturating_sub(keep_end)];
        let end = &line[end_start..];
        let candidate = format!("{start}{ELLIPSIS}{end}");
        if measurer.measure(&candidate, style).width <= max_width + WRAP_EPSILON {
            return candidate;
        }
    }

    ELLIPSIS.to_string()
}

fn char_boundaries(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.chars().count() + 1);
    out.push(0);
    for (idx, _) in text.char_indices() {
        if idx != 0 {
            out.push(idx);
        }
    }
    out.push(text.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextUnit;

    #[test]
    fn text_layout_options_wraps_and_limits_lines() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: 2,
            min_lines: 1,
        };

        let prepared = prepare_text_layout(
            "A B C D E F",
            &style,
            options,
            Some(24.0), // roughly 4 chars in monospaced fallback
        );

        assert!(prepared.did_overflow);
        assert!(prepared.metrics.line_count <= 2);
    }

    #[test]
    fn text_layout_options_end_ellipsis_applies() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Ellipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout("Long long line", &style, options, Some(20.0));
        assert!(prepared.did_overflow);
        assert!(prepared.text.contains(ELLIPSIS));
    }

    #[test]
    fn text_layout_options_visible_keeps_full_text() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Visible,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let input = "This should remain unchanged";
        let prepared = prepare_text_layout(input, &style, options, Some(10.0));
        assert_eq!(prepared.text, input);
    }

    #[test]
    fn text_layout_options_respects_min_lines() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: 4,
            min_lines: 3,
        };

        let prepared = prepare_text_layout("short", &style, options, Some(100.0));
        assert_eq!(prepared.metrics.line_count, 3);
    }

    #[test]
    fn text_layout_options_middle_ellipsis_for_single_line() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::MiddleEllipsis,
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        };

        let prepared = prepare_text_layout("abcdefghijk", &style, options, Some(24.0));
        assert!(prepared.text.contains(ELLIPSIS));
        assert!(prepared.did_overflow);
    }

    #[test]
    fn text_layout_options_does_not_wrap_on_tiny_width_delta() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        };

        let text = "if counter % 2 == 0";
        let exact_width = measure_text(text, &style).width;
        let prepared = prepare_text_layout(text, &style, options, Some(exact_width - 0.1));

        assert!(
            !prepared.text.contains('\n'),
            "unexpected line split: {:?}",
            prepared.text
        );
    }
}
