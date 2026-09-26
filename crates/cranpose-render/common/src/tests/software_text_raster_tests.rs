use cranpose_ui::text::{RangeStyle, SpanStyle};
use cranpose_ui_graphics::Point;

use super::*;

fn count_ink_pixels(image: &ImageBitmap) -> usize {
    image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > 0)
        .count()
}

#[test]
fn software_glyph_raster_cache_reuses_static_masks_across_positions() {
    let font = default_software_text_font().expect("bundled default font");
    let style = TextStyle::default();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 160.0,
        height: 32.0,
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);

    let uncached = rasterize_text_to_image(
        "aaaa",
        rect,
        &style,
        Color(1.0, 1.0, 1.0, 1.0),
        18.0,
        1.0,
        &font,
    )
    .expect("uncached image");
    let cached = rasterize_text_to_image_with_glyph_cache(
        "aaaa",
        rect,
        &style,
        Color(1.0, 1.0, 1.0, 1.0),
        18.0,
        1.0,
        &font,
        &mut cache,
    )
    .expect("cached image");

    assert_eq!(cached.pixels(), uncached.pixels());
    let stats = cache.stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 3);

    let shifted_rect = Rect {
        x: 24.0,
        y: 17.0,
        ..rect
    };
    let _ = rasterize_text_to_image_with_glyph_cache(
        "aaaa",
        shifted_rect,
        &style,
        Color(1.0, 1.0, 1.0, 1.0),
        18.0,
        1.0,
        &font,
        &mut cache,
    )
    .expect("cached shifted image");

    let shifted_stats = cache.stats();
    assert_eq!(shifted_stats.entries, 1);
    assert_eq!(shifted_stats.misses, 1);
    assert_eq!(shifted_stats.hits, 7);
}

#[test]
fn annotated_solid_text_direct_raster_matches_plain_text_pixels() {
    let font = default_software_text_font().expect("bundled default font");
    let font_set = SoftwareTextFontSet::from_font(font.clone());
    let style = TextStyle::default();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 240.0,
        height: 40.0,
    };
    let color = Color(1.0, 1.0, 1.0, 1.0);
    let annotated = AnnotatedString {
        text: "plain link".to_string(),
        span_styles: vec![RangeStyle {
            item: SpanStyle {
                color: Some(color),
                ..Default::default()
            },
            range: 0..10,
        }],
        ..Default::default()
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);

    let plain = rasterize_text_to_image(
        annotated.text.as_str(),
        rect,
        &style,
        color,
        18.0,
        1.0,
        &font,
    )
    .expect("plain text image");
    let direct = rasterize_annotated_text_to_image_with_glyph_cache(
        &annotated, rect, &style, color, 18.0, 1.0, &font_set, &mut cache,
    )
    .expect("annotated text image");

    assert_eq!(direct.pixels(), plain.pixels());
}

#[test]
fn solid_annotated_text_collects_atlas_glyphs_with_stable_keys() {
    let font = default_software_text_font().expect("bundled default font");
    let font_set = SoftwareTextFontSet::from_font(font);
    let style = TextStyle::default();
    let rect = Rect {
        x: 12.0,
        y: 4.0,
        width: 260.0,
        height: 48.0,
    };
    let annotated = AnnotatedString {
        text: "markdown link".to_string(),
        span_styles: vec![RangeStyle {
            item: SpanStyle {
                color: Some(Color(0.4, 0.7, 1.0, 1.0)),
                ..Default::default()
            },
            range: 9..13,
        }],
        ..Default::default()
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);
    let mut glyphs = Vec::new();

    collect_solid_text_atlas_glyphs(
        &annotated,
        rect,
        &style,
        Color::WHITE,
        18.0,
        1.0,
        &font_set,
        &mut cache,
        &mut glyphs,
    )
    .expect("solid styled text is atlas-eligible");

    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|glyph| glyph.mask.width > 0));
    assert!(glyphs.iter().all(|glyph| glyph.mask.height > 0));
    assert!(
        glyphs
            .iter()
            .any(|glyph| glyph.color == Color(0.4, 0.7, 1.0, 1.0))
    );
    assert!(cache.stats().entries > 0);
}

#[test]
fn cached_atlas_placements_reuse_existing_glyph_masks_without_payloads() {
    let font = default_software_text_font().expect("bundled default font");
    let font_set = SoftwareTextFontSet::from_font(font);
    let style = TextStyle::default();
    let rect = Rect {
        x: 12.0,
        y: 4.0,
        width: 260.0,
        height: 48.0,
    };
    let annotated = AnnotatedString {
        text: "markdown link".to_string(),
        span_styles: vec![RangeStyle {
            item: SpanStyle {
                color: Some(Color(0.4, 0.7, 1.0, 1.0)),
                ..Default::default()
            },
            range: 9..13,
        }],
        ..Default::default()
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);
    let mut placements = Vec::new();

    assert!(
        collect_cached_solid_text_atlas_placements(
            &annotated,
            rect,
            &style,
            Color::WHITE,
            18.0,
            1.0,
            &font_set,
            &mut cache,
            &mut placements,
        )
        .is_none(),
        "placement-only collection requires retained glyph masks"
    );
    assert!(placements.is_empty());

    let mut glyphs = Vec::new();
    collect_solid_text_atlas_glyphs(
        &annotated,
        rect,
        &style,
        Color::WHITE,
        18.0,
        1.0,
        &font_set,
        &mut cache,
        &mut glyphs,
    )
    .expect("solid styled text is atlas-eligible");

    collect_cached_solid_text_atlas_placements(
        &annotated,
        rect,
        &style,
        Color::WHITE,
        18.0,
        1.0,
        &font_set,
        &mut cache,
        &mut placements,
    )
    .expect("cached masks provide placement-only atlas glyphs");

    assert_eq!(placements.len(), glyphs.len());
    assert!(
        placements
            .iter()
            .zip(glyphs.iter())
            .all(|(placement, glyph)| {
                placement.key == glyph.key
                    && placement.x == glyph.x
                    && placement.y == glyph.y
                    && placement.width == glyph.mask.width
                    && placement.height == glyph.mask.height
                    && placement.color == glyph.color
            })
    );
    let recovered = cache
        .atlas_glyph_for_placement(&placements[0])
        .expect("placement should recover retained mask payload");
    assert_eq!(recovered.key, glyphs[0].key);
    assert_eq!(recovered.x, glyphs[0].x);
    assert_eq!(recovered.y, glyphs[0].y);
    assert_eq!(recovered.mask.width, glyphs[0].mask.width);
    assert_eq!(recovered.mask.height, glyphs[0].mask.height);
    assert_eq!(recovered.mask.alpha, glyphs[0].mask.alpha);
    assert_eq!(recovered.color, glyphs[0].color);
}

#[test]
fn atlas_glyph_collection_rejects_shadow_and_gradient_without_partial_output() {
    let font = default_software_text_font().expect("bundled default font");
    let font_set = SoftwareTextFontSet::from_font(font);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 240.0,
        height: 40.0,
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);
    let mut glyphs = Vec::new();
    glyphs.push(SoftwareGlyphAtlasGlyph {
        key: SoftwareGlyphAtlasKey {
            font_hash: 1,
            glyph_id: 1,
            scale_x_bits: 1,
            scale_y_bits: 1,
            embolden_px_bits: 0,
            slant_bits: 0,
        },
        mask: SoftwareGlyphAtlasMask {
            alpha: Arc::from([1.0f32]),
            width: 1,
            height: 1,
        },
        x: 0,
        y: 0,
        color: Color::WHITE,
    });
    let initial_len = glyphs.len();

    let shadow_style = TextStyle::from_span_style(SpanStyle {
        shadow: Some(Shadow {
            color: Color(0.0, 0.0, 0.0, 0.5),
            offset: Point::new(1.0, 1.0),
            blur_radius: 0.0,
        }),
        ..Default::default()
    });
    assert!(
        collect_solid_text_atlas_glyphs(
            &AnnotatedString::new("shadow".to_string()),
            rect,
            &shadow_style,
            Color::WHITE,
            18.0,
            1.0,
            &font_set,
            &mut cache,
            &mut glyphs,
        )
        .is_none()
    );
    assert_eq!(glyphs.len(), initial_len);

    let gradient_style = TextStyle::from_span_style(SpanStyle {
        brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
        ..Default::default()
    });
    assert!(
        collect_solid_text_atlas_glyphs(
            &AnnotatedString::new("gradient".to_string()),
            rect,
            &gradient_style,
            Color::WHITE,
            18.0,
            1.0,
            &font_set,
            &mut cache,
            &mut glyphs,
        )
        .is_none()
    );
    assert_eq!(glyphs.len(), initial_len);
}

fn average_ink_rgb(
    image: &ImageBitmap,
    x_start: u32,
    x_end: u32,
    y_start: u32,
    y_end: u32,
) -> Option<[f32; 3]> {
    let width = image.width();
    let height = image.height();
    let mut sums = [0.0f32; 3];
    let mut count = 0usize;
    let pixels = image.pixels();

    let x_end = x_end.min(width);
    let y_end = y_end.min(height);
    for y in y_start.min(height)..y_end {
        for x in x_start.min(width)..x_end {
            let idx = ((y * width + x) * 4) as usize;
            let alpha = pixels[idx + 3];
            if alpha == 0 {
                continue;
            }
            sums[0] += pixels[idx] as f32 / 255.0;
            sums[1] += pixels[idx + 1] as f32 / 255.0;
            sums[2] += pixels[idx + 2] as f32 / 255.0;
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }
    Some([
        sums[0] / count as f32,
        sums[1] / count as f32,
        sums[2] / count as f32,
    ])
}

fn ink_x_range(image: &ImageBitmap) -> Option<(u32, u32)> {
    let width = image.width();
    let height = image.height();
    let pixels = image.pixels();
    let mut min_x = u32::MAX;
    let mut max_x = 0u32;
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if pixels[idx + 3] > 0 {
                min_x = min_x.min(x);
                max_x = max_x.max(x + 1);
                found = true;
            }
        }
    }
    found.then_some((min_x, max_x))
}

fn ink_y_range(image: &ImageBitmap) -> Option<(u32, u32)> {
    let width = image.width();
    let height = image.height();
    let pixels = image.pixels();
    let mut min_y = u32::MAX;
    let mut max_y = 0u32;
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if pixels[idx + 3] > 0 {
                min_y = min_y.min(y);
                max_y = max_y.max(y + 1);
                found = true;
            }
        }
    }
    found.then_some((min_y, max_y))
}

fn ink_centroid_x(image: &ImageBitmap, y_start: u32, y_end: u32) -> Option<f32> {
    let width = image.width();
    let height = image.height();
    let pixels = image.pixels();
    let mut weighted_x = 0.0f32;
    let mut total_alpha = 0.0f32;

    for y in y_start.min(height)..y_end.min(height) {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let alpha = pixels[idx + 3] as f32 / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            weighted_x += x as f32 * alpha;
            total_alpha += alpha;
        }
    }

    (total_alpha > 0.0).then_some(weighted_x / total_alpha)
}

fn vertical_slant_delta(image: &ImageBitmap) -> f32 {
    let (top, bottom) = ink_y_range(image).expect("image should contain ink");
    let mid = top + (bottom - top).max(1) / 2;
    let top_x = ink_centroid_x(image, top, mid).expect("top ink centroid");
    let bottom_x = ink_centroid_x(image, mid, bottom).expect("bottom ink centroid");
    top_x - bottom_x
}

fn top_ink_row(image: &ImageBitmap) -> Option<u32> {
    let width = image.width();
    let height = image.height();
    let pixels = image.pixels();
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if pixels[idx + 3] > 0 {
                return Some(y);
            }
        }
    }
    None
}

fn reference_dilation_offsets(radius: i32) -> Vec<(i32, i32)> {
    let mut offsets = Vec::new();
    let squared_radius = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= squared_radius {
                offsets.push((dx, dy));
            }
        }
    }
    if offsets.is_empty() {
        offsets.push((0, 0));
    }
    offsets
}

fn reference_dilation_stroke_mask(fill: &GlyphMask, stroke_width: f32) -> GlyphMask {
    let radius = (stroke_width * 0.5).ceil() as i32;
    let offsets = reference_dilation_offsets(radius);
    let out_width = fill.width as i32 + radius * 2;
    let out_height = fill.height as i32 + radius * 2;
    let fill_width_i32 = fill.width as i32;
    let fill_height_i32 = fill.height as i32;
    let mut alpha = vec![0.0f32; (out_width * out_height) as usize];

    for out_y in 0..out_height {
        let oy = out_y - radius;
        for out_x in 0..out_width {
            let ox = out_x - radius;
            let base_alpha = if ox >= 0 && oy >= 0 && ox < fill_width_i32 && oy < fill_height_i32 {
                fill.alpha[oy as usize * fill.width + ox as usize]
            } else {
                0.0
            };

            let mut dilated_alpha = 0.0f32;
            for (dx, dy) in &offsets {
                let sx = ox + dx;
                let sy = oy + dy;
                if sx < 0 || sy < 0 || sx >= fill_width_i32 || sy >= fill_height_i32 {
                    continue;
                }
                let sample = fill.alpha[sy as usize * fill.width + sx as usize];
                if sample > dilated_alpha {
                    dilated_alpha = sample;
                    if dilated_alpha >= 0.999 {
                        break;
                    }
                }
            }
            alpha[out_y as usize * out_width as usize + out_x as usize] =
                (dilated_alpha - base_alpha).max(0.0);
        }
    }

    GlyphMask {
        alpha: Arc::from(alpha),
        width: out_width as usize,
        height: out_height as usize,
        origin_x: fill.origin_x - radius,
        origin_y: fill.origin_y - radius,
    }
}

fn rasterize_reference_dilation_stroke(
    text: &str,
    rect: Rect,
    font_size: f32,
    stroke_width: f32,
    font: &impl Font,
) -> ImageBitmap {
    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;
    let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];

    let metrics = vertical_metrics(font, font_size);
    let baseline = line_box_for(&TextStyle::default(), metrics, font_size * 1.4, 1.0).baseline;
    for glyph in layout_line_glyphs(font, text, font_size, point(0.0, baseline)) {
        let Some((outlined, bounds)) = outline_glyph_with_bounds(font, &glyph) else {
            continue;
        };
        let Some(fill) = build_fill_mask(&outlined, bounds) else {
            continue;
        };
        let reference = reference_dilation_stroke_mask(&fill, stroke_width);
        draw_mask_glyph(
            &mut canvas,
            width,
            height,
            &reference,
            &Brush::solid(Color::WHITE),
            1.0,
            rect,
        );
    }

    let mut rgba = vec![0u8; canvas.len() * 4];
    for (index, pixel) in canvas.iter().enumerate() {
        let base = index * 4;
        rgba[base] = (pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 1] = (pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 2] = (pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    ImageBitmap::from_rgba8(width, height, rgba).expect("reference dilation image")
}

fn test_font() -> ab_glyph::FontRef<'static> {
    ab_glyph::FontRef::try_from_slice(include_bytes!("../../assets/NotoSansMerged.ttf"))
        .expect("font")
}

fn test_software_font() -> SoftwareTextFont {
    SoftwareTextFont::from_bytes(include_bytes!("../../assets/NotoSansMerged.ttf").to_vec())
        .expect("font")
}

#[test]
fn software_text_font_rejects_invalid_bytes() {
    assert!(SoftwareTextFont::from_bytes(vec![0, 1, 2, 3]).is_err());
}

#[test]
fn default_software_text_font_has_no_process_global_cache() {
    let source = include_str!("../software_text_raster.rs");
    let once_lock = ["Once", "Lock"].concat();
    let cached_default = ["static ", "FONT"].concat();
    let default_font_fn = ["fn ", "default_font()"].concat();

    assert!(
        !source.contains(&cached_default)
            && !source.contains(&default_font_fn)
            && !source.contains(&once_lock),
        "default software text font construction must be explicit renderer/app-owned state, not a process-global cache"
    );
}

#[test]
fn software_text_measurer_empty_font_set_uses_deterministic_fallback_without_panicking() {
    let measurer = SoftwareTextMeasurer::from_font_set(SoftwareTextFontSet::empty(), 4);
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = AnnotatedString::from("ab\nc");

    let metrics = measurer.measure(&text, &style);
    assert_eq!(metrics.line_count, 2);
    assert!(metrics.width > 0.0);
    assert!(metrics.height >= metrics.line_height * 2.0);

    let cursor_x = measurer.get_cursor_x_for_offset(&text, &style, 2);
    assert!(cursor_x > 0.0);
    let second_line_offset =
        measurer.get_offset_for_position(&text, &style, 0.0, metrics.line_height);
    assert!(
        second_line_offset >= "ab\n".len(),
        "fallback hit testing should resolve into the second line: {second_line_offset}"
    );

    let layout = measurer.layout(&text, &style);
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.glyph_layouts().len(), 3);
}

#[test]
fn software_text_metrics_layout_and_cursor_share_font_backend() {
    let font = test_software_font();
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(18.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = "Text\nBackend";

    let metrics = measure_text_with_font(text, &style, 18.0, &font);
    let layout = layout_text_with_font(text, &style, &font);

    assert!(metrics.width > 0.0);
    assert_eq!(metrics.line_count, 2);
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.height, metrics.height);
    assert!(layout.glyph_layouts().len() >= "TextBackend".len());

    let offset = text_offset_for_position_with_font(text, &style, 0.0, metrics.line_height, &font);
    assert!(
        offset >= "Text\n".len(),
        "second-line hit testing should return a byte offset on the second line: {offset}"
    );
    let cursor_x = cursor_x_for_offset_with_font(text, &style, "Text".len(), &font);
    assert!(cursor_x > 0.0);
}

#[test]
fn software_text_metrics_keep_requested_font_size_for_default_font() {
    let font = default_software_text_font().expect("bundled default test font");
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    };

    let metrics = measure_text_with_font("Counter App", &style, 14.0, &font);
    assert!(
        (metrics.width - 83.16).abs() < 0.05 && (metrics.height - 19.0).abs() < 0.05,
        "14sp demo text must use font em metrics, not ab_glyph height units, and a line \
         with no line height is the font's own 15px ascent and 4px descent: {metrics:?}"
    );
}

#[test]
fn software_text_synthesizes_missing_bold_weight() {
    let font = test_software_font();
    let normal_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let bold_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };
    let no_synthesis_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            font_weight: Some(FontWeight::BOLD),
            font_synthesis: Some(FontSynthesis::None),
            ..Default::default()
        },
        ..Default::default()
    };

    let normal = measure_text_with_font("Save Raster WebP", &normal_style, 20.0, &font);
    let synthesized = measure_text_with_font("Save Raster WebP", &bold_style, 20.0, &font);
    let disabled = measure_text_with_font("Save Raster WebP", &no_synthesis_style, 20.0, &font);

    assert!(
        synthesized.width > normal.width * 1.04,
        "bold fallback should synthesize heavier advances: normal={normal:?} synthesized={synthesized:?}"
    );
    assert!(
        (disabled.width - normal.width).abs() < 0.01,
        "explicit FontSynthesis::None should preserve regular metrics: normal={normal:?} disabled={disabled:?}"
    );
}

#[test]
fn rasterized_synthetic_bold_adds_ink_without_changing_line_box() {
    let font = test_software_font();
    let normal_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let bold_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(20.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };
    let normal_metrics = measure_text_with_font("Composer", &normal_style, 20.0, &font);
    let bold_metrics = measure_text_with_font("Composer", &bold_style, 20.0, &font);

    let normal = rasterize_text_to_image(
        "Composer",
        Rect {
            x: 0.0,
            y: 0.0,
            width: normal_metrics.width.ceil(),
            height: normal_metrics.height.ceil(),
        },
        &normal_style,
        Color::WHITE,
        20.0,
        1.0,
        &font,
    )
    .expect("normal text image");
    let bold = rasterize_text_to_image(
        "Composer",
        Rect {
            x: 0.0,
            y: 0.0,
            width: bold_metrics.width.ceil(),
            height: bold_metrics.height.ceil(),
        },
        &bold_style,
        Color::WHITE,
        20.0,
        1.0,
        &font,
    )
    .expect("bold text image");

    assert_eq!(bold.height(), normal.height());
    assert!(
        count_ink_pixels(&bold) > count_ink_pixels(&normal),
        "synthetic bold should increase rasterized ink coverage"
    );
}

#[test]
fn software_text_synthesizes_missing_italic_style() {
    let font = test_software_font();
    let normal_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(36.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let italic_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(36.0),
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        },
        ..Default::default()
    };
    let no_synthesis_style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(36.0),
            font_style: Some(FontStyle::Italic),
            font_synthesis: Some(FontSynthesis::None),
            ..Default::default()
        },
        ..Default::default()
    };

    let normal_metrics = measure_text_with_font("Italic", &normal_style, 36.0, &font);
    let italic_metrics = measure_text_with_font("Italic", &italic_style, 36.0, &font);
    let disabled_metrics = measure_text_with_font("Italic", &no_synthesis_style, 36.0, &font);

    assert!(
        italic_metrics.width > normal_metrics.width + 6.0,
        "italic fallback should reserve slanted visual overhang: normal={normal_metrics:?} italic={italic_metrics:?}"
    );
    assert!(
        (disabled_metrics.width - normal_metrics.width).abs() < 0.01,
        "explicit FontSynthesis::None should preserve regular metrics: normal={normal_metrics:?} disabled={disabled_metrics:?}"
    );

    let normal = rasterize_text_to_image(
        "Italic",
        Rect {
            x: 0.0,
            y: 0.0,
            width: normal_metrics.width.ceil(),
            height: normal_metrics.height.ceil(),
        },
        &normal_style,
        Color::WHITE,
        36.0,
        1.0,
        &font,
    )
    .expect("normal text image");
    let italic = rasterize_text_to_image(
        "Italic",
        Rect {
            x: 0.0,
            y: 0.0,
            width: italic_metrics.width.ceil(),
            height: italic_metrics.height.ceil(),
        },
        &italic_style,
        Color::WHITE,
        36.0,
        1.0,
        &font,
    )
    .expect("italic text image");
    let disabled = rasterize_text_to_image(
        "Italic",
        Rect {
            x: 0.0,
            y: 0.0,
            width: disabled_metrics.width.ceil(),
            height: disabled_metrics.height.ceil(),
        },
        &no_synthesis_style,
        Color::WHITE,
        36.0,
        1.0,
        &font,
    )
    .expect("disabled italic text image");

    assert_eq!(
        normal.pixels(),
        disabled.pixels(),
        "FontSynthesis::None must not synthesize oblique glyphs"
    );
    assert!(
        vertical_slant_delta(&italic) > vertical_slant_delta(&normal) + 2.0,
        "synthetic italic should visibly lean top ink to the right"
    );
}

#[test]
fn rasterized_default_text_fills_expected_visual_height() {
    let font = default_software_text_font().expect("bundled default test font");
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let metrics = measure_text_with_font("Counter App", &style, 14.0, &font);
    let image = rasterize_text_to_image(
        "Counter App",
        Rect {
            x: 0.0,
            y: 0.0,
            width: metrics.width.ceil(),
            height: metrics.height.ceil(),
        },
        &style,
        Color::WHITE,
        14.0,
        1.0,
        &font,
    )
    .expect("text image");
    let (top, bottom) = ink_y_range(&image).expect("text should contain ink");
    let ink_height = bottom - top;

    assert!(
        ink_height >= 13,
        "14sp default text ink should keep visual height parity with the WGPU baseline: top={top} bottom={bottom} image={}x{}",
        image.width(),
        image.height()
    );
}

#[test]
fn software_text_font_selection_preserves_first_complete_default_face() {
    let regular =
        SoftwareTextFont::from_bytes(include_bytes!("../../assets/NotoSansMerged.ttf").to_vec())
            .expect("regular test font should load");
    let font = software_text_font_from_fonts_or_default(&[
        include_bytes!("../../assets/NotoSansMerged.ttf"),
        include_bytes!("../../assets/NotoSansBold.ttf"),
        include_bytes!("../../assets/TwemojiMozilla.ttf"),
    ])
    .expect("font selection should resolve a test font");
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(18.0),
            ..Default::default()
        },
        ..Default::default()
    };

    let regular_metrics = measure_text_with_font("UNDER", &style, 18.0, &regular);
    let metrics = measure_text_with_font("UNDER", &style, 18.0, &font);
    assert!(
        (metrics.width - regular_metrics.width).abs() < 0.01,
        "font selection should keep the declared regular face for default text: selected={metrics:?}, regular={regular_metrics:?}"
    );
}

#[test]
fn software_text_font_resolution_reuses_cached_font_score() {
    let font = test_software_font();
    assert!(
        font.score.is_complete_default_face(),
        "test font should cache complete Latin coverage at load time: supported={} width={}",
        font.score.supported_latin_chars,
        font.score.latin_sample_width
    );

    let fonts = SoftwareTextFontSet::from_font(font.clone());
    let resolved = fonts
        .resolve(&TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("font set should resolve a test font");

    assert_eq!(
        resolved.score.supported_latin_chars,
        font.score.supported_latin_chars
    );
    assert_eq!(
        resolved.score.latin_sample_width,
        font.score.latin_sample_width
    );
}

#[test]
fn software_text_font_set_resolves_requested_weight() {
    let fonts = software_text_font_set_from_fonts_or_default(&[
        include_bytes!("../../assets/NotoSansMerged.ttf"),
        include_bytes!("../../assets/NotoSansBold.ttf"),
        include_bytes!("../../assets/TwemojiMozilla.ttf"),
    ]);
    let regular = fonts
        .resolve(&TextStyle::default())
        .expect("font set should resolve regular test font");
    let bold_style = TextStyle {
        span_style: SpanStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };
    let bold = fonts
        .resolve(&bold_style)
        .expect("font set should resolve bold test font");

    assert_eq!(regular.weight(), FontWeight::NORMAL);
    assert_eq!(bold.weight(), FontWeight::BOLD);

    let regular_metrics =
        measure_text_with_font("Counter App", &TextStyle::default(), 18.0, regular);
    let bold_metrics = measure_text_with_font("Counter App", &bold_style, 18.0, bold);
    assert!(
        bold_metrics.width > regular_metrics.width,
        "bold face resolution should affect real text metrics: regular={regular_metrics:?} bold={bold_metrics:?}"
    );
}

fn registered_face(family: &FontFamily, weight: FontWeight) -> SoftwareTextFont {
    SoftwareTextFont::from_registered_bytes(
        family,
        weight,
        FontStyle::Normal,
        include_bytes!("../../assets/NotoSansMerged.ttf").to_vec(),
    )
    .expect("registered test face")
}

fn style_naming(family: &FontFamily) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_family: Some(family.clone()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn unregistered_face() -> SoftwareTextFont {
    SoftwareTextFont::from_bytes(include_bytes!("../../assets/NotoSansBold.ttf").to_vec())
        .expect("unregistered test face")
}

#[test]
fn a_named_family_resolves_the_face_registered_under_it() {
    let family = FontFamily::named("Game UI");
    let fonts = SoftwareTextFontSet::from_faces(vec![
        unregistered_face(),
        registered_face(&family, FontWeight::NORMAL),
    ]);

    let resolved = fonts
        .resolve(&style_naming(&family))
        .expect("registered face");
    assert_eq!(
        resolved.registered_family(),
        Some(FontFamilyKey::of(&family))
    );
}

#[test]
fn a_file_backed_family_never_resolves_a_face_filed_under_another_one() {
    let mine = FontFamily::loaded_typeface_path("/fonts/Mine.ttf");
    let theirs = FontFamily::loaded_typeface_path("/fonts/Theirs.ttf");
    let fallback =
        SoftwareTextFont::from_bytes(include_bytes!("../../assets/NotoSansMerged.ttf").to_vec())
            .expect("fallback test face");
    let theirs_face = SoftwareTextFont::from_registered_bytes(
        &theirs,
        FontWeight::BOLD,
        FontStyle::Normal,
        include_bytes!("../../assets/NotoSansBold.ttf").to_vec(),
    )
    .expect("registered test face");
    let fonts = SoftwareTextFontSet::from_faces(vec![fallback.clone(), theirs_face]);

    assert_eq!(
        fonts
            .resolve(&style_naming(&mine))
            .expect("fallback face")
            .content_hash(),
        fallback.content_hash(),
        "an unregistered family must fall back rather than borrow someone else's face"
    );
    assert_eq!(
        fonts
            .resolve(&style_naming(&theirs))
            .expect("registered face")
            .registered_family(),
        Some(FontFamilyKey::of(&theirs)),
        "the family that was registered still resolves to its own face"
    );
}

#[test]
fn a_generic_family_only_constrains_the_set_once_a_face_is_registered_for_it() {
    let bold_sans_serif = TextStyle {
        span_style: SpanStyle {
            font_family: Some(FontFamily::SansSerif),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };

    let unclaimed = software_text_font_set_from_fonts_or_default(&[
        include_bytes!("../../assets/NotoSansMerged.ttf"),
        include_bytes!("../../assets/NotoSansBold.ttf"),
    ]);
    assert_eq!(
        unclaimed
            .resolve(&bold_sans_serif)
            .expect("bold face")
            .weight(),
        FontWeight::BOLD
    );

    let claimed = SoftwareTextFontSet::from_faces(vec![
        SoftwareTextFont::from_bytes(include_bytes!("../../assets/NotoSansBold.ttf").to_vec())
            .expect("bold test face"),
        registered_face(&FontFamily::SansSerif, FontWeight::NORMAL),
    ]);
    let resolved = claimed.resolve(&bold_sans_serif).expect("system face");
    assert_eq!(
        resolved.registered_family(),
        Some(FontFamilyKey::of(&FontFamily::SansSerif))
    );
}

#[test]
fn an_app_supplied_family_measures_once_and_is_served_from_the_metrics_cache() {
    let family = FontFamily::named("Game UI");
    let measurer = SoftwareTextMeasurer::from_font_set(
        SoftwareTextFontSet::from_faces(vec![registered_face(&family, FontWeight::NORMAL)]),
        64,
    );
    let style = style_naming(&family);
    let text = AnnotatedString::from("SCORE 1234");

    let first = measurer.measure(&text, &style);
    let stats_after_first = measurer.lock_cache().glyph_metrics.stats();
    for _ in 0..60 {
        assert_eq!(measurer.measure(&text, &style), first);
    }

    assert_eq!(
        measurer.lock_cache().glyph_metrics.stats(),
        stats_after_first,
        "repeat frames of an unchanged string must not re-shape against the app face"
    );
}

#[test]
fn a_font_size_animation_measures_each_glyph_once_rather_than_once_per_size() {
    let font = default_software_text_font().expect("bundled default test font");
    let measurer = SoftwareTextMeasurer::new(font, 64);
    let text = AnnotatedString::from("Scaling list row");

    let sized = |size: f32| TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(size),
            ..Default::default()
        },
        ..Default::default()
    };

    let first = measurer.measure(&text, &sized(14.0));
    let after_first = measurer.lock_cache().glyph_metrics.stats();

    for step in 0..120 {
        let size = 14.0 + step as f32 * 0.137;
        let measured = measurer.measure(&text, &sized(size));
        assert!(
            measured.width > 0.0,
            "a scaled measurement must still produce a width"
        );
    }

    let after_scaling = measurer.lock_cache().glyph_metrics.stats();
    assert_eq!(
        (after_scaling.glyph_misses, after_scaling.kern_misses),
        (after_first.glyph_misses, after_first.kern_misses),
        "measuring the same glyphs at a new size must not re-read the font: {after_scaling:?}"
    );
    assert!(
        after_scaling.glyph_hits > after_first.glyph_hits,
        "the scaled measurements must have come from the cache"
    );

    let single = measurer.measure(&AnnotatedString::from("W"), &sized(20.0));
    let double = measurer.measure(&AnnotatedString::from("W"), &sized(40.0));
    let ratio = double.width / single.width.max(f32::EPSILON);
    assert!(
        (ratio - 2.0).abs() < 0.01,
        "advances must scale with the font size: {single:?} -> {double:?} (ratio {ratio})"
    );
    let _ = first;
}

#[test]
fn software_text_metrics_use_largest_annotated_span_font_size() {
    let font = default_software_text_font().expect("bundled default test font");
    let text = AnnotatedString::builder()
        .push_style(SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(30.0),
            ..Default::default()
        })
        .append("BIG ")
        .pop()
        .push_style(SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(10.0),
            ..Default::default()
        })
        .append("small")
        .pop()
        .to_annotated_string();

    let metrics = measure_annotated_text_with_font(&text, &TextStyle::default(), 14.0, &font);

    assert!(
        metrics.height >= 30.0,
        "rich text metrics must include the largest span height: {metrics:?}"
    );
    assert!(
        metrics.width > 48.0,
        "rich text metrics should measure run widths at their span sizes: {metrics:?}"
    );
}

#[test]
fn software_text_line_height_matches_full_measurement_without_width_layout() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let text = AnnotatedString::builder()
        .append("normal ")
        .push_style(SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(32.0),
            ..Default::default()
        })
        .append("large")
        .pop()
        .append("\nsecond line")
        .to_annotated_string();
    let style = TextStyle::default();

    let measured = measurer.measure(&text, &style);
    let line_height = measurer.line_height(&text, &style);

    assert_eq!(line_height, measured.line_height);
    assert!(
        line_height > measurer.line_height(&AnnotatedString::from("normal"), &style),
        "span font size should affect fast line-height lookup"
    );
}

#[test]
fn solid_text_atlas_line_advance_matches_measured_line_height() {
    let font = default_software_text_font().expect("bundled default test font");
    let fonts = SoftwareTextFontSet::from_font(font);
    let style = TextStyle::default();
    let text = AnnotatedString::from("A\nA\nA\nA");
    let font_size = style.resolve_font_size(14.0);
    let metrics = measure_annotated_text_with_font_set(&text, &style, font_size, &fonts);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: metrics.height,
    };
    let mut glyph_cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(16);
    let mut run = Vec::new();

    collect_solid_text_atlas_run(
        &text,
        rect,
        &style,
        Color(1.0, 1.0, 1.0, 1.0),
        font_size,
        1.0,
        &fonts,
        &mut glyph_cache,
        &mut run,
    )
    .expect("atlas-compatible text");

    let mut glyph_y: Vec<i32> = run.iter().map(|glyph| glyph.placement().y).collect();
    glyph_y.sort_unstable();
    glyph_y.dedup();
    assert_eq!(glyph_y.len(), 4);
    for window in glyph_y.windows(2) {
        let advance = (window[1] - window[0]) as f32;
        assert!(
            (advance - metrics.line_height).abs() <= 1.0,
            "glyph advance {advance} should match measured line height {}",
            metrics.line_height
        );
    }
}

#[test]
fn software_text_metrics_cache_keys_include_span_styles() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let plain = AnnotatedString::from("BIG small");
    let rich = AnnotatedString::builder()
        .push_style(SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(30.0),
            ..Default::default()
        })
        .append("BIG ")
        .pop()
        .append("small")
        .to_annotated_string();

    let plain_metrics = measurer.measure(&plain, &TextStyle::default());
    let rich_metrics = measurer.measure(&rich, &TextStyle::default());

    assert!(
        rich_metrics.height > plain_metrics.height,
        "cached plain text metrics must not be reused for styled text: plain={plain_metrics:?} rich={rich_metrics:?}"
    );
}

#[test]
fn software_text_metrics_cache_recovers_after_poison() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let text = AnnotatedString::from("Recovered text metrics");

    let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = measurer
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        panic!("poison software text metrics cache for recovery test");
    }));

    assert!(poison_result.is_err());

    let metrics = measurer.measure(&text, &TextStyle::default());
    assert!(metrics.width > 0.0);
    assert!(metrics.height > 0.0);

    let subset = measurer.measure_subsequence(&text, 0.."Recovered".len(), &TextStyle::default());
    assert!(subset.width > 0.0);
    assert!(subset.width < metrics.width);
}

#[test]
fn software_text_prefix_widths_match_subsequence_measurement() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(18.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = AnnotatedString::from("Hello Prefix Widths");
    let widths = measurer
        .measure_line_prefix_widths(&text, 0..text.text.len(), &style)
        .expect("uniform line should expose prefix widths");

    let start = "Hello ".len();
    let end = "Hello Prefix".len();
    let expected = measurer
        .measure_subsequence(&text, start..end, &style)
        .width;
    let actual = widths
        .width_for_char_range(6, 12)
        .expect("valid char range");

    assert!(
        (actual - expected).abs() < 0.01,
        "prefix width should match exact subsequence width: actual={actual}, expected={expected}"
    );
}

#[test]
fn software_text_line_width_and_prefix_width_share_cached_plan() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let style = TextStyle::default();
    let text = AnnotatedString::from("shared prefix plan ".repeat(32).as_str());
    let line_range = 0..text.text.len();

    let width = measurer
        .measure_line_width(&text, line_range.clone(), &style)
        .expect("software text should expose a line width");
    let stats_after_width = {
        let cache = measurer.lock_cache();
        assert_eq!(cache.line_prefix_widths.len(), 1);
        cache.glyph_metrics.stats()
    };

    let widths = measurer
        .measure_line_prefix_widths(&text, line_range, &style)
        .expect("line width probe should cache the prefix plan");
    let stats_after_prefix = measurer.lock_cache().glyph_metrics.stats();

    assert_eq!(stats_after_prefix, stats_after_width);
    assert!(
        (width - widths.width_for_char_range(0, widths.char_count()).unwrap()).abs() < 0.01,
        "cached line-width probe and prefix plan must agree"
    );
}

#[test]
fn software_text_glyph_metrics_cache_reuses_common_glyphs_across_unique_lines() {
    let measurer = SoftwareTextMeasurer::new(
        default_software_text_font().expect("bundled default test font"),
        8,
    );
    let style = TextStyle::default();
    let first = AnnotatedString::from("algorithm data structure ".repeat(24).as_str());
    let second = AnnotatedString::from("algorithmic structures repeat data ".repeat(24).as_str());

    measurer
        .measure_line_prefix_widths(&first, 0..first.text.len(), &style)
        .expect("first unique line should measure");
    let stats_after_first = measurer.lock_cache().glyph_metrics.stats();

    measurer
        .measure_line_prefix_widths(&second, 0..second.text.len(), &style)
        .expect("second unique line should measure");
    let stats_after_second = measurer.lock_cache().glyph_metrics.stats();

    assert!(
        stats_after_second.glyph_hits > stats_after_first.glyph_hits,
        "unique markdown rows should reuse retained glyph metrics: first={stats_after_first:?} second={stats_after_second:?}"
    );
    assert!(
        stats_after_second.kern_hits > stats_after_first.kern_hits,
        "unique markdown rows should reuse retained kerning metrics: first={stats_after_first:?} second={stats_after_second:?}"
    );
}

#[test]
fn rasterized_gradient_text_shows_color_transition() {
    let font = test_font();
    let plain_style = TextStyle::default();
    let probe = rasterize_text_to_image_with_font(
        "MMMMMMMM",
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 96.0,
        },
        &plain_style,
        Color::WHITE,
        48.0,
        1.0,
        &font,
    )
    .expect("probe image");
    let (ink_x_min, ink_x_max) = ink_x_range(&probe).expect("probe must contain ink");
    let gradient_end = ink_x_max as f32;

    let style = TextStyle {
        span_style: SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color::RED, Color::BLUE],
                Point::new(0.0, 0.0),
                Point::new(gradient_end, 0.0),
            )),
            ..Default::default()
        },
        ..Default::default()
    };

    let image = rasterize_text_to_image_with_font(
        "MMMMMMMM",
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 96.0,
        },
        &style,
        Color::WHITE,
        48.0,
        1.0,
        &font,
    )
    .expect("rasterized image");

    let ink_span = ink_x_max.saturating_sub(ink_x_min).max(1);
    let left_end = ink_x_min + ink_span * 3 / 10;
    let right_start = ink_x_max.saturating_sub(ink_span * 3 / 10);
    let left = average_ink_rgb(&image, ink_x_min, left_end, 8, 90).expect("left ink");
    let right = average_ink_rgb(&image, right_start, ink_x_max, 8, 90).expect("right ink");
    assert!(
        left[0] > left[2] * 1.1,
        "left region should be red dominant, got {left:?}"
    );
    assert!(
        right[2] > right[0] * 1.1,
        "right region should be blue dominant, got {right:?}"
    );
}

#[test]
fn rasterized_stroke_and_fill_ink_coverage_differs() {
    let font = test_font();
    let fill_style = TextStyle::default();
    let stroke_style = TextStyle {
        span_style: SpanStyle {
            draw_style: Some(TextDrawStyle::Stroke { width: 6.0 }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 96.0,
    };

    let fill = rasterize_text_to_image_with_font(
        "MMMMMMMM",
        rect,
        &fill_style,
        Color::WHITE,
        48.0,
        1.0,
        &font,
    )
    .expect("fill image");
    let stroke = rasterize_text_to_image_with_font(
        "MMMMMMMM",
        rect,
        &stroke_style,
        Color::WHITE,
        48.0,
        1.0,
        &font,
    )
    .expect("stroke image");

    let fill_ink = count_ink_pixels(&fill);
    let stroke_ink = count_ink_pixels(&stroke);
    assert_ne!(fill.pixels(), stroke.pixels());
    assert!(
        fill_ink.abs_diff(stroke_ink) > 300,
        "fill/stroke ink coverage should differ; fill={fill_ink}, stroke={stroke_ink}"
    );
}

#[test]
fn stroke_path_uses_miter_join_for_acute_apexes() {
    let font = test_font();
    let fill_style = TextStyle::default();
    let stroke_width = 12.0;
    let stroke_style = TextStyle {
        span_style: SpanStyle {
            draw_style: Some(TextDrawStyle::Stroke {
                width: stroke_width,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 180.0,
        height: 140.0,
    };

    let fill =
        rasterize_text_to_image_with_font("A", rect, &fill_style, Color::WHITE, 110.0, 1.0, &font)
            .expect("fill image");
    let stroke = rasterize_text_to_image_with_font(
        "A",
        rect,
        &stroke_style,
        Color::WHITE,
        110.0,
        1.0,
        &font,
    )
    .expect("stroke image");

    let fill_top = top_ink_row(&fill).expect("fill top row");
    let stroke_top = top_ink_row(&stroke).expect("stroke top row");
    let reference_dilation =
        rasterize_reference_dilation_stroke("A", rect, 110.0, stroke_width, &font);
    let reference_top = top_ink_row(&reference_dilation).expect("reference top row");
    let extra_extension = fill_top.saturating_sub(stroke_top) as f32;
    let half_stroke = stroke_width * 0.5;
    assert!(
        extra_extension >= half_stroke - 0.25,
        "stroke apex should extend by roughly at least half stroke width; fill_top={fill_top}, stroke_top={stroke_top}, half_stroke={half_stroke:.2}"
    );
    assert!(
        stroke.pixels() != reference_dilation.pixels(),
        "path stroke should diverge from mask-dilation reference output"
    );
    assert!(
        stroke_top <= reference_top,
        "miter stroke should keep acute apex at least as extended as mask-dilation reference; stroke_top={stroke_top}, reference_top={reference_top}"
    );
}

#[test]
fn shadow_blur_radius_changes_spread_for_shared_raster_path() {
    let font = test_font();
    let base_shadow = Shadow {
        color: Color(0.0, 0.0, 0.0, 0.9),
        offset: Point::new(5.5, 4.25),
        blur_radius: 0.0,
    };
    let hard_shadow_style = TextStyle {
        span_style: SpanStyle {
            shadow: Some(base_shadow),
            ..Default::default()
        },
        ..Default::default()
    };
    let blurred_shadow_style = TextStyle {
        span_style: SpanStyle {
            shadow: Some(Shadow {
                blur_radius: 9.0,
                ..base_shadow
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 120.0,
    };

    let hard_shadow = rasterize_text_to_image_with_font(
        "Shared shadow",
        rect,
        &hard_shadow_style,
        Color::TRANSPARENT,
        48.0,
        1.0,
        &font,
    )
    .expect("hard shadow image");
    let blurred_shadow = rasterize_text_to_image_with_font(
        "Shared shadow",
        rect,
        &blurred_shadow_style,
        Color::TRANSPARENT,
        48.0,
        1.0,
        &font,
    )
    .expect("blurred shadow image");

    let hard_ink = count_ink_pixels(&hard_shadow);
    let blurred_ink = count_ink_pixels(&blurred_shadow);
    assert_ne!(
        hard_shadow.pixels(),
        blurred_shadow.pixels(),
        "blur radius should change rasterized shadow output"
    );
    assert!(
        blurred_ink > hard_ink,
        "blurred shadow should spread to more pixels; hard={hard_ink}, blurred={blurred_ink}"
    );
}

#[test]
fn text_motion_changes_fractional_shadow_sampling() {
    let font = test_font();
    let base_shadow = Shadow {
        color: Color(0.0, 0.0, 0.0, 0.9),
        offset: Point::new(3.35, 2.65),
        blur_radius: 6.0,
    };
    let static_style = TextStyle {
        span_style: SpanStyle {
            shadow: Some(base_shadow),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::text::ParagraphStyle {
            text_motion: Some(TextMotion::Static),
            ..Default::default()
        },
    };
    let animated_style = TextStyle {
        span_style: SpanStyle {
            shadow: Some(base_shadow),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::text::ParagraphStyle {
            text_motion: Some(TextMotion::Animated),
            ..Default::default()
        },
    };
    let rect = Rect {
        x: 11.35,
        y: 7.65,
        width: 280.0,
        height: 120.0,
    };

    let static_image = rasterize_text_to_image_with_font(
        "Motion shadow",
        rect,
        &static_style,
        Color::TRANSPARENT,
        42.0,
        1.0,
        &font,
    )
    .expect("static image");
    let animated_image = rasterize_text_to_image_with_font(
        "Motion shadow",
        rect,
        &animated_style,
        Color::TRANSPARENT,
        42.0,
        1.0,
        &font,
    )
    .expect("animated image");

    assert_ne!(
        static_image.pixels(),
        animated_image.pixels(),
        "TextMotion::Static should quantize shadow placement while Animated keeps fractional sampling"
    );
}

#[test]
fn static_text_motion_aligns_glyph_positions_to_pixel_grid() {
    let font = test_font();
    let base_glyph = layout_line_glyphs(&font, "A", 17.0, point(0.0, 13.37))
        .into_iter()
        .next()
        .expect("glyph");
    let static_aligned = align_glyph_for_text_motion(base_glyph, true);
    let static_position = static_aligned.position;
    assert!(
        (static_position.x - static_position.x.round()).abs() < f32::EPSILON,
        "static text should snap glyph x to pixel grid"
    );
    assert!(
        (static_position.y - static_position.y.round()).abs() < f32::EPSILON,
        "static text should snap glyph y to pixel grid"
    );

    let animated_source = layout_line_glyphs(&font, "A", 17.0, point(0.0, 13.37))
        .into_iter()
        .next()
        .expect("glyph");
    let animated_aligned = align_glyph_for_text_motion(animated_source, false);
    let animated_position = animated_aligned.position;
    assert!(
        (animated_position.y - 13.37).abs() < 1e-3,
        "animated text should preserve fractional glyph position"
    );
}
