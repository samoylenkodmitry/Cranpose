use std::rc::Rc;

use cranpose_render_common::brush_sampling::sample_brush_rgba;
use cranpose_render_common::graph_scene::RenderDiagnostics;
use cranpose_render_common::shape_sdf;
use cranpose_render_common::software_text_raster::rasterize_text_to_image;
use cranpose_render_common::text_measure::SoftwareTextResources;
#[cfg(test)]
use cranpose_render_common::text_measure::{
    fallback_char_width, fallback_cursor_x_for_byte_offset, fallback_line_height,
    fallback_text_metrics,
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{BlendMode, ColorFilter, Point, Rect};

use crate::pipeline;
use crate::scene::{ImageDraw, RasterScene, Scene, TextDraw};
use crate::style::point_in_resolved_rounded_rect;

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

fn snap_delta_for_anchor(anchor: Point) -> Point {
    Point::new(anchor.x.round() - anchor.x, anchor.y.round() - anchor.y)
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
    for chunk in frame.chunks_exact_mut(4) {
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
    // Unique z per item (the scene's `next_z` counter), so unstable sorting is
    // order-identical and avoids the stable sort's per-frame scratch allocation.
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
    // Clips are already resolved in scene space and may belong to a fixed
    // ancestor. Only the draw geometry borrows this item's raster snap.
    let clip = draw.clip;
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
    // Empty geometry draws nothing. The shared lowering already drops these,
    // but a `DrawShape` can also be built directly (shadow casters, caches), so
    // guard here too rather than emitting a hairline for a zero-area band.
    if draw.arc.is_some_and(|arc| arc.is_degenerate())
        || draw.stroke.is_some_and(|stroke| !stroke.is_visible())
    {
        return;
    }

    let clip_bounds = match clip_rect_to_bounds(rect, clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };
    let Rect {
        width: rect_width,
        height: rect_height,
        ..
    } = rect;

    // Stroked outlines and arcs are rasterized from the very same signed
    // distance functions the GPU shader evaluates (see
    // `cranpose_render_common::shape_sdf`), so the software backend draws the
    // real shape instead of degrading a stroke to a filled box.
    let arc = draw.arc.map(|mut arc| {
        arc.center.x += snap_delta.x;
        arc.center.y += snap_delta.y;
        arc
    });
    let stroke = draw.stroke;
    // For a stroked shape `rect` is already inflated by half the stroke width,
    // so corner radii must resolve against the geometry that was asked for.
    let stroke_outset = stroke.map(|stroke| stroke.half_width()).unwrap_or(0.0);
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
                if let Some(ref radii) = resolved_shape {
                    if !point_in_resolved_rounded_rect(center_x, center_y, rect, radii) {
                        continue;
                    }
                }
                1.0
            };
            if coverage <= 0.0 {
                continue;
            }

            let mut sample = sample_brush_rgba(&draw.brush, rect, center_x, center_y);
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

    let clip_bounds = match clip_rect_to_bounds(rect, clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };

    let img_width = draw.image.width();
    let img_height = draw.image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }
    let src_pixels = draw.image.pixels();

    // Source region: either a sub-rect or the full image
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
    let clip_bounds = match clip_rect_to_bounds(rect, clip, width, height) {
        Some(bounds) => bounds,
        None => return,
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
mod tests {
    use super::*;
    use cranpose_render_common::brush_sampling::normalize_gradient_t;
    use cranpose_render_common::graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    };
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui::Brush;
    use cranpose_ui_graphics::{Color, TileMode};

    fn draw_raster_scene_for_test(frame: &mut [u8], width: u32, height: u32, scene: &RasterScene) {
        let diagnostics = RenderDiagnostics::new();
        let text_resources = SoftwareTextResources::default();
        draw_raster_scene(frame, width, height, scene, &diagnostics, &text_resources);
    }

    #[test]
    fn fallback_text_metrics_cover_empty_and_multiline_text() {
        let empty = fallback_text_metrics("", 10.0);
        assert_eq!(empty.line_count, 1);
        assert_eq!(empty.width, 0.0);
        assert_eq!(empty.height, fallback_line_height(10.0));

        let multiline = fallback_text_metrics("ab\ncde", 10.0);
        assert_eq!(multiline.line_count, 2);
        assert_eq!(multiline.width, 3.0 * fallback_char_width(10.0));
        assert_eq!(multiline.height, 2.0 * fallback_line_height(10.0));
    }

    #[test]
    fn fallback_cursor_position_handles_non_boundary_byte_offsets() {
        let text = "éx";
        let width = fallback_char_width(12.0);
        assert_eq!(fallback_cursor_x_for_byte_offset(text, 0, 12.0), 0.0);
        assert_eq!(fallback_cursor_x_for_byte_offset(text, 1, 12.0), width);
        assert_eq!(
            fallback_cursor_x_for_byte_offset(text, text.len(), 12.0),
            width * 2.0
        );
    }

    #[test]
    fn shape_snap_does_not_move_its_fixed_ancestor_clip() {
        let draw = crate::scene::DrawShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            snap_anchor: Some(Point::new(0.4, 0.4)),
            snap_to_pixel_grid: false,
            brush: Brush::solid(Color::WHITE),
            shape: None,
            stroke: None,
            arc: None,
            z_index: 0,
            clip: Some(Rect {
                x: 2.0,
                y: 2.0,
                width: 2.0,
                height: 2.0,
            }),
            blend_mode: BlendMode::SrcOver,
        };
        let mut frame = vec![0; 8 * 8 * 4];
        draw_shape(&mut frame, 8, 8, &draw, &RenderDiagnostics::new());

        let alpha = |x: usize, y: usize| frame[(y * 8 + x) * 4 + 3];
        assert_eq!(alpha(1, 2), 0, "content snapping moved the clip left");
        assert_eq!(alpha(2, 2), 255, "the fixed clip must retain its coverage");
    }

    fn count_non_background_pixels(frame: &[u8], width: u32, height: u32) -> usize {
        count_non_background_pixels_in_band(frame, width, 0, height)
    }

    fn render_single_text_frame(
        style: cranpose_ui::TextStyle,
        color: Color,
        x: f32,
    ) -> (u32, u32, Vec<u8>) {
        let mut raster_scene = RasterScene::new();
        raster_scene.push_text(
            11,
            Rect {
                x,
                y: 16.0,
                width: 320.0,
                height: 90.0,
            },
            Rc::new(cranpose_ui::text::AnnotatedString::from("MMMMMMMM")),
            color,
            style,
            64.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            None,
        );

        let width = 360;
        let height = 140;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_raster_scene_for_test(&mut frame, width, height, &raster_scene);
        (width, height, frame)
    }

    fn average_ink_rgb(
        frame: &[u8],
        width: u32,
        x_min: u32,
        x_max: u32,
        y_min: u32,
        y_max: u32,
    ) -> Option<[f32; 3]> {
        let mut sum_r = 0.0f32;
        let mut sum_g = 0.0f32;
        let mut sum_b = 0.0f32;
        let mut count = 0usize;

        for y in y_min..y_max {
            for x in x_min..x_max {
                let idx = ((y * width + x) * 4) as usize;
                let px = &frame[idx..idx + 4];
                if px == [18, 18, 24, 255] {
                    continue;
                }
                sum_r += px[0] as f32 / 255.0;
                sum_g += px[1] as f32 / 255.0;
                sum_b += px[2] as f32 / 255.0;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }
        Some([
            sum_r / count as f32,
            sum_g / count as f32,
            sum_b / count as f32,
        ])
    }

    fn count_non_background_pixels_in_band(
        frame: &[u8],
        width: u32,
        y_min_inclusive: u32,
        y_max_exclusive: u32,
    ) -> usize {
        let mut count = 0usize;
        for y in y_min_inclusive..y_max_exclusive {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                let px = &frame[idx..idx + 4];
                if px != [18, 18, 24, 255] {
                    count += 1;
                }
            }
        }
        count
    }

    /// Returns `(top_y, bottom_y)` (exclusive) of all non-background ink rows.
    fn ink_y_range(frame: &[u8], width: u32, height: u32) -> Option<(u32, u32)> {
        let mut top = None;
        let mut bottom = 0u32;
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                if frame[idx..idx + 4] != [18, 18, 24, 255] {
                    top.get_or_insert(y);
                    bottom = y + 1;
                    break;
                }
            }
        }
        top.map(|t| (t, bottom))
    }

    #[test]
    fn blend_mode_support_matrix_is_explicit() {
        assert!(is_blend_mode_supported(BlendMode::SrcOver));
        assert!(is_blend_mode_supported(BlendMode::DstOut));
        assert!(!is_blend_mode_supported(BlendMode::Clear));
        assert!(!is_blend_mode_supported(BlendMode::Multiply));
    }

    #[test]
    fn unsupported_blend_mode_falls_back_without_abort() {
        let diagnostics = RenderDiagnostics::new();
        let src = [1.0, 0.0, 0.0, 0.5];
        let mut unsupported = [0, 0, 255, 255];
        let mut src_over = unsupported;

        blend_pixel(&mut unsupported, src, BlendMode::Multiply, &diagnostics);
        blend_pixel(&mut src_over, src, BlendMode::SrcOver, &diagnostics);

        assert_eq!(unsupported, src_over);
    }

    #[test]
    fn mirror_tile_mode_reflects_second_interval() {
        assert_eq!(normalize_gradient_t(1.25, TileMode::Mirror), Some(0.75));
        assert_eq!(normalize_gradient_t(1.75, TileMode::Mirror), Some(0.25));
    }

    #[test]
    fn multiline_text_renders_second_line_pixels() {
        let mut raster_scene = RasterScene::new();
        raster_scene.push_text(
            1,
            Rect {
                x: 8.0,
                y: 8.0,
                width: 180.0,
                height: 80.0,
            },
            Rc::new(cranpose_ui::text::AnnotatedString::from(
                "Dynamic\nModifiers",
            )),
            Color::WHITE,
            cranpose_ui::TextStyle::default(),
            14.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            None,
        );

        let width = 220;
        let height = 100;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_raster_scene_for_test(&mut frame, width, height, &raster_scene);

        // Find the y-range of all ink pixels (font-agnostic approach).
        let (ink_top, ink_bottom) =
            ink_y_range(&frame, width, height).expect("expected ink pixels in rendered text");
        let ink_height = ink_bottom - ink_top;
        assert!(
            ink_height >= 20,
            "expected two lines of ink, ink spans only {ink_height}px (y={ink_top}..{ink_bottom})"
        );
        let mid_y = ink_top + ink_height / 2;
        let first_line_ink = count_non_background_pixels_in_band(&frame, width, ink_top, mid_y);
        let second_line_ink = count_non_background_pixels_in_band(&frame, width, mid_y, ink_bottom);
        assert!(
            first_line_ink > 20,
            "expected first line to render, got {first_line_ink}"
        );
        assert!(
            second_line_ink > 20,
            "expected second line ink, got {second_line_ink}"
        );
    }

    #[test]
    fn draw_scene_renders_graph_backed_scene_without_flat_primitives() {
        let mut scene = Scene::new();
        scene.graph = Some(RenderGraph::new(LayerNode {
            node_id: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: cranpose_ui_graphics::Point::default(),
            content_offset: cranpose_ui_graphics::Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
            graphics_layer: cranpose_ui_graphics::GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 2.0,
                            y: 3.0,
                            width: 6.0,
                            height: 5.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        }));

        let width = 20;
        let height = 20;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_scene(&mut frame, width, height, &scene);

        assert!(
            count_non_background_pixels(&frame, width, height) > 0,
            "graph-backed scenes should render even when flat primitive arrays are empty"
        );
    }

    #[test]
    fn text_clip_bounds_prevent_drawing_outside_scroll_window() {
        let mut raster_scene = RasterScene::new();
        raster_scene.push_text(
            2,
            Rect {
                x: 8.0,
                y: 40.0,
                width: 180.0,
                height: 24.0,
            },
            Rc::new(cranpose_ui::text::AnnotatedString::from("Clipped Text")),
            Color::WHITE,
            cranpose_ui::TextStyle::default(),
            14.0,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 20.0,
            }),
        );

        let width = 220;
        let height = 100;
        let mut frame = vec![0u8; (width * height * 4) as usize];
        draw_raster_scene_for_test(&mut frame, width, height, &raster_scene);

        let total_ink = count_non_background_pixels_in_band(&frame, width, 0, height);
        assert_eq!(
            total_ink, 0,
            "text should be fully clipped but rendered {total_ink} ink pixels"
        );
    }

    #[test]
    fn gradient_brush_contract_requires_visible_color_transition() {
        let style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)],
                    cranpose_ui_graphics::Point::new(0.0, 0.0),
                    cranpose_ui_graphics::Point::new(320.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        let (width, _height, frame) = render_single_text_frame(style, Color::WHITE, 12.0);
        let left = average_ink_rgb(&frame, width, 20, 150, 20, 120).expect("left ink");
        let right = average_ink_rgb(&frame, width, 200, 340, 20, 120).expect("right ink");

        assert!(
            left[0] > left[2] * 1.15,
            "left side should be red-dominant for horizontal gradient, got {left:?}"
        );
        assert!(
            right[2] > right[0] * 1.15,
            "right side should be blue-dominant for horizontal gradient, got {right:?}"
        );
    }

    #[test]
    fn draw_style_stroke_contract_changes_raster_output() {
        let fill_style = cranpose_ui::TextStyle::default();
        let stroke_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 6.0 }),
                ..Default::default()
            },
            ..Default::default()
        };

        let (width, height, fill_frame) = render_single_text_frame(fill_style, Color::WHITE, 12.0);
        let (_, _, stroke_frame) = render_single_text_frame(stroke_style, Color::WHITE, 12.0);
        let fill_ink = count_non_background_pixels(&fill_frame, width, height);
        let stroke_ink = count_non_background_pixels(&stroke_frame, width, height);

        assert_ne!(
            fill_frame, stroke_frame,
            "Fill and Stroke text must not rasterize identically"
        );
        assert!(
            fill_ink.abs_diff(stroke_ink) > 250,
            "Fill/Stroke ink coverage should differ; fill={fill_ink}, stroke={stroke_ink}"
        );
    }

    #[test]
    fn shadow_blur_radius_contract_changes_raster_output() {
        let base_shadow = cranpose_ui::text::Shadow {
            color: Color(0.0, 0.0, 0.0, 0.85),
            offset: cranpose_ui_graphics::Point::new(6.0, 4.0),
            blur_radius: 0.0,
        };
        let zero_blur_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            ..Default::default()
        };
        let blurred_style = cranpose_ui::TextStyle {
            span_style: cranpose_ui::SpanStyle {
                shadow: Some(cranpose_ui::text::Shadow {
                    blur_radius: 10.0,
                    ..base_shadow
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_, _, zero_frame) = render_single_text_frame(zero_blur_style, Color::WHITE, 12.0);
        let (_, _, blur_frame) = render_single_text_frame(blurred_style, Color::WHITE, 12.0);

        assert_ne!(
            zero_frame, blur_frame,
            "Changing shadow blur radius must change rendered output"
        );
    }

    #[test]
    fn text_motion_contract_changes_raster_output() {
        let static_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Static),
                ..Default::default()
            },
            ..Default::default()
        };
        let animated_style = cranpose_ui::TextStyle {
            paragraph_style: cranpose_ui::ParagraphStyle {
                text_motion: Some(cranpose_ui::text::TextMotion::Animated),
                ..Default::default()
            },
            ..Default::default()
        };

        let (_, _, static_frame) = render_single_text_frame(static_style, Color::WHITE, 12.35);
        let (_, _, animated_frame) = render_single_text_frame(animated_style, Color::WHITE, 12.35);

        assert_ne!(
            static_frame, animated_frame,
            "TextMotion::Static and TextMotion::Animated should not rasterize identically"
        );
    }

    // ── Stroke / arc rasterization ──────────────────────────────────────────
    //
    // The software backend must draw the real stroked/arc shape. Falling back
    // to a filled rect would look plausible in a screenshot diff but is simply
    // the wrong picture, so these assert the defining properties: a stroke is
    // hollow, an arc is a band, and an annular sector has flat radial edges.

    const CANVAS: u32 = 64;

    fn blank_frame() -> Vec<u8> {
        vec![0u8; (CANVAS * CANVAS * 4) as usize]
    }

    fn is_background(frame: &[u8], x: u32, y: u32) -> bool {
        let idx = ((y * CANVAS + x) * 4) as usize;
        frame[idx..idx + 4] == [18, 18, 24, 255]
    }

    fn is_inked(frame: &[u8], x: u32, y: u32) -> bool {
        !is_background(frame, x, y)
    }

    fn render_shape(shape: crate::scene::DrawShape) -> Vec<u8> {
        let mut scene = RasterScene::new();
        scene.shapes.push(shape);
        let mut frame = blank_frame();
        draw_raster_scene_for_test(&mut frame, CANVAS, CANVAS, &scene);
        frame
    }

    fn shape_template(rect: Rect) -> crate::scene::DrawShape {
        crate::scene::DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush: Brush::solid(Color::WHITE),
            shape: None,
            stroke: None,
            arc: None,
            z_index: 0,
            clip: None,
            blend_mode: BlendMode::SrcOver,
        }
    }

    #[test]
    fn stroked_rect_rasterizes_hollow() {
        // Geometry (16,16)-(48,48) stroked at width 4 => bounds (14,14)-(50,50).
        let mut shape = shape_template(Rect {
            x: 14.0,
            y: 14.0,
            width: 36.0,
            height: 36.0,
        });
        shape.stroke = Some(cranpose_ui_graphics::Stroke::new(4.0));
        let frame = render_shape(shape);

        assert!(is_inked(&frame, 32, 16), "top edge must be stroked");
        assert!(is_inked(&frame, 16, 32), "left edge must be stroked");
        assert!(is_inked(&frame, 48, 32), "right edge must be stroked");
        assert!(is_inked(&frame, 32, 48), "bottom edge must be stroked");
        assert!(
            is_background(&frame, 32, 32),
            "the interior of a stroked rect must stay empty — a silent fallback \
             to a filled rect would fill it"
        );
        assert!(is_background(&frame, 32, 8), "outside must stay empty");
    }

    #[test]
    fn stroked_rect_differs_from_the_filled_rect_of_the_same_bounds() {
        let rect = Rect {
            x: 14.0,
            y: 14.0,
            width: 36.0,
            height: 36.0,
        };
        let filled = render_shape(shape_template(rect));
        let mut stroked_shape = shape_template(rect);
        stroked_shape.stroke = Some(cranpose_ui_graphics::Stroke::new(4.0));
        let stroked = render_shape(stroked_shape);
        assert_ne!(filled, stroked);
    }

    #[test]
    fn arc_band_rasterizes_between_the_two_radii() {
        // Full ring, inner 10, outer 16, centered at (32, 32).
        let mut shape = shape_template(Rect {
            x: 16.0,
            y: 16.0,
            width: 32.0,
            height: 32.0,
        });
        shape.arc = Some(cranpose_ui_graphics::ArcGeometry::new(
            Point::new(32.0, 32.0),
            10.0,
            16.0,
            0.0,
            cranpose_ui_graphics::TAU,
            cranpose_ui_graphics::StrokeCap::Butt,
        ));
        let frame = render_shape(shape);

        assert!(is_background(&frame, 32, 32), "the hole must stay empty");
        // Centerline radius 13 in each cardinal direction.
        assert!(is_inked(&frame, 45, 32), "+X band");
        assert!(is_inked(&frame, 19, 32), "-X band");
        assert!(is_inked(&frame, 32, 45), "+Y band");
        assert!(
            is_inked(&frame, 32, 19),
            "-Y band — a seam here would mean the full-turn wrap is mishandled"
        );
    }

    #[test]
    fn annular_sector_has_flat_radial_edges_and_respects_the_sweep() {
        // 0 -> 90 degrees (i.e. +X sweeping down to +Y in screen space),
        // inner 8, outer 16, centered at (32, 32).
        let mut shape = shape_template(Rect {
            x: 32.0,
            y: 32.0,
            width: 16.0,
            height: 16.0,
        });
        shape.arc = Some(cranpose_ui_graphics::ArcGeometry::new(
            Point::new(32.0, 32.0),
            8.0,
            16.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            cranpose_ui_graphics::StrokeCap::Butt,
        ));
        let frame = render_shape(shape);

        // Inside the sweep, between the radii.
        assert!(is_inked(&frame, 44, 33), "inside the sector near 0 degrees");
        assert!(
            is_inked(&frame, 33, 44),
            "inside the sector near 90 degrees"
        );
        // Outside the sweep at the same radius: the radial edge is flat, so
        // one pixel the other side of the start angle is empty.
        assert!(
            is_background(&frame, 44, 30),
            "past the flat radial start edge must be empty"
        );
        assert!(
            is_background(&frame, 30, 44),
            "past the flat radial end edge must be empty"
        );
        // Inside the hole and outside the outer radius.
        assert!(is_background(&frame, 35, 35), "inner hole");
        assert!(is_background(&frame, 52, 33), "beyond the outer radius");
    }

    #[test]
    fn arc_and_stroke_rasterization_never_writes_nan_or_panics() {
        // Degenerate geometry can reach the rasterizer through a translated or
        // cached scene; it must simply draw nothing.
        for arc in [
            cranpose_ui_graphics::ArcGeometry::new(
                Point::new(32.0, 32.0),
                10.0,
                10.0,
                0.0,
                1.0,
                cranpose_ui_graphics::StrokeCap::Butt,
            ),
            cranpose_ui_graphics::ArcGeometry::new(
                Point::new(32.0, 32.0),
                0.0,
                0.0,
                0.0,
                0.0,
                cranpose_ui_graphics::StrokeCap::Round,
            ),
        ] {
            let mut shape = shape_template(Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            });
            shape.arc = Some(arc);
            let frame = render_shape(shape);
            assert!(
                (0..CANVAS).all(|y| (0..CANVAS).all(|x| is_background(&frame, x, y))),
                "a degenerate arc must draw nothing"
            );
        }

        let mut zero_width = shape_template(Rect {
            x: 8.0,
            y: 8.0,
            width: 32.0,
            height: 32.0,
        });
        zero_width.stroke = Some(cranpose_ui_graphics::Stroke::new(0.0));
        let frame = render_shape(zero_width);
        assert!(
            (0..CANVAS).all(|y| (0..CANVAS).all(|x| is_background(&frame, x, y))),
            "a zero-width stroke must draw nothing"
        );
    }
}
