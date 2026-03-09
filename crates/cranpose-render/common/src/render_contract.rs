use cranpose_core::NodeId;
use cranpose_ui::text::{AnnotatedString, SpanStyle};
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

#[derive(Clone)]
pub struct RenderFixture {
    pub width: u32,
    pub height: u32,
    pub graph: RenderGraph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedRenderCase {
    RoundedRect,
    PrimitiveClip,
    MultilineText,
    ClippedText,
}

pub const ALL_SHARED_RENDER_CASES: [SharedRenderCase; 4] = [
    SharedRenderCase::RoundedRect,
    SharedRenderCase::PrimitiveClip,
    SharedRenderCase::MultilineText,
    SharedRenderCase::ClippedText,
];

impl SharedRenderCase {
    pub fn name(self) -> &'static str {
        match self {
            SharedRenderCase::RoundedRect => "rounded_rect",
            SharedRenderCase::PrimitiveClip => "primitive_clip",
            SharedRenderCase::MultilineText => "multiline_text",
            SharedRenderCase::ClippedText => "clipped_text",
        }
    }

    pub fn fixture(self) -> RenderFixture {
        match self {
            SharedRenderCase::RoundedRect => rounded_rect_fixture(),
            SharedRenderCase::PrimitiveClip => primitive_clip_fixture(),
            SharedRenderCase::MultilineText => multiline_text_fixture(),
            SharedRenderCase::ClippedText => clipped_text_fixture(),
        }
    }

    pub fn assert_frame(self, pixels: &[u8], width: u32, height: u32) {
        match self {
            SharedRenderCase::RoundedRect => assert_rounded_rect_frame(pixels, width, height),
            SharedRenderCase::PrimitiveClip => assert_primitive_clip_frame(pixels, width, height),
            SharedRenderCase::MultilineText => assert_multiline_text_frame(pixels, width, height),
            SharedRenderCase::ClippedText => assert_clipped_text_frame(pixels, width, height),
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

fn build_fixture(width: u32, height: u32, mut children: Vec<RenderNode>) -> RenderFixture {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
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

    RenderFixture {
        width,
        height,
        graph: RenderGraph::new(LayerNode {
            node_id: None,
            local_bounds: bounds,
            placement: Point::default(),
            content_offset: Point::default(),
            transform_to_parent: ProjectiveTransform::identity(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            children,
        }),
    }
}

fn draw_node(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode { primitive, clip }),
    })
}

fn text_node(node_id: NodeId, rect: Rect, text: &str, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id,
            rect,
            text: AnnotatedString::from(text),
            text_style: TextStyle::from_span_style(SpanStyle {
                color: Some(FOREGROUND_COLOR),
                ..Default::default()
            }),
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
            let fixture = case.fixture();
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
