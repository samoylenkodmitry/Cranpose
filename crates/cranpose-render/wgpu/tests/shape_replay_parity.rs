//! Pixel parity for the similarity-replay cache.
//!
//! Renders a MEGA-boss-shaped scene — rings of arcs sharing one pivot, each
//! ring rotating at its own speed under a global breathing scale, plus
//! recoloring twinkle dots and free movers — twice over the same frame
//! sequence: once with replay disabled (every frame through the full
//! per-shape pipeline) and once with it enabled (stable segments replayed
//! from retained GPU captures under per-frame similarity transforms). The
//! frames must match pixel-for-pixel within blending noise, and the enabled
//! run must actually have replayed — a silently-disengaged cache would pass
//! parity vacuously.

mod support;

use cranpose_render_common::graph::{
    CachePolicy, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode,
};
use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, GraphicsLayer, Point, Rect};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 8;

fn arc_ring(children: &mut Vec<DrawPrimitive>, radius: f32, band: f32, delta: f32, count: usize) {
    let sweep = std::f32::consts::TAU / count as f32 * 0.8;
    for i in 0..count {
        let start = i as f32 * (std::f32::consts::TAU / count as f32) + delta;
        let reach = radius + 2.0;
        children.push(DrawPrimitive::Arc {
            rect: Rect {
                x: CENTER - reach,
                y: CENTER - reach,
                width: reach * 2.0,
                height: reach * 2.0,
            },
            brush: Brush::solid(Color(0.3, 0.5 + (i % 5) as f32 * 0.08, 0.8, 1.0)),
            center: Point::new(CENTER, CENTER),
            radius,
            start_angle: start,
            sweep_angle: sweep,
            stroke: None,
            inner_radius: radius - band,
        });
    }
}

/// One frame of the synthetic boss: three rings rotating at distinct speeds
/// under a shared breathing scale, a block of twinkle dots recoloring in
/// place, and a few movers at the run's head so segment anchors shift.
fn mega_like_graph(frame: usize) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    let breathing = 1.0 - 0.0005 * frame as f32;
    let mut children = vec![DrawPrimitive::Rect {
        rect: bounds,
        brush: Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
        stroke: None,
    }];
    // Movers ahead of the rings: their POSITIONS shift every frame and their
    // COUNT changes every frame. On a slow device the simulation steps far
    // enough between frames that no two consecutive frames carry the same
    // entry list — the bootstrap must align around the churn or it never
    // engages exactly where replay matters most.
    for m in 0..(2 + frame % 3) {
        let x = 30.0 + frame as f32 * 7.0 + m as f32 * 15.0;
        children.push(DrawPrimitive::RoundRect {
            rect: Rect {
                x,
                y: 40.0 + m as f32 * 12.0,
                width: 8.0,
                height: 8.0,
            },
            brush: Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
            radii: cranpose_ui_graphics::CornerRadii::uniform(4.0),
            stroke: None,
        });
    }
    for (ring, (radius, band, speed)) in [
        (150.0, 10.0, 0.013),
        (120.0, 9.0, -0.008),
        (90.0, 8.0, 0.019),
    ]
    .into_iter()
    .enumerate()
    {
        arc_ring(
            &mut children,
            radius * breathing,
            band * breathing,
            speed * frame as f32,
            420,
        );
        if ring == 1 {
            // A spark burst wedged between rings: count and positions both
            // change every frame, the way particle churn lands mid-run
            // between distant simulation steps. Alignment has to resync
            // across it so the ring behind still forms a chain.
            for s in 0..(30 + (frame * 13) % 25) {
                let a = s as f32 * 0.7 + frame as f32 * 0.31;
                let r = 60.0 + ((s * 17 + frame * 29) % 90) as f32;
                children.push(DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: CENTER + a.cos() * r - 2.5,
                        y: CENTER + a.sin() * r - 2.5,
                        width: 5.0,
                        height: 5.0,
                    },
                    brush: Brush::solid(Color(1.0, 0.6, 0.2, 0.8)),
                    radii: cranpose_ui_graphics::CornerRadii::uniform(2.5),
                    stroke: None,
                });
            }
        }
    }
    // Twinkle dots: static geometry, colors stepping per frame — the recolor
    // patch path.
    for d in 0..220 {
        let angle = d as f32 * 0.285;
        let orbit = 55.0 + (d % 7) as f32 * 3.0;
        let alpha = 0.25 + 0.7 * (((d + frame * 3) % 11) as f32 / 10.0);
        children.push(DrawPrimitive::Arc {
            rect: Rect {
                x: CENTER + angle.cos() * orbit - 3.0,
                y: CENTER + angle.sin() * orbit - 3.0,
                width: 6.0,
                height: 6.0,
            },
            brush: Brush::solid(Color(0.9, 0.85, 0.4, alpha)),
            center: Point::new(CENTER, CENTER),
            radius: orbit + 3.0,
            start_angle: angle - 0.02,
            sweep_angle: 0.04,
            stroke: None,
            inner_radius: orbit - 3.0,
        });
    }
    RenderGraph::new(LayerNode {
        node_id: None,
        local_bounds: bounds,
        transform_to_parent: ProjectiveTransform::identity(),
        content_offset: Point::default(),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: vec![RenderNode::DrawRun(DrawRunNode {
            phase: PrimitivePhase::BeforeChildren,
            primitives: children,
        })],
    })
}

fn render_sequence(renderer: &mut support::LockedRenderer) -> Vec<Vec<u8>> {
    (0..FRAMES)
        .map(|frame| {
            renderer.scene_mut().graph = Some(mega_like_graph(frame));
            let captured = renderer
                .capture_frame(SIZE, SIZE)
                .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
            assert_eq!((captured.width, captured.height), (SIZE, SIZE));
            captured.pixels
        })
        .collect()
}

#[test]
fn similarity_replay_matches_the_full_pipeline_pixel_for_pixel() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping similarity replay parity: headless WGPU init failed: {err}");
            return;
        }
    };

    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "0");
    let baseline = render_sequence(&mut renderer);
    let (segments_off, ..) = cranpose_render_wgpu::shape_replay_live_stats();
    assert_eq!(segments_off, 0, "disabled replay must hold no segments");

    std::env::set_var("CRANPOSE_SIMILARITY_REPLAY", "1");
    let replayed = render_sequence(&mut renderer);
    std::env::remove_var("CRANPOSE_SIMILARITY_REPLAY");

    let (segments_on, patches, frames, supported, phase) =
        cranpose_render_wgpu::shape_replay_live_stats();
    assert!(
        segments_on >= 3,
        "replay should hold the ring segments \
         (got {segments_on}; frames {frames}, supported {supported}, phase {phase})"
    );
    assert!(
        patches > 0,
        "twinkle recolors should have gone through the patch path"
    );

    for (frame, (baseline, replayed)) in baseline.iter().zip(&replayed).enumerate() {
        assert_eq!(baseline.len(), replayed.len());
        let mut worst = 0u8;
        let mut differing = 0usize;
        for (a, b) in baseline.iter().zip(replayed) {
            let diff = a.abs_diff(*b);
            worst = worst.max(diff);
            if diff > 3 {
                differing += 1;
            }
        }
        assert_eq!(
            differing, 0,
            "frame {frame}: {differing} channel values diverged by more than blending noise \
             (worst diff {worst})"
        );
    }
}
