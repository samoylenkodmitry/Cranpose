//! The shared cases that draw text: plain and decorated text moved by a
//! layer, wrapped lines and a clipped line.

use cranpose_core::NodeId;
use cranpose_ui::{
    TextLayoutOptions, TextStyle,
    text::{Shadow, SpanStyle, TextDecoration},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Point, Rect};

use super::{
    FOREGROUND_COLOR, NormalizedDifferenceBudget, RenderFixture, RenderedFrame,
    assert_normalized_region_matches, build_fixture, build_translated_fixture_with_context,
    count_non_background_pixels, count_non_background_pixels_in_band, draw_node, ink_y_range,
    normalize_frame_region, normalized_output_dimensions,
};
use crate::{
    graph::{PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderNode, TextPrimitiveNode},
    image_compare::sample_pixel,
};

const TRANSLATED_PLAIN_TEXT_BUDGET: NormalizedDifferenceBudget = NormalizedDifferenceBudget {
    max_differing_pixels: 550,
    max_pixel_difference: 360,
};

const TRANSLATED_TEXT_DECORATIONS_BUDGET: NormalizedDifferenceBudget = NormalizedDifferenceBudget {
    max_differing_pixels: 320,
    max_pixel_difference: 400,
};

pub(super) fn translated_plain_text_fixture(
    translation_x: f32,
    translation_y: f32,
) -> RenderFixture {
    let subtree_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 116.0,
        height: 36.0,
    };

    let mut fixture = build_translated_fixture_with_context(
        196,
        112,
        subtree_bounds,
        Point::new(translation_x, translation_y),
        true,
        vec![
            draw_node(
                DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: 2.0,
                        y: 2.0,
                        width: 112.0,
                        height: 32.0,
                    },
                    brush: Brush::solid(Color(0.24, 0.26, 0.40, 0.92)),
                    radii: CornerRadii::uniform(8.0),
                    stroke: None,
                },
                None,
            ),
            text_node(
                NodeId(33),
                Rect {
                    x: 10.0,
                    y: 8.0,
                    width: 96.0,
                    height: 18.0,
                },
                "Scroll text",
                None,
            ),
        ],
    );
    fixture.normalized_rect = Some(Rect {
        x: translation_x.round(),
        y: translation_y.round(),
        width: subtree_bounds.width,
        height: subtree_bounds.height,
    });
    fixture
}

pub(super) fn multiline_text_fixture() -> RenderFixture {
    build_fixture(
        220,
        100,
        vec![text_node(
            NodeId(1),
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

pub(super) fn clipped_text_fixture() -> RenderFixture {
    build_fixture(
        220,
        100,
        vec![text_node(
            NodeId(2),
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

pub(super) fn translated_text_decorations_fixture(
    translation_x: f32,
    translation_y: f32,
) -> RenderFixture {
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

    let mut fixture = build_translated_fixture_with_context(
        180,
        96,
        subtree_bounds,
        Point::new(translation_x, translation_y),
        true,
        vec![text_node_with_style(
            NodeId(3),
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
    );
    fixture.normalized_rect = Some(Rect {
        x: translation_x.round(),
        y: translation_y.round(),
        width: subtree_bounds.width,
        height: subtree_bounds.height,
    });
    fixture
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
    let text = cranpose_ui::text::shared_plain_annotated_string(text);
    let prepared =
        cranpose_ui::text::prepare_text_layout(&text, &text_style, Default::default(), None);
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id,
            rect,
            render_text: prepared.render_text(),
            text,
            layout: prepared.layout.clone(),
            text_style: std::sync::Arc::new(text_style),
            font_size: 14.0,
            layout_options: TextLayoutOptions::default(),
            clip,
            paint: Default::default(),
        })),
    })
}

pub(super) fn assert_multiline_text_frame(pixels: &[u8], width: u32, height: u32) {
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

pub(super) fn assert_translated_plain_text_frames(frames: &[RenderedFrame]) {
    let [base, moved] = frames else {
        panic!("translated_plain_text expects exactly two rendered frames");
    };
    assert_eq!((base.width, base.height), (196, 112));
    assert_eq!((moved.width, moved.height), (196, 112));
    assert_ne!(
        base.pixels, moved.pixels,
        "translated plain text contract should move within the full frame"
    );
    assert_normalized_region_matches(
        base,
        moved,
        TRANSLATED_PLAIN_TEXT_BUDGET,
        "translated plain text should remain visually stable after normalization",
    );
}

pub(super) fn assert_translated_text_decorations_frames(frames: &[RenderedFrame]) {
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

pub(super) fn assert_clipped_text_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (220, 100));
    let background = sample_pixel(pixels, width, 2, 2);
    let total_ink = count_non_background_pixels(pixels, width, height, background);
    assert_eq!(
        total_ink, 0,
        "fully clipped text should not draw ink, but observed {total_ink} differing pixels"
    );
}
