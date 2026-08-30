//! Byte-level contracts of the two direct scene-range mechanisms.
//!
//! The prefix snapshot serves a frame's stable bottom EXACTLY: a claimed,
//! stored, or replayed prefix must produce the same bytes as rendering the
//! ops directly, in every cache phase. The small flatten class is inexact
//! by design — compositing through an intermediate texture cannot
//! reproduce the direct path's chained per-draw roundings — so its test
//! measures the envelope of that inexactness instead of asserting zero.

mod support;

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph, RenderNode,
    },
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Rect};

const SIZE: u32 = 600;
const FRAMES: usize = 3;

fn round_rect(rect: Rect, color: Color, radius: f32) -> PrimitiveEntry {
    PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::RoundRect {
                rect,
                brush: Brush::solid(color),
                radii: CornerRadii::uniform(radius),
                stroke: None,
            },
            clip: None,
        }),
    }
}

/// Overlapping translucent rounded rects at fractional coordinates: every
/// op blends over the previous one's anti-aliased edges, so any extra
/// rounding a replay path introduces shows up as differing bytes.
fn layered_graph(node_id: NodeId, region: Rect, frame: usize) -> RenderGraph {
    let step_x = region.width / 8.0;
    let step_y = region.height / 8.0;
    let mut primitives: Vec<RenderNode> = (0..5)
        .map(|index| {
            let offset = index as f32;
            RenderNode::Primitive(round_rect(
                Rect {
                    x: region.x + offset * step_x + 0.37,
                    y: region.y + offset * step_y + 0.61,
                    width: region.width - 3.0 * step_x,
                    height: region.height - 3.0 * step_y,
                },
                Color(
                    0.15 + 0.13 * offset,
                    0.72 - 0.11 * offset,
                    0.35 + 0.09 * offset,
                    0.55,
                ),
                9.0 + offset,
            ))
        })
        .collect();
    primitives.push(RenderNode::Primitive(round_rect(
        Rect {
            x: region.x + 4.3 + frame as f32 * 13.7,
            y: region.y + region.height * 0.4,
            width: region.width * 0.3,
            height: region.height * 0.3,
        },
        Color(0.9, 0.55, 0.2, 0.65),
        7.0,
    )));
    RenderGraph::new(support::layer_node(
        Some(node_id),
        SIZE as f32,
        SIZE as f32,
        primitives,
    ))
}

/// One pass over the sequence: per-frame pixels plus per-frame
/// (layer cache hits, misses) from the renderer's frame stats.
type PassObservations = (Vec<Vec<u8>>, Vec<(u32, u32)>);

fn render_pass(renderer: &mut support::LockedRenderer, graphs: &[RenderGraph]) -> PassObservations {
    let mut frames = Vec::with_capacity(graphs.len());
    let mut cache_traffic = Vec::with_capacity(graphs.len());
    for (frame, graph) in graphs.iter().enumerate() {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        let stats = renderer.last_frame_stats().expect("frame stats");
        frames.push(captured.pixels);
        cache_traffic.push((stats.layer_cache_hits, stats.layer_cache_misses));
    }
    (frames, cache_traffic)
}

fn assert_byte_exact(label: &str, truth: &[Vec<u8>], candidate: &[Vec<u8>]) {
    for (frame, (truth, candidate)) in truth.iter().zip(candidate).enumerate() {
        assert_eq!(truth.len(), candidate.len());
        let differing = truth.iter().zip(candidate).filter(|(a, b)| a != b).count();
        assert_eq!(
            differing, 0,
            "{label} frame {frame}: {differing} bytes differ from the direct render"
        );
    }
}

fn max_channel_delta(truth: &[Vec<u8>], candidate: &[Vec<u8>]) -> (u8, usize) {
    let mut max_delta = 0u8;
    let mut differing = 0usize;
    for (truth, candidate) in truth.iter().zip(candidate) {
        for (a, b) in truth.iter().zip(candidate) {
            let delta = a.abs_diff(*b);
            if delta > 0 {
                differing += 1;
                max_delta = max_delta.max(delta);
            }
        }
    }
    (max_delta, differing)
}

/// The prefix snapshot's whole contract: claim, store, and replay frames
/// all produce the direct path's bytes. The kill-switch arm renders first
/// and is the ground truth; its own two passes must agree so the truth is
/// not itself polluted by another cache warming up.
#[test]
fn a_prefix_snapshot_replays_the_direct_bytes_in_every_cache_phase() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping prefix snapshot exactness: headless WGPU init failed: {err}");
            return;
        }
    };
    // Content spans nearly the whole 600x600 target, so the flatten class
    // refuses the range on its byte floor and the truth passes stay pure
    // direct renders.
    let region = Rect {
        x: 5.0,
        y: 5.0,
        width: 590.0,
        height: 590.0,
    };
    let graphs: Vec<RenderGraph> = (0..FRAMES)
        .map(|frame| layered_graph(30_000, region, frame))
        .collect();

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_PREFIX_SNAPSHOT", Some("1"));
    let (truth_a, _) = render_pass(&mut renderer, &graphs);
    let (truth_b, _) = render_pass(&mut renderer, &graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_PREFIX_SNAPSHOT", None);
    assert_byte_exact("kill-switch stability", &truth_a, &truth_b);

    let (claim_frames, claim_traffic) = render_pass(&mut renderer, &graphs);
    let (store_frames, store_traffic) = render_pass(&mut renderer, &graphs);
    let (replay_frames, replay_traffic) = render_pass(&mut renderer, &graphs);
    let (replay_again_frames, replay_again_traffic) = render_pass(&mut renderer, &graphs);

    assert_byte_exact("claim pass", &truth_a, &claim_frames);
    assert_byte_exact("store pass", &truth_a, &store_frames);
    assert_byte_exact("replay pass", &truth_a, &replay_frames);
    assert_byte_exact("second replay pass", &truth_a, &replay_again_frames);

    for (frame, (hits, misses)) in claim_traffic.iter().enumerate() {
        assert_eq!(
            (*hits, *misses),
            (0, 0),
            "claim pass frame {frame} must render direct with no cache traffic"
        );
    }
    for (frame, (_, misses)) in store_traffic.iter().enumerate() {
        assert!(
            *misses >= 1,
            "store pass frame {frame} recorded no cache store"
        );
    }
    for (label, traffic) in [
        ("replay", &replay_traffic),
        ("second replay", &replay_again_traffic),
    ] {
        for (frame, (hits, _)) in traffic.iter().enumerate() {
            assert!(
                *hits >= 1,
                "{label} pass frame {frame} never hit the prefix entry"
            );
        }
    }
}

/// The small flatten class's inexactness has to stay small and stable:
/// store-frame and replay-frame composites must agree byte-for-byte with
/// each other, and their divergence from the direct render — the chained
/// roundings collapsed into one — is measured and reported here as the
/// class's envelope.
#[test]
fn the_small_flatten_class_stays_inside_a_measured_envelope() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping flatten envelope: headless WGPU init failed: {err}");
            return;
        }
    };
    // The prefix snapshot would claim this raw scene outright; the flatten
    // class only exists below its kill switch here.
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_PREFIX_SNAPSHOT", Some("1"));
    // Content confined to 300x300 so the range passes the flatten byte
    // floor and the whole scene lands in one cacheable chunk.
    let region = Rect {
        x: 30.0,
        y: 30.0,
        width: 300.0,
        height: 300.0,
    };
    let graphs: Vec<RenderGraph> = (0..FRAMES)
        .map(|frame| layered_graph(30_100, region, frame))
        .collect();

    let (direct_frames, _) = render_pass(&mut renderer, &graphs);
    let (store_frames, store_traffic) = render_pass(&mut renderer, &graphs);
    let (replay_frames, replay_traffic) = render_pass(&mut renderer, &graphs);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_DISABLE_PREFIX_SNAPSHOT", None);

    for (frame, (_, misses)) in store_traffic.iter().enumerate() {
        assert!(
            *misses >= 1,
            "store pass frame {frame} recorded no flatten store"
        );
    }
    for (frame, (hits, _)) in replay_traffic.iter().enumerate() {
        assert!(
            *hits >= 1,
            "replay pass frame {frame} never hit the flatten entry"
        );
    }

    // A store frame composites through the entry it just rendered, so it
    // must agree exactly with every later replay of that entry.
    assert_byte_exact("flatten store vs replay", &store_frames, &replay_frames);

    let (max_delta, differing) = max_channel_delta(&direct_frames, &replay_frames);
    let total: usize = direct_frames.iter().map(Vec::len).sum();
    eprintln!(
        "flatten envelope: max per-channel delta {max_delta}, {differing} of {total} bytes differ"
    );
    assert!(
        max_delta <= 3,
        "flatten envelope grew: max per-channel delta {max_delta} exceeds 3"
    );
}
