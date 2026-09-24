use std::rc::Rc;

#[cfg(test)]
use cranpose_render_common::text_measure::{
    fallback_char_width, fallback_cursor_x_for_byte_offset, fallback_line_height,
    fallback_text_metrics,
};
use cranpose_render_common::{
    brush_sampling::sample_brush_rgba, graph_scene::RenderDiagnostics, shape_sdf,
    software_text_raster::rasterize_text_to_image, text_measure::SoftwareTextResources,
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{BlendMode, ColorFilter, Point, Rect};

use crate::{
    pipeline,
    scene::{ImageDraw, RasterScene, Scene, TextDraw},
    style::point_in_resolved_rounded_rect,
};

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

fn snapped_anchor_origin(anchor: Point) -> Point {
    Point::new(anchor.x.round(), anchor.y.round())
}

fn snap_delta_for_anchor(anchor: Point) -> Point {
    let snapped = snapped_anchor_origin(anchor);
    Point::new(snapped.x - anchor.x, snapped.y - anchor.y)
}

#[derive(Clone, Copy)]
struct ClipBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

fn clip_rect_to_bounds(
    rect: Rect,
    clip: Option<Rect>,
    width: u32,
    height: u32,
) -> Option<ClipBounds> {
    let mut min_x = rect.x;
    let mut min_y = rect.y;
    let mut max_x = rect.x + rect.width;
    let mut max_y = rect.y + rect.height;

    if let Some(clip_rect) = clip {
        min_x = min_x.max(clip_rect.x);
        min_y = min_y.max(clip_rect.y);
        max_x = max_x.min(clip_rect.x + clip_rect.width);
        max_y = max_y.min(clip_rect.y + clip_rect.height);
    }

    min_x = min_x.max(0.0);
    min_y = min_y.max(0.0);
    max_x = max_x.min(width as f32);
    max_y = max_y.min(height as f32);

    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let min_x = min_x.floor() as i32;
    let min_y = min_y.floor() as i32;
    let max_x = max_x.ceil() as i32;
    let max_y = max_y.ceil() as i32;

    let min_x = min_x.clamp(0, width as i32);
    let min_y = min_y.clamp(0, height as i32);
    let max_x = max_x.clamp(0, width as i32);
    let max_y = max_y.clamp(0, height as i32);

    if min_x >= max_x || min_y >= max_y {
        return None;
    }

    Some(ClipBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

pub fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    let text_resources = SoftwareTextResources::default();
    draw_scene_with_text_resources(frame, width, height, scene, &text_resources);
}

pub fn draw_scene_with_text_resources(
    frame: &mut [u8],
    width: u32,
    height: u32,
    scene: &Scene,
    text_resources: &SoftwareTextResources,
) {
    if let Some(graph) = scene.graph.as_ref() {
        let raster_scene = pipeline::build_raster_scene(graph, scene.diagnostics());
        draw_raster_scene(
            frame,
            width,
            height,
            &raster_scene,
            scene.diagnostics(),
            text_resources,
        );
    } else {
        clear_frame(frame);
    }
}

fn clear_frame(frame: &mut [u8]) {
    for chunk in frame.as_chunks_mut::<4>().0 {
        chunk.copy_from_slice(&[18, 18, 24, 255]);
    }
}

fn draw_raster_scene(
    frame: &mut [u8],
    width: u32,
    height: u32,
    scene: &RasterScene,
    diagnostics: &RenderDiagnostics,
    text_resources: &SoftwareTextResources,
) {
    clear_frame(frame);
    let mut ordered_items =
        Vec::with_capacity(scene.shapes.len() + scene.images.len() + scene.texts.len());
    for (index, shape) in scene.shapes.iter().enumerate() {
        ordered_items.push((shape.z_index, RenderItem::Shape(index)));
    }
    for (index, image) in scene.images.iter().enumerate() {
        ordered_items.push((image.z_index, RenderItem::Image(index)));
    }
    for (index, text) in scene.texts.iter().enumerate() {
        ordered_items.push((text.z_index, RenderItem::Text(index)));
    }
    ordered_items.sort_unstable_by_key(|(z, _)| *z);

    for (_, item) in ordered_items {
        match item {
            RenderItem::Shape(index) => {
                draw_shape(frame, width, height, &scene.shapes[index], diagnostics);
            }
            RenderItem::Image(index) => {
                draw_image(frame, width, height, &scene.images[index], diagnostics);
            }
            RenderItem::Text(index) => {
                draw_text(
                    frame,
                    width,
                    height,
                    &scene.texts[index],
                    diagnostics,
                    text_resources,
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderItem {
    Shape(usize),
    Image(usize),
    Text(usize),
}

fn draw_shape(
    frame: &mut [u8],
    width: u32,
    height: u32,
    draw: &crate::scene::DrawShape,
    diagnostics: &RenderDiagnostics,
) {
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    let rect = draw.rect.translate(snap_delta.x, snap_delta.y);
    let clip = draw.clip;
    let dither_origin = draw
        .snap_anchor
        .map(snapped_anchor_origin)
        .unwrap_or_default();
    let rect = if draw.snap_to_pixel_grid {
        Rect {
            x: rect.x.round(),
            y: rect.y.round(),
            width: if rect.width > 0.0 {
                rect.width.ceil().max(1.0)
            } else {
                rect.width
            },
            height: if rect.height > 0.0 {
                rect.height.ceil().max(1.0)
            } else {
                rect.height
            },
        }
    } else {
        rect
    };
    if draw.arc.is_some_and(|arc| arc.is_degenerate())
        || draw.stroke.is_some_and(|stroke| !stroke.is_visible())
    {
        return;
    }

    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, width, height) else {
        return;
    };
    let Rect {
        width: rect_width,
        height: rect_height,
        ..
    } = rect;

    let arc = draw.arc.map(|mut arc| {
        arc.center.x += snap_delta.x;
        arc.center.y += snap_delta.y;
        arc
    });
    let stroke = draw.stroke;
    let stroke_outset = stroke.map_or(0.0, |stroke| stroke.half_width());
    let resolved_shape = draw.shape.map(|shape| {
        shape.resolve(
            (rect_width - stroke_outset * 2.0).max(0.0),
            (rect_height - stroke_outset * 2.0).max(0.0),
        )
    });

    for py in clip_bounds.min_y..clip_bounds.max_y {
        if py < 0 || py >= height as i32 {
            continue;
        }
        for px in clip_bounds.min_x..clip_bounds.max_x {
            if px < 0 || px >= width as i32 {
                continue;
            }
            let center_x = px as f32 + 0.5;
            let center_y = py as f32 + 0.5;

            let coverage = if let Some(arc) = arc.as_ref() {
                shape_sdf::arc_coverage(Point::new(center_x, center_y), arc)
            } else if let Some(stroke) = stroke {
                shape_sdf::stroked_rect_coverage(
                    Point::new(center_x, center_y),
                    rect,
                    resolved_shape,
                    stroke.half_width(),
                    stroke.join,
                )
            } else {
                if let Some(ref radii) = resolved_shape
                    && !point_in_resolved_rounded_rect(center_x, center_y, rect, radii)
                {
                    continue;
                }
                1.0
            };
            if coverage <= 0.0 {
                continue;
            }

            let mut sample =
                sample_brush_rgba(&draw.brush, rect, center_x, center_y, dither_origin);
            sample[3] *= coverage;
            let alpha = sample[3];
            if alpha <= 0.0 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            blend_pixel(
                &mut frame[idx..idx + 4],
                sample,
                draw.blend_mode,
                diagnostics,
            );
        }
    }
}

fn draw_image(
    frame: &mut [u8],
    width: u32,
    height: u32,
    draw: &ImageDraw,
    diagnostics: &RenderDiagnostics,
) {
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    let rect = draw.rect.translate(snap_delta.x, snap_delta.y);
    let clip = draw.clip;

    if draw.alpha <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, width, height) else {
        return;
    };

    let img_width = draw.image.width();
    let img_height = draw.image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }
    let src_pixels = draw.image.pixels();

    let (sr_x, sr_y, sr_w, sr_h) = if let Some(sr) = draw.src_rect {
        (sr.x, sr.y, sr.width, sr.height)
    } else {
        (0.0, 0.0, img_width as f32, img_height as f32)
    };

    for py in clip_bounds.min_y..clip_bounds.max_y {
        for px in clip_bounds.min_x..clip_bounds.max_x {
            let sample_x = px as f32 + 0.5;
            let sample_y = py as f32 + 0.5;
            let u = ((sample_x - rect.x) / rect.width).clamp(0.0, 1.0);
            let v = ((sample_y - rect.y) / rect.height).clamp(0.0, 1.0);

            let mut sample = match draw.sampling {
                cranpose_ui_graphics::ImageSampling::Nearest => {
                    let src_x = ((sr_x + u * sr_w).floor() as i32).clamp(0, img_width as i32 - 1);
                    let src_y = ((sr_y + v * sr_h).floor() as i32).clamp(0, img_height as i32 - 1);
                    sample_image_nearest(src_pixels, img_width, src_x as u32, src_y as u32)
                }
                cranpose_ui_graphics::ImageSampling::Linear => sample_image_linear(
                    src_pixels,
                    img_width,
                    img_height,
                    sr_x + u * sr_w - 0.5,
                    sr_y + v * sr_h - 0.5,
                ),
            };

            if let Some(filter) = draw.color_filter {
                sample = apply_color_filter(sample, filter);
            }

            sample[3] *= draw.alpha.clamp(0.0, 1.0);
            if sample[3] <= 0.0 {
                continue;
            }

            let dst_idx = ((py as u32 * width + px as u32) * 4) as usize;
            blend_pixel(
                &mut frame[dst_idx..dst_idx + 4],
                sample,
                draw.blend_mode,
                diagnostics,
            );
        }
    }
}

fn sample_image_nearest(src_pixels: &[u8], img_width: u32, src_x: u32, src_y: u32) -> [f32; 4] {
    let src_idx = ((src_y * img_width + src_x) * 4) as usize;
    [
        src_pixels[src_idx] as f32 / 255.0,
        src_pixels[src_idx + 1] as f32 / 255.0,
        src_pixels[src_idx + 2] as f32 / 255.0,
        src_pixels[src_idx + 3] as f32 / 255.0,
    ]
}

fn sample_image_linear(
    src_pixels: &[u8],
    img_width: u32,
    img_height: u32,
    x: f32,
    y: f32,
) -> [f32; 4] {
    let x = x.clamp(0.0, img_width.saturating_sub(1) as f32);
    let y = y.clamp(0.0, img_height.saturating_sub(1) as f32);
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let x0 = (x0 as i32).clamp(0, img_width as i32 - 1) as u32;
    let y0 = (y0 as i32).clamp(0, img_height as i32 - 1) as u32;
    let x1 = (x0 + 1).min(img_width - 1);
    let y1 = (y0 + 1).min(img_height - 1);
    let top_left = sample_image_nearest(src_pixels, img_width, x0, y0);
    let top_right = sample_image_nearest(src_pixels, img_width, x1, y0);
    let bottom_left = sample_image_nearest(src_pixels, img_width, x0, y1);
    let bottom_right = sample_image_nearest(src_pixels, img_width, x1, y1);

    let mut out = [0.0; 4];
    for channel in 0..4 {
        let top = top_left[channel] + (top_right[channel] - top_left[channel]) * tx;
        let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * tx;
        out[channel] = top + (bottom - top) * ty;
    }
    out
}

fn draw_text(
    frame: &mut [u8],
    width: u32,
    height: u32,
    draw: &TextDraw,
    diagnostics: &RenderDiagnostics,
    text_resources: &SoftwareTextResources,
) {
    if draw.text.span_styles.is_empty() {
        draw_text_plain(frame, width, height, draw, diagnostics, text_resources);
        return;
    }

    draw_text_with_span_styles(frame, width, height, draw, diagnostics, text_resources);
}

fn draw_text_with_span_styles(
    frame: &mut [u8],
    width: u32,
    height: u32,
    draw: &TextDraw,
    diagnostics: &RenderDiagnostics,
    text_resources: &SoftwareTextResources,
) {
    let boundaries = draw.text.span_boundaries();
    let mut cursor_x = draw.rect.x;
    let mut cursor_y = draw.rect.y;
    let base_line_height = draw
        .text_style
        .resolve_line_height(14.0, draw.font_size)
        .max(1.0);
    let mut current_line_height = base_line_height;

    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let chunk = &draw.text.text[start..end];
        let mut merged_span = draw.text_style.span_style.clone();
        for span in &draw.text.span_styles {
            if span.range.start <= start && span.range.end >= end {
                merged_span = merged_span.merge(&span.item);
            }
        }

        let mut chunk_style = draw.text_style.clone();
        chunk_style.span_style = merged_span;

        for part in chunk.split_inclusive('\n') {
            let has_newline = part.ends_with('\n');
            let content = if has_newline {
                &part[..part.len().saturating_sub(1)]
            } else {
                part
            };

            if !content.is_empty() {
                let segment = cranpose_ui::text::AnnotatedString::from(content);
                let metrics = cranpose_ui::text::measure_text(&segment, &chunk_style);
                let segment_draw = TextDraw {
                    node_id: draw.node_id,
                    rect: Rect {
                        x: cursor_x,
                        y: cursor_y,
                        width: metrics.width.max(1.0),
                        height: metrics.height.max(1.0),
                    },
                    snap_anchor: draw.snap_anchor,
                    text: Rc::new(segment),
                    color: chunk_style.resolve_text_color(draw.color),
                    text_style: chunk_style.clone(),
                    font_size: chunk_style.resolve_font_size(draw.font_size),
                    scale: draw.scale,
                    layout_options: draw.layout_options,
                    z_index: draw.z_index,
                    clip: draw.clip,
                };
                draw_text_plain(
                    frame,
                    width,
                    height,
                    &segment_draw,
                    diagnostics,
                    text_resources,
                );
                cursor_x += metrics.width;
                current_line_height = current_line_height.max(metrics.line_height.max(1.0));
            }

            if has_newline {
                cursor_x = draw.rect.x;
                cursor_y += current_line_height;
                current_line_height = base_line_height;
            }
        }
    }
}

fn draw_text_plain(
    frame: &mut [u8],
    width: u32,
    height: u32,
    draw: &TextDraw,
    diagnostics: &RenderDiagnostics,
    text_resources: &SoftwareTextResources,
) {
    let text_scale = draw.scale.max(0.0);
    if text_scale == 0.0 {
        return;
    }

    let static_text_motion = draw
        .text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;
    let snap_delta = if static_text_motion {
        draw.snap_anchor
            .map(snap_delta_for_anchor)
            .unwrap_or_default()
    } else {
        Point::default()
    };
    let rect = draw.rect.translate(snap_delta.x, snap_delta.y);
    let clip = draw.clip;

    let raster_rect = if static_text_motion {
        Rect {
            x: rect.x.round(),
            y: rect.y.round(),
            width: if rect.width > 0.0 {
                rect.width.ceil().max(1.0)
            } else {
                rect.width
            },
            height: if rect.height > 0.0 {
                rect.height.ceil().max(1.0)
            } else {
                rect.height
            },
        }
    } else {
        rect
    };

    let Some(font) = text_resources.fonts().resolve(&draw.text_style) else {
        return;
    };

    let Some(image) = rasterize_text_to_image(
        draw.text.text.as_str(),
        raster_rect,
        &draw.text_style,
        draw.color,
        draw.font_size,
        text_scale,
        font,
    ) else {
        return;
    };

    let blit_origin = if static_text_motion {
        Point::new(raster_rect.x, raster_rect.y)
    } else {
        Point::new(rect.x, rect.y)
    };
    let blit_rect = Rect {
        x: blit_origin.x,
        y: blit_origin.y,
        width: image.width() as f32,
        height: image.height() as f32,
    };

    blit_rasterized_text_image(frame, width, height, blit_rect, clip, &image, diagnostics);
}

fn blit_rasterized_text_image(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    clip: Option<Rect>,
    image: &cranpose_ui_graphics::ImageBitmap,
    diagnostics: &RenderDiagnostics,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, width, height) else {
        return;
    };

    let img_width = image.width();
    let img_height = image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }
    let src_pixels = image.pixels();

    for py in clip_bounds.min_y..clip_bounds.max_y {
        for px in clip_bounds.min_x..clip_bounds.max_x {
            let sample_x = px as f32 + 0.5;
            let sample_y = py as f32 + 0.5;
            let u = ((sample_x - rect.x) / rect.width).clamp(0.0, 1.0);
            let v = ((sample_y - rect.y) / rect.height).clamp(0.0, 1.0);

            let src = sample_image_linear(
                src_pixels,
                img_width,
                img_height,
                u * img_width.saturating_sub(1) as f32,
                v * img_height.saturating_sub(1) as f32,
            );
            if src[3] <= 0.0 {
                continue;
            }

            let dst_idx = ((py as u32 * width + px as u32) * 4) as usize;
            blend_pixel(
                &mut frame[dst_idx..dst_idx + 4],
                src,
                BlendMode::SrcOver,
                diagnostics,
            );
        }
    }
}

fn blend_pixel(
    dst: &mut [u8],
    src: [f32; 4],
    blend_mode: BlendMode,
    diagnostics: &RenderDiagnostics,
) {
    let resolved_blend_mode = if is_blend_mode_supported(blend_mode) {
        blend_mode
    } else {
        if diagnostics.claim_warning_once("pixels.unsupported-blend-mode") {
            log::warn!(
                "Pixels renderer currently supports BlendMode::SrcOver and BlendMode::DstOut; falling back to SrcOver for unsupported modes"
            );
        }
        BlendMode::SrcOver
    };

    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }
    let dst_r = dst[0] as f32 / 255.0;
    let dst_g = dst[1] as f32 / 255.0;
    let dst_b = dst[2] as f32 / 255.0;
    let dst_a = dst[3] as f32 / 255.0;

    let (out_r, out_g, out_b, out_a) = match resolved_blend_mode {
        BlendMode::DstOut => {
            let keep = 1.0 - src_alpha;
            (dst_r * keep, dst_g * keep, dst_b * keep, dst_a * keep)
        }
        BlendMode::SrcOver => (
            src[0].clamp(0.0, 1.0) * src_alpha + dst_r * (1.0 - src_alpha),
            src[1].clamp(0.0, 1.0) * src_alpha + dst_g * (1.0 - src_alpha),
            src[2].clamp(0.0, 1.0) * src_alpha + dst_b * (1.0 - src_alpha),
            src_alpha + dst_a * (1.0 - src_alpha),
        ),
        _ => (
            src[0].clamp(0.0, 1.0) * src_alpha + dst_r * (1.0 - src_alpha),
            src[1].clamp(0.0, 1.0) * src_alpha + dst_g * (1.0 - src_alpha),
            src[2].clamp(0.0, 1.0) * src_alpha + dst_b * (1.0 - src_alpha),
            src_alpha + dst_a * (1.0 - src_alpha),
        ),
    };

    dst[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn apply_color_filter(sample: [f32; 4], filter: ColorFilter) -> [f32; 4] {
    filter.apply_rgba(sample)
}

#[cfg(test)]
#[path = "tests/draw_tests.rs"]
mod tests;
