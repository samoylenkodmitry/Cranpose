use std::sync::{Arc, OnceLock, RwLock};

use cranpose_render_common::{software_text_raster::rasterize_text_to_image_with_font, Brush};
use cranpose_ui::text::{FontFamily, FontStyle, TextDrawStyle, TextStyle};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect};
use rusttype::Font;
use ttf_parser::{name_id, Face};

struct RasterFontFace {
    font: Arc<Font<'static>>,
    weight: u16,
    italic: bool,
    family_name: Option<String>,
}

static RASTER_FONTS: OnceLock<RwLock<Vec<RasterFontFace>>> = OnceLock::new();

pub(crate) fn configure_raster_fonts(fonts: &[&[u8]]) {
    let parsed: Vec<RasterFontFace> = fonts
        .iter()
        .filter_map(|font_data| {
            let metadata = Face::parse(font_data, 0).ok();
            let font = Font::try_from_vec(font_data.to_vec())?;
            Some(RasterFontFace {
                font: Arc::new(font),
                weight: metadata
                    .as_ref()
                    .map(|face| face.weight().to_number())
                    .unwrap_or(400),
                italic: metadata.as_ref().is_some_and(Face::is_italic),
                family_name: metadata.as_ref().and_then(extract_family_name),
            })
        })
        .collect();
    assert!(
        !parsed.is_empty(),
        "rasterized styled text requires at least one valid configured font"
    );

    let storage = RASTER_FONTS.get_or_init(|| RwLock::new(Vec::new()));
    let mut configured = storage
        .write()
        .expect("rasterized styled text font storage poisoned");
    *configured = parsed;
}

fn extract_family_name(face: &Face<'_>) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|name| {
            name.name_id == name_id::TYPOGRAPHIC_FAMILY || name.name_id == name_id::FAMILY
        })
        .find_map(|name| {
            name.to_string().and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_ascii_lowercase())
                }
            })
        })
}

fn choose_raster_face_index(style: &TextStyle, fonts: &[RasterFontFace]) -> Option<usize> {
    if fonts.is_empty() {
        return None;
    }
    if fonts.len() == 1 {
        return Some(0);
    }

    let requested_weight = style
        .span_style
        .font_weight
        .map(|weight| weight.0)
        .unwrap_or(400);
    let requested_italic = style.span_style.font_style == Some(FontStyle::Italic);
    let requested_family = style.span_style.font_family.as_ref();
    let requested_named_family = match requested_family {
        Some(FontFamily::Named(name)) => Some(name.to_ascii_lowercase()),
        _ => None,
    };

    let mut best_index = 0usize;
    let mut best_score = i32::MAX;
    for (index, face) in fonts.iter().enumerate() {
        let family_penalty = match requested_family {
            Some(FontFamily::Named(_)) => match requested_named_family.as_ref() {
                Some(requested) => match face.family_name.as_ref() {
                    Some(actual)
                        if actual == requested
                            || actual.contains(requested)
                            || requested.contains(actual) =>
                    {
                        0
                    }
                    _ => 30_000,
                },
                None => 30_000,
            },
            Some(FontFamily::Serif) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("serif") && !name.contains("sans"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Monospace) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("mono"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Cursive) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("cursive") || name.contains("script"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Fantasy) => {
                if face
                    .family_name
                    .as_ref()
                    .is_some_and(|name| name.contains("fantasy"))
                {
                    0
                } else {
                    5_000
                }
            }
            Some(FontFamily::Default | FontFamily::SansSerif) | None => 0,
        };
        let style_penalty = if requested_italic == face.italic {
            0
        } else {
            2_000
        };
        let weight_penalty = (face.weight as i32 - requested_weight as i32).abs();
        let score = family_penalty + style_penalty + weight_penalty;
        if score < best_score {
            best_score = score;
            best_index = index;
        }
    }

    Some(best_index)
}

fn raster_font(style: &TextStyle) -> Option<Arc<Font<'static>>> {
    let storage = RASTER_FONTS.get()?;
    let fonts = storage.read().ok()?;
    let best_index = choose_raster_face_index(style, &fonts)?;
    Some(Arc::clone(&fonts[best_index].font))
}

pub(crate) fn requires_rasterized_glyph_path(style: &TextStyle) -> bool {
    let uses_non_solid_brush = matches!(
        style.span_style.brush,
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    );
    let uses_stroke = matches!(
        style.span_style.draw_style,
        Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0
    );
    uses_non_solid_brush || uses_stroke
}

pub(crate) fn rasterize_text_to_image(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
) -> Option<ImageBitmap> {
    let font = raster_font(style)?;
    rasterize_text_to_image_with_font(text, rect, style, fallback_color, font_size, scale, &font)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui::text::{FontWeight, SpanStyle};
    use cranpose_ui_graphics::Point;

    fn init_test_fonts() {
        configure_raster_fonts(&[
            include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Light.ttf") as &[u8],
            include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Regular.ttf") as &[u8],
        ]);
    }

    fn selected_weight(style: &TextStyle) -> u16 {
        let storage = RASTER_FONTS.get().expect("configured test fonts");
        let fonts = storage
            .read()
            .expect("rasterized styled text font storage poisoned");
        let index = choose_raster_face_index(style, &fonts).expect("selected font index");
        fonts[index].weight
    }

    fn count_ink_pixels(image: &ImageBitmap) -> usize {
        image
            .pixels()
            .chunks_exact(4)
            .filter(|px| px[3] > 0)
            .count()
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

    #[test]
    fn requires_rasterized_glyph_path_for_gradient_or_stroke() {
        init_test_fonts();
        let fill_style = TextStyle::default();
        assert!(!requires_rasterized_glyph_path(&fill_style));

        let gradient_style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color::RED, Color::BLUE],
                    Point::new(0.0, 0.0),
                    Point::new(100.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(requires_rasterized_glyph_path(&gradient_style));

        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke { width: 4.0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(requires_rasterized_glyph_path(&stroke_style));
    }

    #[test]
    fn raster_font_selection_prefers_closest_weight() {
        init_test_fonts();
        let default_style = TextStyle::default();
        let light_style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::LIGHT),
                ..Default::default()
            },
            ..Default::default()
        };
        let medium_style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
            ..Default::default()
        };

        let default_weight = selected_weight(&default_style);
        let light_weight = selected_weight(&light_style);
        let medium_weight = selected_weight(&medium_style);

        assert!(
            default_weight.abs_diff(400) <= default_weight.abs_diff(300),
            "default style should prefer regular-ish face, got {default_weight}"
        );
        assert!(
            light_weight.abs_diff(300) <= light_weight.abs_diff(400),
            "light style should prefer light-ish face, got {light_weight}"
        );
        assert!(
            medium_weight.abs_diff(400) <= medium_weight.abs_diff(300),
            "medium style should stay closest to regular-ish face with configured fonts, got {medium_weight}"
        );
    }

    #[test]
    fn rasterized_gradient_text_shows_color_transition() {
        init_test_fonts();
        let style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color::RED, Color::BLUE],
                    Point::new(0.0, 0.0),
                    Point::new(320.0, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        let image = rasterize_text_to_image(
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
        )
        .expect("rasterized image");

        let left = average_ink_rgb(&image, 12, 130, 8, 90).expect("left ink");
        let right = average_ink_rgb(&image, 190, 308, 8, 90).expect("right ink");
        assert!(
            left[0] > left[2] * 1.15,
            "left region should be red dominant, got {left:?}"
        );
        assert!(
            right[2] > right[0] * 1.15,
            "right region should be blue dominant, got {right:?}"
        );
    }

    #[test]
    fn rasterized_stroke_and_fill_ink_coverage_differs() {
        init_test_fonts();
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

        let fill = rasterize_text_to_image("MMMMMMMM", rect, &fill_style, Color::WHITE, 48.0, 1.0)
            .expect("fill image");
        let stroke =
            rasterize_text_to_image("MMMMMMMM", rect, &stroke_style, Color::WHITE, 48.0, 1.0)
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
    fn configure_raster_fonts_replaces_previously_configured_faces() {
        configure_raster_fonts(&[include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
        ) as &[u8]]);
        let style = TextStyle::default();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 96.0,
        };
        let light = rasterize_text_to_image("MMMMMMMM", rect, &style, Color::WHITE, 48.0, 1.0)
            .expect("light raster image");

        configure_raster_fonts(&[include_bytes!(
            "../../../../apps/desktop-demo/assets/Roboto-Regular.ttf"
        ) as &[u8]]);
        let regular = rasterize_text_to_image("MMMMMMMM", rect, &style, Color::WHITE, 48.0, 1.0)
            .expect("regular raster image");

        assert_ne!(
            light.pixels(),
            regular.pixels(),
            "reconfiguring raster fonts should change rendered output when face data changes"
        );
    }

    #[test]
    fn rasterized_radial_stroke_bidi_text_shows_visible_color_variation() {
        init_test_fonts();
        let style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::radial_gradient(
                    vec![Color(0.35, 0.95, 1.0, 1.0), Color(1.0, 0.7, 0.45, 1.0)],
                    Point::new(180.0, 48.0),
                    210.0,
                )),
                alpha: Some(0.9),
                draw_style: Some(TextDrawStyle::Stroke { width: 2.2 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let image = rasterize_text_to_image(
            "Gradient שלום stroke",
            Rect {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 96.0,
            },
            &style,
            Color::WHITE,
            32.0,
            1.0,
        )
        .expect("rasterized image");

        let mut min_green = 1.0f32;
        let mut max_green = 0.0f32;
        let mut min_blue = 1.0f32;
        let mut max_blue = 0.0f32;
        let mut ink = 0usize;
        let pixels = image.pixels();
        for px in pixels.chunks_exact(4) {
            if px[3] == 0 {
                continue;
            }
            let green = px[1] as f32 / 255.0;
            let blue = px[2] as f32 / 255.0;
            min_green = min_green.min(green);
            max_green = max_green.max(green);
            min_blue = min_blue.min(blue);
            max_blue = max_blue.max(blue);
            ink += 1;
        }

        assert!(
            ink > 600,
            "expected visible ink for bidi stroke sample, got {ink}"
        );
        let green_span = max_green - min_green;
        let blue_span = max_blue - min_blue;
        assert!(
            green_span.max(blue_span) > 0.12,
            "radial brush stroke should vary channels, green_span={green_span:.3}, blue_span={blue_span:.3}"
        );
    }
}
