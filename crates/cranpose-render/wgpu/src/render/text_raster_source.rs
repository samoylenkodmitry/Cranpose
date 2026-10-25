use std::{borrow::Cow, sync::Arc};

use cranpose_ui_graphics::Rect;

use crate::scene::{DrawnText, TextDraw};

const MIN_MULTILINE_TEXT_LINES_FOR_CLIPPED_RASTER: usize = 2;

pub(super) struct TextRasterSource<'a> {
    pub(super) draw: Cow<'a, TextDraw>,
    pub(super) raster_rect: Rect,
}

pub(super) fn clipped_text_raster_source_with_line_starts<'a>(
    text_draw: &'a TextDraw,
    logical_rect: Rect,
    raster_rect: Rect,
    clip: Rect,
    root_scale: f32,
    line_starts: &[usize],
) -> TextRasterSource<'a> {
    if line_starts.len() < MIN_MULTILINE_TEXT_LINES_FOR_CLIPPED_RASTER {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let Some(visible_rect) = logical_rect.intersect(clip) else {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    };

    let line_count = line_starts.len().max(1);
    let line_height = logical_rect.height / line_count as f32;
    if !line_height.is_finite() || line_height <= 0.0 {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let visible_top = ((visible_rect.y - logical_rect.y) / line_height).floor() as isize;
    let visible_bottom =
        ((visible_rect.y + visible_rect.height - logical_rect.y) / line_height).ceil() as isize;
    let start_line = visible_top.saturating_sub(1).max(0) as usize;
    let end_line = (visible_bottom + 1).max(start_line as isize + 1) as usize;
    let end_line = end_line.min(line_count);
    if start_line == 0 && end_line >= line_count {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let byte_start = line_starts[start_line];
    let byte_end = line_end_offset(text_draw.text.string().text(), line_starts, end_line - 1);
    if byte_start >= byte_end {
        return TextRasterSource {
            draw: Cow::Borrowed(text_draw),
            raster_rect,
        };
    }

    let slice_y = logical_rect.y + start_line as f32 * line_height;
    let slice_height = (end_line - start_line) as f32 * line_height;
    let mut slice_raster_rect = Rect {
        x: logical_rect.x * root_scale,
        y: slice_y * root_scale,
        width: logical_rect.width * root_scale,
        height: slice_height * root_scale,
    };
    slice_raster_rect.x = slice_raster_rect.x.round();
    slice_raster_rect.y = slice_raster_rect.y.round();
    slice_raster_rect.width = slice_raster_rect.width.ceil().max(1.0);
    slice_raster_rect.height = slice_raster_rect.height.ceil().max(1.0);

    let mut sliced_draw = text_draw.clone();
    sliced_draw.rect = Rect {
        x: logical_rect.x,
        y: slice_y,
        width: logical_rect.width,
        height: slice_height,
    };
    sliced_draw.text = DrawnText::of(
        Arc::new(text_draw.text.string().subsequence(byte_start..byte_end)),
        &text_draw.text_style,
    );

    TextRasterSource {
        draw: Cow::Owned(sliced_draw),
        raster_rect: slice_raster_rect,
    }
}

pub(super) fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts =
        Vec::with_capacity(text.as_bytes().iter().filter(|b| **b == b'\n').count() + 1);
    starts.push(0);
    starts.extend(
        text.char_indices()
            .filter_map(|(index, ch)| (ch == '\n').then_some(index + ch.len_utf8())),
    );
    starts
}

fn line_end_offset(text: &str, line_starts: &[usize], line: usize) -> usize {
    line_starts.get(line + 1).copied().unwrap_or(text.len())
}
