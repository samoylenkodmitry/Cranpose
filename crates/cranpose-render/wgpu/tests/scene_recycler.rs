use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};

use crate::support;

const WIDTH: u32 = 96;
const HEIGHT: u32 = 80;

fn layer(node_id: usize, color: Color, rect: Rect) -> RenderNode {
    let mut layer = support::contract_layer(
        Some(node_id),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH as f32,
            height: HEIGHT as f32,
        },
        ProjectiveTransform::identity(),
        vec![support::solid_rect(rect, color)],
    );
    layer.graphics_layer.compositing_strategy =
        cranpose_ui_graphics::CompositingStrategy::Offscreen;
    RenderNode::Layer(Box::new(layer))
}

fn backdrop_graph(enabled: bool) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: WIDTH as f32,
        height: HEIGHT as f32,
    };
    let mut leaf = support::contract_layer(
        Some(9_410),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        vec![support::solid_rect(
            Rect {
                x: 24.0,
                y: 18.0,
                width: 48.0,
                height: 42.0,
            },
            Color(0.9, 0.3, 0.1, 0.65),
        )],
    );
    leaf.graphics_layer = GraphicsLayer {
        compositing_strategy: cranpose_ui_graphics::CompositingStrategy::Offscreen,
        backdrop_effect: enabled.then(|| RenderEffect::blur(4.0)),
        ..Default::default()
    }
    .into();
    let mut middle = support::contract_layer(
        Some(9_411),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        {
            let mut children = support::striped_page(WIDTH, HEIGHT);
            children.push(RenderNode::Layer(Box::new(leaf)));
            children
        },
    );
    middle.graphics_layer = GraphicsLayer {
        compositing_strategy: cranpose_ui_graphics::CompositingStrategy::Offscreen,
        ..Default::default()
    }
    .into();
    let mut outer = support::contract_layer(
        Some(9_412),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        vec![RenderNode::Layer(Box::new(middle))],
    );
    outer.graphics_layer = GraphicsLayer {
        compositing_strategy: cranpose_ui_graphics::CompositingStrategy::Offscreen,
        ..Default::default()
    }
    .into();
    graph(vec![RenderNode::Layer(Box::new(outer))])
}

fn graph(children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(support::layer_node(
        None,
        WIDTH as f32,
        HEIGHT as f32,
        children,
    ))
}

#[test]
fn returned_packet_storage_reuses_both_slots_after_topology_shrinks() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping scene recycler rendering check: {err}");
            return;
        }
    };

    let next_graph = graph(vec![layer(
        9_302,
        Color::from_rgb_u8(25, 180, 230),
        Rect {
            x: 28.0,
            y: 20.0,
            width: 30.0,
            height: 24.0,
        },
    )]);
    let mut shadowed = support::contract_layer(
        Some(9_300),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH as f32,
            height: HEIGHT as f32,
        },
        ProjectiveTransform::identity(),
        vec![
            layer(
                9_301,
                Color::from_rgb_u8(235, 45, 35),
                Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 38.0,
                    height: 32.0,
                },
            ),
            layer(
                9_303,
                Color::from_rgb_u8(35, 225, 70),
                Rect {
                    x: 54.0,
                    y: 35.0,
                    width: 28.0,
                    height: 26.0,
                },
            ),
        ],
    );
    shadowed.graphics_layer = GraphicsLayer {
        shadow_elevation: 9.0,
        ..Default::default()
    }
    .into();
    let stale_graph = graph(vec![RenderNode::Layer(Box::new(shadowed))]);
    renderer.scene_mut().graph = Some(stale_graph.clone());
    renderer.set_inspector_overlay(Some(stale_graph.clone()));
    let older_packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("first shadowed root and overlay packet builds");
    renderer.scene_mut().graph = Some(stale_graph.clone());
    renderer.set_inspector_overlay(Some(stale_graph));
    let newer_packet = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("second shadowed root and overlay packet builds");
    renderer.return_held_packet_for_tests(older_packet);
    renderer.return_held_packet_for_tests(newer_packet);

    renderer.set_inspector_overlay(None);
    renderer.scene_mut().graph = Some(next_graph.clone());
    let first_small = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("first small packet builds from a returned slot");
    let second_small = renderer
        .build_frame_packet_for_tests(WIDTH, HEIGHT)
        .expect("second small packet builds from the other returned slot");

    let (texture, view) = support::render_target(
        renderer.try_device().expect("device"),
        WIDTH,
        HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let expected = support::present_and_read(&mut renderer, WIDTH, HEIGHT, next_graph);
    for (index, packet) in [first_small, second_small].into_iter().enumerate() {
        let outcome = renderer
            .render_held_packet_for_tests(&texture, &view, WIDTH, HEIGHT, packet)
            .expect("small packet renders");
        assert_eq!(outcome, cranpose_render_wgpu::PresentOutcome::Presented);
        let pixels = support::read_texture(
            renderer.try_device().expect("device"),
            renderer.try_queue_for_tests().expect("queue"),
            &texture,
        );
        assert_eq!(
            pixels, expected,
            "small packet {index} must not retain prior topology, shadows, or overlay draws"
        );
    }
}

#[test]
fn reused_deep_scene_updates_backdrop_presence() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping scene recycler backdrop check: {err}");
            return;
        }
    };

    let plain = backdrop_graph(false);
    let expected = support::present_and_read(&mut renderer, WIDTH, HEIGHT, plain.clone());
    let with_backdrop =
        support::present_and_read(&mut renderer, WIDTH, HEIGHT, backdrop_graph(true));
    assert!(
        with_backdrop != expected,
        "the deepest layer's backdrop must affect the picture"
    );

    let actual = support::present_and_read(&mut renderer, WIDTH, HEIGHT, plain);
    assert_eq!(
        actual, expected,
        "a returned deep backdrop must not remain active after its shell is reused"
    );
}
