use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_ui_graphics::{GraphicsLayer, Rect};

use super::*;
use crate::{WgpuTextSystem, test_support::layer_node};

fn frontend() -> RendererFrontend {
    let text_system = WgpuTextSystem::from_fonts(&[]);
    RendererFrontend::new(text_system.render_state(), text_system.software_fonts())
}

fn graph() -> RenderGraph {
    RenderGraph::new(layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 160.0,
        },
        ProjectiveTransform::default(),
        GraphicsLayer::default(),
        vec![],
    ))
}

#[test]
fn build_without_graph_produces_no_packet() {
    let mut frontend = frontend();
    assert!(frontend.build_frame_packet(320, 240, 0, 0).is_none());
    assert_eq!(frontend.frame_sequence, 0);
}

#[test]
fn root_builds_packet_with_frame_scalars() {
    let mut frontend = frontend();
    frontend.scene.graph = Some(graph());
    frontend.root_scale = 2.0;

    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    assert_eq!(packet.frame_id, 1);
    assert_eq!(packet.viewport, (320, 240));
    assert_eq!(packet.root_scale, 2.0);
    assert!(packet.root.children.is_empty());
    assert!(packet.overlay.is_none());

    let next = frontend
        .build_frame_packet_with_scale(320, 240, 1.0, 0, 0)
        .expect("second frame collects too");
    assert_eq!(next.frame_id, 2, "frame sequence must be monotone");
    assert_eq!(next.root_scale, 1.0, "explicit scale must win");
}

#[test]
fn dev_overlay_collects_into_packet() {
    let mut frontend = frontend();
    frontend.scene.graph = Some(graph());
    frontend.dev_overlay_graph = Some(graph());

    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    let overlay = packet
        .overlay
        .as_ref()
        .expect("the dev overlay is collected producer-side");
    assert!(overlay.children.is_empty());
}

#[test]
fn apply_returns_seeds_the_next_collect_capacity() {
    let mut frontend = frontend();
    frontend.scene.graph = Some(graph());
    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    let mut scene = packet.root.scene;
    scene.runs.reserve(64);
    let hint = scene.capacity_hint();
    frontend.apply_returns(RenderReturns {
        scene: Some(scene),
        ..RenderReturns::default()
    });
    assert_eq!(frontend.root_scene_capacity, hint);
}
