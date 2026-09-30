use cranpose_render_common::graph::{
    DrawRunNode, LayerNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, DrawPrimitive, ImageBitmap, ImageSampling, Rect,
};

use crate::support;

fn graph(
    image: &ImageBitmap,
    sampling: ImageSampling,
    filter: Option<ColorFilter>,
    blend_mode: BlendMode,
    transformed: bool,
) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: support::SIZE as f32,
        height: support::SIZE as f32,
    };
    let rect = Rect {
        x: 18.25,
        y: 25.75,
        width: 179.5,
        height: 163.25,
    };
    let make_image = |image: ImageBitmap, x: f32| DrawPrimitive::Blend {
        blend_mode,
        primitive: Box::new(DrawPrimitive::Image {
            rect: Rect { x, ..rect },
            image,
            alpha: 0.73,
            color_filter: filter,
            sampling,
            src_rect: Some(Rect {
                x: 14.0,
                y: 1.0,
                width: -12.0,
                height: 13.0,
            }),
        }),
    };
    let rgba = ImageBitmap::from_rgba8(16, 16, image.rgba8_pixels().into_owned()).expect("rgba");
    let images = LayerNode {
        local_bounds: bounds,
        transform_to_parent: if transformed {
            ProjectiveTransform::from_rect_to_quad(
                bounds,
                [[23.0, 9.0], [242.0, 28.0], [8.0, 233.0], [227.0, 252.0]],
            )
        } else {
            ProjectiveTransform::identity()
        },
        children: vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![
                DrawPrimitive::Rect {
                    rect: bounds,
                    brush: Brush::solid(Color::from_rgba_u8(212, 177, 133, 255)),
                    stroke: None,
                },
                make_image(image.clone(), rect.x),
                make_image(rgba, rect.x + 6.5),
                make_image(image.clone(), rect.x + 13.0),
            ],
        ))],
        ..LayerNode::default()
    };
    RenderGraph::new(LayerNode {
        local_bounds: bounds,
        children: vec![
            RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                vec![DrawPrimitive::Rect {
                    rect: bounds,
                    brush: Brush::solid(Color::from_rgba_u8(41, 71, 101, 255)),
                    stroke: None,
                }],
            )),
            RenderNode::Layer(Box::new(images)),
        ],
        ..LayerNode::default()
    })
}

#[test]
fn alpha_masks_match_rgba_pixels_across_sampling_filters_blends_and_transforms() {
    assert_alpha_mask_parity(wgpu::Backends::PRIMARY);
}

#[test]
#[cfg(all(target_os = "linux", feature = "backend-gles"))]
fn alpha_masks_match_rgba_pixels_on_gl() {
    assert_alpha_mask_parity(wgpu::Backends::GL);
}

fn assert_alpha_mask_parity(backends: wgpu::Backends) {
    let mut renderer = support::headless_renderer_configured(wgpu::Limits::default(), backends)
        .expect("GPU for alpha-mask parity");
    let mask = ImageBitmap::from_alpha8(16, 16, [19, 127, 239], (0..=255).collect()).expect("mask");
    let rgba = ImageBitmap::from_rgba8(16, 16, mask.rgba8_pixels().into_owned()).expect("rgba");
    let matrix = ColorFilter::Matrix([
        0.0, 0.7, 0.0, 0.2, 0.1, 0.0, 0.0, 0.8, 0.0, 0.1, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        0.6, 0.2,
    ]);
    for sampling in [ImageSampling::Nearest, ImageSampling::Linear] {
        for filter in [
            None,
            Some(ColorFilter::Modulate(Color(0.7, 0.5, 0.3, 0.8))),
            Some(ColorFilter::Tint(Color(0.4, 0.2, 0.9, 0.6))),
            Some(matrix),
        ] {
            for blend in [BlendMode::SrcOver, BlendMode::DstOut] {
                for transformed in [false, true] {
                    let expected = support::settled_capture(
                        &mut renderer,
                        &graph(&rgba, sampling, filter, blend, transformed),
                    );
                    let actual = support::settled_capture(
                        &mut renderer,
                        &graph(&mask, sampling, filter, blend, transformed),
                    );
                    assert!(
                        support::distinct_colors(&expected) > 10,
                        "scene must draw varied coverage: {sampling:?} {filter:?} {blend:?} transformed={transformed}, colors={}",
                        support::distinct_colors(&expected)
                    );
                    let differing = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                    let worst = actual
                        .iter()
                        .zip(&expected)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or_default();
                    assert_eq!(
                        differing, 0,
                        "{sampling:?} {filter:?} {blend:?} transformed={transformed}: differing={differing}, worst={worst}"
                    );
                }
            }
        }
    }
}
