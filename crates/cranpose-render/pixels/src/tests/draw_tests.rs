use cranpose_render_common::{
    brush_sampling::normalize_gradient_t,
    graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
};
use cranpose_ui::Brush;
use cranpose_ui_graphics::{Color, TileMode};

use super::*;

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
        wraps: None,
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
        has_origin_sinks: false,
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

    assert!(is_inked(&frame, 44, 33), "inside the sector near 0 degrees");
    assert!(
        is_inked(&frame, 33, 44),
        "inside the sector near 90 degrees"
    );
    assert!(
        is_background(&frame, 44, 30),
        "past the flat radial start edge must be empty"
    );
    assert!(
        is_background(&frame, 30, 44),
        "past the flat radial end edge must be empty"
    );
    assert!(is_background(&frame, 35, 35), "inner hole");
    assert!(is_background(&frame, 52, 33), "beyond the outer radius");
}

#[test]
fn arc_and_stroke_rasterization_never_writes_nan_or_panics() {
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
