use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_ui_graphics::{GraphicsLayer, Rect};

use super::*;
use crate::{
    WgpuTextSystem,
    frame_packet::{FrameSceneStorage, HeldWindow},
    target_state::TargetDrawn,
    test_support::layer_node,
};

fn frontend() -> RendererFrontend {
    RendererFrontend::new(WgpuTextSystem::from_fonts(&[]).fonts().clone())
}

fn graph() -> LayerNode {
    layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 160.0,
        },
        ProjectiveTransform::default(),
        GraphicsLayer::default(),
        vec![],
    )
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
    frontend.scene.describe(&graph());
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
        .build_frame_packet_with_scale((320, 240, 1.0), 0, 0, PacketFor::Capture)
        .expect("second frame collects too");
    assert_eq!(next.frame_id, 2, "frame sequence must be monotone");
    assert_eq!(next.root_scale, 1.0, "explicit scale must win");
}

#[test]
fn dev_overlay_collects_into_packet() {
    let mut frontend = frontend();
    frontend.scene.describe(&graph());
    frontend.dev_overlay.describe(&graph());

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
    frontend.scene.describe(&graph());
    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    let mut root = packet.root;
    root.scene.runs.reserve(64);
    let hint = root.scene.capacity_hint();
    frontend.apply_returns(RenderReturns {
        scene: Some(FrameSceneStorage {
            root,
            overlay: packet.overlay,
        }),
        ..RenderReturns::default()
    });
    assert_eq!(frontend.root_scene_capacity, hint);
}

// What came back of `packet` with `outcome`, drawn whole into a target that
// then holds it when it presented.
#[cfg(not(target_arch = "wasm32"))]
fn returned(packet: FramePacket, outcome: PresentOutcome) -> RenderReturns {
    let presented = outcome == PresentOutcome::Presented;
    RenderReturns {
        frame_id: packet.frame_id,
        target: presented.then_some(TargetDrawn {
            damage: Damage::Full,
            skipped: false,
            knew_before: false,
            unheld: true,
            cleared: false,
        }),
        held: presented.then_some(HeldWindow {
            oldest: packet.frame_id,
            stand_ins: Damage::Part(DamageRects::default()),
        }),
        scene: Some(FrameSceneStorage {
            root: packet.root,
            overlay: packet.overlay,
        }),
        outcome,
        ..RenderReturns::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_walk_the_targets_make_whole_waits_for_the_frames_in_flight() {
    let mut frontend = frontend();
    frontend.scene.describe(&graph());
    assert!(
        matches!(frontend.next_overlap(0), PacketOverlap::Alone),
        "a window's images hold nothing at its start"
    );
    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    frontend.apply_returns(returned(packet, PresentOutcome::Presented));
    assert!(
        matches!(frontend.next_overlap(0), PacketOverlap::WithFramesInFlight),
        "a walk culled against the frame the images hold goes with others"
    );

    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    frontend.apply_returns(returned(
        packet,
        PresentOutcome::Cancelled(CancelReason::NeedsWhole),
    ));
    assert!(
        matches!(frontend.next_overlap(0), PacketOverlap::Alone),
        "the walk after a refusal draws whole, and waits for the frames in flight"
    );
    frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    assert!(
        matches!(frontend.next_overlap(0), PacketOverlap::WithFramesInFlight),
        "the whole walk ends the wait"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_walk_for_a_new_surface_waits_for_the_frames_in_flight() {
    let mut frontend = frontend();
    frontend.scene.describe(&graph());
    let packet = frontend
        .build_frame_packet(320, 240, 0, 0)
        .expect("a graph collects into a packet");
    frontend.apply_returns(returned(packet, PresentOutcome::Presented));
    assert!(
        matches!(frontend.next_overlap(0), PacketOverlap::WithFramesInFlight),
        "the images hold the frame drawn into them"
    );
    assert!(
        matches!(frontend.next_overlap(1), PacketOverlap::Alone),
        "a resized or new surface hands out images that hold nothing the window drew"
    );
    let packet = frontend
        .build_frame_packet(320, 240, 0, 1)
        .expect("a graph collects into a packet");
    assert!(
        matches!(frontend.next_overlap(1), PacketOverlap::Alone),
        "the frames wait until a frame drawn into the new images comes back"
    );
    frontend.apply_returns(returned(packet, PresentOutcome::Presented));
    assert!(
        matches!(frontend.next_overlap(1), PacketOverlap::WithFramesInFlight),
        "the new images hold that frame"
    );
}
