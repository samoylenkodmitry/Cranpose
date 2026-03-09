use cranpose_core::NodeId;
use cranpose_ui::text::{AnnotatedString, Shadow, SpanStyle, TextDecoration};
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, GraphicsLayer, Point, Rect};

use crate::graph::{
    CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
    PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};
use crate::raster_cache::LayerRasterCacheHashes;

const BACKGROUND_COLOR: Color = Color(18.0 / 255.0, 18.0 / 255.0, 24.0 / 255.0, 1.0);
const FOREGROUND_COLOR: Color = Color::WHITE;
const PIXEL_DIFFERENCE_TOLERANCE: u32 = 24;

// These budgets sit just above the currently observed normalized diffs from both backends.
// They still reject material subtree motion or distortion while tolerating the bounded edge drift
// that comes from comparing root-space rasterization at different fractional translations.
const TRANSLATED_SUBTREE_BUDGET: NormalizedDifferenceBudget = NormalizedDifferenceBudget {
    max_differing_pixels: 245,
    max_pixel_difference: 360,
};
const TRANSLATED_TEXT_DECORATIONS_BUDGET: NormalizedDifferenceBudget = NormalizedDifferenceBudget {
    max_differing_pixels: 240,
    max_pixel_difference: 360,
};

#[derive(Clone)]
pub struct RenderFixture {
    pub width: u32,
    pub height: u32,
    pub graph: RenderGraph,
    pub normalized_rect: Option<Rect>,
}

#[derive(Clone, Debug)]
pub struct RenderedFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub normalized_rect: Option<Rect>,
}

#[derive(Clone, Copy)]
struct NormalizedDifferenceBudget {
    max_differing_pixels: u32,
    max_pixel_difference: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedRenderCase {
    RoundedRect,
    PrimitiveClip,
    TranslatedSubtree,
    TranslatedTextDecorations,
    MultilineText,
    ClippedText,
}

pub const ALL_SHARED_RENDER_CASES: [SharedRenderCase; 6] = [
    SharedRenderCase::RoundedRect,
    SharedRenderCase::PrimitiveClip,
    SharedRenderCase::TranslatedSubtree,
    SharedRenderCase::TranslatedTextDecorations,
    SharedRenderCase::MultilineText,
    SharedRenderCase::ClippedText,
];

impl SharedRenderCase {
    pub fn name(self) -> &'static str {
        match self {
            SharedRenderCase::RoundedRect => "rounded_rect",
            SharedRenderCase::PrimitiveClip => "primitive_clip",
            SharedRenderCase::TranslatedSubtree => "translated_subtree",
            SharedRenderCase::TranslatedTextDecorations => "translated_text_decorations",
            SharedRenderCase::MultilineText => "multiline_text",
            SharedRenderCase::ClippedText => "clipped_text",
        }
    }

    pub fn fixtures(self) -> Vec<RenderFixture> {
        match self {
            SharedRenderCase::RoundedRect => vec![rounded_rect_fixture()],
            SharedRenderCase::PrimitiveClip => vec![primitive_clip_fixture()],
            SharedRenderCase::TranslatedSubtree => vec![
                translated_subtree_fixture(12.3, 14.7),
                translated_subtree_fixture(32.6, 26.2),
            ],
            SharedRenderCase::TranslatedTextDecorations => vec![
                translated_text_decorations_fixture(14.3, 18.6),
                translated_text_decorations_fixture(36.4, 30.1),
            ],
            SharedRenderCase::MultilineText => vec![multiline_text_fixture()],
            SharedRenderCase::ClippedText => vec![clipped_text_fixture()],
        }
    }

    pub fn assert_frames(self, frames: &[RenderedFrame]) {
        match self {
            SharedRenderCase::RoundedRect => {
                let [frame] = frames else {
                    panic!("rounded_rect expects exactly one rendered frame");
                };
                assert_rounded_rect_frame(&frame.pixels, frame.width, frame.height);
            }
            SharedRenderCase::PrimitiveClip => {
                let [frame] = frames else {
                    panic!("primitive_clip expects exactly one rendered frame");
                };
                assert_primitive_clip_frame(&frame.pixels, frame.width, frame.height);
            }
            SharedRenderCase::TranslatedSubtree => {
                assert_translated_subtree_frames(frames);
            }
            SharedRenderCase::TranslatedTextDecorations => {
                assert_translated_text_decorations_frames(frames);
            }
            SharedRenderCase::MultilineText => {
                let [frame] = frames else {
                    panic!("multiline_text expects exactly one rendered frame");
                };
                assert_multiline_text_frame(&frame.pixels, frame.width, frame.height);
            }
            SharedRenderCase::ClippedText => {
                let [frame] = frames else {
                    panic!("clipped_text expects exactly one rendered frame");
                };
                assert_clipped_text_frame(&frame.pixels, frame.width, frame.height);
            }
        }
    }
}

fn rounded_rect_fixture() -> RenderFixture {
    build_fixture(
        72,
        72,
        vec![draw_node(
            DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 12.0,
                    y: 12.0,
                    width: 48.0,
                    height: 48.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
                radii: CornerRadii::uniform(18.0),
            },
            None,
        )],
    )
}

fn primitive_clip_fixture() -> RenderFixture {
    build_fixture(
        52,
        44,
        vec![draw_node(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 8.0,
                    y: 10.0,
                    width: 28.0,
                    height: 18.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
            },
            Some(Rect {
                x: 14.0,
                y: 15.0,
                width: 10.0,
                height: 6.0,
            }),
        )],
    )
}

fn translated_subtree_fixture(translation_x: f32, translation_y: f32) -> RenderFixture {
    let subtree_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 48.0,
        height: 36.0,
    };

    build_translated_fixture(
        96,
        84,
        subtree_bounds,
        Point::new(translation_x, translation_y),
        vec![
            draw_node(
                DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: 4.0,
                        y: 4.0,
                        width: 40.0,
                        height: 28.0,
                    },
                    brush: Brush::solid(FOREGROUND_COLOR),
                    radii: CornerRadii::uniform(10.0),
                },
                None,
            ),
            draw_node(
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 18.0,
                        width: 18.0,
                        height: 10.0,
                    },
                    brush: Brush::solid(Color(0.2, 0.8, 1.0, 1.0)),
                },
                Some(Rect {
                    x: 12.0,
                    y: 20.0,
                    width: 10.0,
                    height: 4.0,
                }),
            ),
        ],
    )
}

fn multiline_text_fixture() -> RenderFixture {
    build_fixture(
        220,
        100,
        vec![text_node(
            1,
            Rect {
                x: 8.0,
                y: 8.0,
                width: 180.0,
                height: 80.0,
            },
            "Dynamic\nModifiers",
            None,
        )],
    )
}

fn clipped_text_fixture() -> RenderFixture {
    build_fixture(
        220,
        100,
        vec![text_node(
            2,
            Rect {
                x: 8.0,
                y: 40.0,
                width: 180.0,
                height: 24.0,
            },
            "Clipped Text",
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 20.0,
            }),
        )],
    )
}

fn translated_text_decorations_fixture(translation_x: f32, translation_y: f32) -> RenderFixture {
    let subtree_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 112.0,
        height: 40.0,
    };
    let text_style = TextStyle::from_span_style(SpanStyle {
        color: Some(FOREGROUND_COLOR),
        shadow: Some(Shadow {
            color: Color(0.0, 0.0, 0.0, 0.85),
            offset: Point::new(3.0, 2.0),
            blur_radius: 4.0,
        }),
        text_decoration: Some(TextDecoration::UNDERLINE),
        ..Default::default()
    });

    build_translated_fixture(
        180,
        96,
        subtree_bounds,
        Point::new(translation_x, translation_y),
        vec![text_node_with_style(
            3,
            Rect {
                x: 6.0,
                y: 6.0,
                width: 96.0,
                height: 24.0,
            },
            "Shifted",
            None,
            text_style,
        )],
    )
}

fn build_fixture(width: u32, height: u32, children: Vec<RenderNode>) -> RenderFixture {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };

    RenderFixture {
        width,
        height,
        graph: RenderGraph::new(graph_layer(
            bounds,
            ProjectiveTransform::identity(),
            with_background(bounds, children),
        )),
        normalized_rect: None,
    }
}

fn build_translated_fixture(
    width: u32,
    height: u32,
    subtree_bounds: Rect,
    translation: Point,
    subtree_children: Vec<RenderNode>,
) -> RenderFixture {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let subtree = graph_layer(
        subtree_bounds,
        ProjectiveTransform::translation(translation.x, translation.y),
        subtree_children,
    );

    RenderFixture {
        width,
        height,
        graph: RenderGraph::new(graph_layer(
            bounds,
            ProjectiveTransform::identity(),
            with_background(bounds, vec![RenderNode::Layer(Box::new(subtree))]),
        )),
        normalized_rect: Some(Rect {
            x: translation.x,
            y: translation.y,
            width: subtree_bounds.width,
            height: subtree_bounds.height,
        }),
    }
}

fn graph_layer(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id: None,
        local_bounds,
        transform_to_parent,
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        children,
    }
}

fn with_background(bounds: Rect, mut children: Vec<RenderNode>) -> Vec<RenderNode> {
    children.insert(
        0,
        draw_node(
            DrawPrimitive::Rect {
                rect: bounds,
                brush: Brush::solid(BACKGROUND_COLOR),
            },
            None,
        ),
    );
    children
}

fn draw_node(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode { primitive, clip }),
    })
}

fn text_node(node_id: NodeId, rect: Rect, text: &str, clip: Option<Rect>) -> RenderNode {
    text_node_with_style(
        node_id,
        rect,
        text,
        clip,
        TextStyle::from_span_style(SpanStyle {
            color: Some(FOREGROUND_COLOR),
            ..Default::default()
        }),
    )
}

fn text_node_with_style(
    node_id: NodeId,
    rect: Rect,
    text: &str,
    clip: Option<Rect>,
    text_style: TextStyle,
) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id,
            rect,
            text: AnnotatedString::from(text),
            text_style,
            font_size: 14.0,
            layout_options: TextLayoutOptions::default(),
            clip,
        })),
    })
}

fn assert_rounded_rect_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (72, 72));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        14,
        14,
        true,
        "rounded rect corner should stay background-colored",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        30,
        16,
        false,
        "rounded rect top edge should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        36,
        36,
        false,
        "rounded rect center should contain fill",
    );
}

fn assert_primitive_clip_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (52, 44));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        18,
        18,
        false,
        "pixel inside primitive clip should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        10,
        12,
        true,
        "pixel inside source rect but outside clip should stay background-colored",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        30,
        20,
        true,
        "pixel on the far side of the source rect but outside clip should stay background-colored",
    );
}

fn assert_multiline_text_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (220, 100));
    let background = sample_pixel(pixels, width, 2, 2);
    let (ink_top, ink_bottom) = ink_y_range(pixels, width, height, background)
        .expect("expected rendered text ink in multiline contract frame");
    let ink_height = ink_bottom - ink_top;
    assert!(
        ink_height >= 18,
        "expected two text lines of ink, observed span {ink_height}px (y={ink_top}..{ink_bottom})"
    );
    let mid_y = ink_top + ink_height / 2;
    let first_line_ink =
        count_non_background_pixels_in_band(pixels, width, ink_top, mid_y, background);
    let second_line_ink =
        count_non_background_pixels_in_band(pixels, width, mid_y, ink_bottom, background);
    assert!(
        first_line_ink > 20,
        "expected first line ink in multiline contract frame, got {first_line_ink}"
    );
    assert!(
        second_line_ink > 20,
        "expected second line ink in multiline contract frame, got {second_line_ink}"
    );
}

fn assert_translated_subtree_frames(frames: &[RenderedFrame]) {
    let [base, moved] = frames else {
        panic!("translated_subtree expects exactly two rendered frames");
    };
    assert_eq!((base.width, base.height), (96, 84));
    assert_eq!((moved.width, moved.height), (96, 84));
    assert_ne!(
        base.pixels, moved.pixels,
        "translated subtree contract should move within the full frame"
    );
    assert_normalized_region_matches(
        base,
        moved,
        TRANSLATED_SUBTREE_BUDGET,
        "translated subtree output should remain invariant under rigid parent translation",
    );
}

fn assert_translated_text_decorations_frames(frames: &[RenderedFrame]) {
    let [base, moved] = frames else {
        panic!("translated_text_decorations expects exactly two rendered frames");
    };
    assert_eq!((base.width, base.height), (180, 96));
    assert_eq!((moved.width, moved.height), (180, 96));
    assert_ne!(
        base.pixels, moved.pixels,
        "translated text contract should move within the full frame"
    );
    assert_normalized_region_matches(
        base,
        moved,
        TRANSLATED_TEXT_DECORATIONS_BUDGET,
        "normalized text/shadow/decoration output should remain invariant under rigid parent translation",
    );

    let background = sample_pixel(&base.pixels, base.width, 2, 2);
    let base_crop = normalize_frame_region(base);
    let (crop_width, crop_height) = normalized_output_dimensions(base);
    let ink_pixels = count_non_background_pixels(&base_crop, crop_width, crop_height, background);
    assert!(
        ink_pixels > 120,
        "translated text contract should contain visible ink, observed {ink_pixels} differing pixels"
    );
}

fn assert_clipped_text_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (220, 100));
    let background = sample_pixel(pixels, width, 2, 2);
    let total_ink = count_non_background_pixels(pixels, width, height, background);
    assert_eq!(
        total_ink, 0,
        "fully clipped text should not draw ink, but observed {total_ink} differing pixels"
    );
}

fn sample_pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

fn normalize_frame_region(frame: &RenderedFrame) -> Vec<u8> {
    let rect = normalized_rect(frame);
    let (width, height) = normalized_output_dimensions(frame);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let sample_x = rect.x + ((x as f32 + 0.5) * rect.width / width as f32);
            let sample_y = rect.y + ((y as f32 + 0.5) * rect.height / height as f32);
            out.extend_from_slice(&sample_pixel_bilinear(frame, sample_x, sample_y));
        }
    }
    out
}

fn sample_pixel_bilinear(frame: &RenderedFrame, x: f32, y: f32) -> [u8; 4] {
    let max_x = frame.width.saturating_sub(1) as f32;
    let max_y = frame.height.saturating_sub(1) as f32;
    let source_x = (x - 0.5).clamp(0.0, max_x);
    let source_y = (y - 0.5).clamp(0.0, max_y);
    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width.saturating_sub(1));
    let y1 = (y0 + 1).min(frame.height.saturating_sub(1));
    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;
    let top_left = sample_pixel(&frame.pixels, frame.width, x0, y0);
    let top_right = sample_pixel(&frame.pixels, frame.width, x1, y0);
    let bottom_left = sample_pixel(&frame.pixels, frame.width, x0, y1);
    let bottom_right = sample_pixel(&frame.pixels, frame.width, x1, y1);

    let lerp_channel = |index: usize| {
        let top = top_left[index] as f32 * (1.0 - tx) + top_right[index] as f32 * tx;
        let bottom = bottom_left[index] as f32 * (1.0 - tx) + bottom_right[index] as f32 * tx;
        (top * (1.0 - ty) + bottom * ty).round() as u8
    };

    [
        lerp_channel(0),
        lerp_channel(1),
        lerp_channel(2),
        lerp_channel(3),
    ]
}

fn assert_normalized_region_matches(
    base: &RenderedFrame,
    moved: &RenderedFrame,
    budget: NormalizedDifferenceBudget,
    message: &str,
) {
    assert_eq!(
        normalized_output_dimensions(base),
        normalized_output_dimensions(moved),
        "normalized comparison requires matching output sizes",
    );
    let (width, height) = normalized_output_dimensions(base);
    let base_normalized = normalize_frame_region(base);
    let moved_normalized = normalize_frame_region(moved);
    let stats = normalized_difference_stats(
        &base_normalized,
        &moved_normalized,
        width,
        height,
        PIXEL_DIFFERENCE_TOLERANCE,
    );
    // Fractional parent motion changes root-space sampling phase. The shared contract tolerates the
    // bounded edge drift that both backends currently produce after normalization, while still
    // rejecting regressions that move or distort the local picture materially.
    if stats.differing_pixels > budget.max_differing_pixels
        || stats.max_difference > budget.max_pixel_difference
    {
        let diff = stats
            .first_difference
            .as_ref()
            .expect("failing normalized comparison should report first difference");
        panic!(
            "{message}; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
            stats.differing_pixels,
            stats.max_difference,
            diff.x,
            diff.y,
            diff.base,
            diff.moved,
            diff.difference
        );
    }
}

fn normalized_rect(frame: &RenderedFrame) -> Rect {
    frame
        .normalized_rect
        .expect("normalized render frame missing normalized_rect")
}

fn normalized_output_dimensions(frame: &RenderedFrame) -> (u32, u32) {
    let rect = normalized_rect(frame);
    (
        normalized_dimension(rect.width, "width"),
        normalized_dimension(rect.height, "height"),
    )
}

fn normalized_dimension(value: f32, axis: &str) -> u32 {
    let rounded = value.round();
    assert!(
        (value - rounded).abs() <= 0.01,
        "normalized {axis} must stay pixel-sized for stable comparison, got {value}",
    );
    assert!(
        rounded > 0.0,
        "normalized {axis} must be positive, got {value}"
    );
    rounded as u32
}

#[derive(Debug)]
struct PixelDifference {
    x: u32,
    y: u32,
    base: [u8; 4],
    moved: [u8; 4],
    difference: u32,
}

struct DifferenceStats {
    differing_pixels: u32,
    max_difference: u32,
    first_difference: Option<PixelDifference>,
}

fn normalized_difference_stats(
    base: &[u8],
    moved: &[u8],
    width: u32,
    height: u32,
    tolerance: u32,
) -> DifferenceStats {
    let mut differing_pixels = 0;
    let mut max_difference = 0;
    let mut first_difference = None;
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let base_pixel = [
                base[index],
                base[index + 1],
                base[index + 2],
                base[index + 3],
            ];
            let moved_pixel = [
                moved[index],
                moved[index + 1],
                moved[index + 2],
                moved[index + 3],
            ];
            let difference = pixel_difference(base_pixel, moved_pixel);
            if difference > tolerance {
                differing_pixels += 1;
                max_difference = max_difference.max(difference);
                if first_difference.is_none() {
                    first_difference = Some(PixelDifference {
                        x,
                        y,
                        base: base_pixel,
                        moved: moved_pixel,
                        difference,
                    });
                }
            }
        }
    }
    DifferenceStats {
        differing_pixels,
        max_difference,
        first_difference,
    }
}

fn pixel_difference(lhs: [u8; 4], rhs: [u8; 4]) -> u32 {
    lhs.into_iter()
        .zip(rhs)
        .map(|(left, right)| left.abs_diff(right) as u32)
        .sum()
}

fn is_background_like(pixel: [u8; 4], background: [u8; 4]) -> bool {
    pixel_difference(pixel, background) <= PIXEL_DIFFERENCE_TOLERANCE
}

fn assert_pixel_matches_background(
    pixels: &[u8],
    width: u32,
    background: [u8; 4],
    x: u32,
    y: u32,
    expect_background: bool,
    message: &str,
) {
    let pixel = sample_pixel(pixels, width, x, y);
    let background_like = is_background_like(pixel, background);
    assert_eq!(
        background_like, expect_background,
        "{message}; pixel at ({x},{y}) was {pixel:?} against background {background:?}"
    );
}

fn count_non_background_pixels(pixels: &[u8], width: u32, height: u32, background: [u8; 4]) -> u32 {
    count_non_background_pixels_in_band(pixels, width, 0, height, background)
}

fn count_non_background_pixels_in_band(
    pixels: &[u8],
    width: u32,
    y_start: u32,
    y_end: u32,
    background: [u8; 4],
) -> u32 {
    let mut count = 0;
    for y in y_start..y_end {
        for x in 0..width {
            if !is_background_like(sample_pixel(pixels, width, x, y), background) {
                count += 1;
            }
        }
    }
    count
}

fn ink_y_range(pixels: &[u8], width: u32, height: u32, background: [u8; 4]) -> Option<(u32, u32)> {
    let mut top = None;
    let mut bottom = 0u32;
    for y in 0..height {
        for x in 0..width {
            if !is_background_like(sample_pixel(pixels, width, x, y), background) {
                top.get_or_insert(y);
                bottom = y + 1;
                break;
            }
        }
    }
    top.map(|top_y| (top_y, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn shared_render_cases_have_unique_names() {
        let names: HashSet<_> = ALL_SHARED_RENDER_CASES
            .into_iter()
            .map(SharedRenderCase::name)
            .collect();
        assert_eq!(names.len(), ALL_SHARED_RENDER_CASES.len());
    }

    #[test]
    fn shared_render_cases_build_non_empty_graphs() {
        for case in ALL_SHARED_RENDER_CASES {
            for fixture in case.fixtures() {
                assert!(fixture.width > 0);
                assert!(fixture.height > 0);
                assert!(
                    !fixture.graph.root.children.is_empty(),
                    "shared render case {} should emit at least one render node",
                    case.name()
                );
            }
        }
    }
}
