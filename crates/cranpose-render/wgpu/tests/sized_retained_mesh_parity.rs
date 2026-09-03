mod support;

use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, LayerNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CommandReplayState, CornerRadii, DrawScope, DrawScopeDefault, GraphicsLayer,
    Point, Rect, Stroke,
};

const SIZE: u32 = 408;
const CENTER: f32 = 204.0;
const FRAMES: usize = 6;

const RING_INNER: f32 = 140.0;
const RING_OUTER: f32 = 160.0;

const RIM_RECT: Rect = Rect {
    x: 24.0,
    y: 24.0,
    width: 360.0,
    height: 360.0,
};
const RIM_RADIUS: f32 = 180.0;
const RIM_STROKE_WIDTH: f32 = 10.0;

const BRICK_ORBITS: usize = 3;
const BRICKS_PER_ORBIT: usize = 180;
const BRICK_COUNT: usize = BRICK_ORBITS * BRICKS_PER_ORBIT;

fn record_frame(frame: usize) -> DrawScopeDefault {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    scope.draw_annular_sector(
        Brush::solid(Color(0.3, 0.5, 0.8, 1.0)),
        Point::new(CENTER, CENTER),
        RING_INNER,
        RING_OUTER,
        0.0,
        std::f32::consts::TAU,
    );
    for orbit in 0..BRICK_ORBITS {
        let mid = 62.0 + orbit as f32 * 14.0;
        for i in 0..BRICKS_PER_ORBIT {
            let start =
                i as f32 * (std::f32::consts::TAU / BRICKS_PER_ORBIT as f32) + orbit as f32 * 0.07;
            scope.draw_annular_sector(
                Brush::solid(Color(0.9, 0.6 + ((i + orbit) % 4) as f32 * 0.08, 0.3, 1.0)),
                Point::new(CENTER, CENTER),
                mid - 3.0,
                mid + 3.0,
                start,
                0.012,
            );
        }
    }
    scope.draw_round_rect_at_stroked(
        RIM_RECT,
        Brush::solid(Color(0.35, 0.75, 0.95, 1.0)),
        CornerRadii::uniform(RIM_RADIUS),
        Stroke {
            width: RIM_STROKE_WIDTH,
            ..Default::default()
        },
    );
    for i in 0..5u32 {
        let angle = (i as f32 + 0.5) * (std::f32::consts::TAU / 5.0);
        scope.draw_circle(
            Brush::solid(Color(0.4, 0.95, 0.55, 0.8)),
            Point::new(
                CENTER + angle.cos() * RIM_RADIUS,
                CENTER + angle.sin() * RIM_RADIUS,
            ),
            6.0,
        );
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.7)),
            Point::new(
                CENTER + angle.cos() * (RING_INNER + 10.0),
                CENTER + angle.sin() * (RING_INNER + 10.0),
            ),
            5.0,
        );
    }
    for m in 0..(2 + frame % 3) {
        let x = 26.0 + frame as f32 * 9.0 + m as f32 * 15.0;
        scope.draw_circle(
            Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
            Point::new(x, 18.0),
            4.0,
        );
    }
    scope
}

fn build_sequence(node_id: usize) -> Vec<RenderGraph> {
    let mut state = CommandReplayState::default();
    let command = DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    (0..FRAMES)
        .map(|frame| {
            let scope = record_frame(frame);
            let outcome = state.advance(scope.recorded());
            let center = state.center();
            let (finished, replay) = scope.finish_replay(center, outcome, &mut |_| false);
            let bounds = Rect {
                x: 0.0,
                y: 0.0,
                width: SIZE as f32,
                height: SIZE as f32,
            };
            RenderGraph::new(LayerNode {
                node_id: None,
                wraps: None,
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
                has_origin_sinks: false,
                isolation: IsolationReasons::default(),
                cache_policy: CachePolicy::None,
                cache_hashes: LayerRasterCacheHashes::default(),
                cache_hashes_valid: false,
                children: vec![RenderNode::DrawRun(DrawRunNode::for_command_replayed(
                    PrimitivePhase::BeforeChildren,
                    Some(command),
                    std::rc::Rc::new(finished.primitives),
                    replay.map(Box::new),
                ))],
            })
        })
        .collect()
}

fn render_sequence(renderer: &mut support::LockedRenderer, graphs: &[RenderGraph]) -> Vec<Vec<u8>> {
    graphs
        .iter()
        .enumerate()
        .map(|(frame, graph)| {
            renderer.scene_mut().graph = Some(graph.clone());
            let captured = renderer
                .capture_frame(SIZE, SIZE)
                .unwrap_or_else(|err| panic!("frame {frame} capture failed: {err:?}"));
            assert_eq!((captured.width, captured.height), (SIZE, SIZE));
            captured.pixels
        })
        .collect()
}

fn clear_env() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_MESH_PX2", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", None);
}

#[test]
fn size_gated_retained_mesh_holds_identity_parity_and_gates_per_threshold() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", Some("0"));
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
            eprintln!("skipping sized retained mesh parity: headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMMAND_FEED", Some("1"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_STATIC_SPAN", Some("0"));

    let graphs_quad = build_sequence(7);
    let graphs_mesh = build_sequence(8);
    let graphs_gated = build_sequence(9);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    let _capture_quad = render_sequence(&mut renderer, &graphs_quad);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("1"));
    let _capture_mesh = render_sequence(&mut renderer, &graphs_mesh);

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("0"));
    let quad_frames = render_sequence(&mut renderer, &graphs_quad);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_ARC_MESH", Some("1"));
    let mesh_frames = render_sequence(&mut renderer, &graphs_mesh);

    let (mesh_slots, total_slots) = renderer.replay_slot_mesh_stats();
    let (arcs_meshed, rims_meshed, passthrough) = renderer.replay_slot_mesh_engagement();
    eprintln!(
        "sized-mesh slots: {mesh_slots} of {total_slots}; engagement: \
         {arcs_meshed} arcs, {rims_meshed} rims, {passthrough} quads"
    );
    assert!(mesh_slots >= 1, "the mesh arm must hold meshed slots");
    assert!(
        mesh_slots < total_slots,
        "the quad arm's slots must have captured without meshes"
    );
    assert_eq!(
        (arcs_meshed, rims_meshed),
        (1, 1),
        "the size gate must mesh exactly the big ring and the big rim"
    );
    assert!(
        passthrough >= BRICK_COUNT,
        "every brick arc must stay instanced ({passthrough} instanced)"
    );

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_MESH_PX2", Some("262144"));
    let _capture_gated = render_sequence(&mut renderer, &graphs_gated);
    let _gated_frames = render_sequence(&mut renderer, &graphs_gated);
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RETAINED_MESH_PX2", None);
    let (mesh_slots_after, total_slots_after) = renderer.replay_slot_mesh_stats();
    assert_eq!(
        mesh_slots_after, mesh_slots,
        "a ceiling threshold must keep every new capture on the quad path"
    );
    assert!(
        total_slots_after > total_slots,
        "the gated arm must actually have captured slots of its own"
    );
    assert_eq!(
        renderer.replay_slot_mesh_engagement(),
        (arcs_meshed, rims_meshed, passthrough),
        "a meshless capture must not move the engagement counters"
    );

    clear_env();
    assert!(
        !renderer.instanced_quads_active(),
        "the pinned-off selection must have latched at construction"
    );

    let on_meshed_band = |index: usize| {
        let pixel = index / 4;
        let (x, y) = (pixel % SIZE as usize, pixel / SIZE as usize);
        let radius = ((x as f32 + 0.5 - CENTER).powi(2) + (y as f32 + 0.5 - CENTER).powi(2)).sqrt();
        let rim_half = RIM_STROKE_WIDTH * 0.5;
        (RING_INNER - 4.0..=RING_OUTER + 4.0).contains(&radius)
            || (RIM_RADIUS - rim_half - 4.0..=RIM_RADIUS + rim_half + 4.0).contains(&radius)
    };
    for (frame, (quad, mesh)) in quad_frames.iter().zip(&mesh_frames).enumerate() {
        assert_eq!(quad.len(), mesh.len());
        let mut differing = 0usize;
        let mut worst = 0u8;
        let mut off_band = 0usize;
        for (index, (a, b)) in quad.iter().zip(mesh).enumerate() {
            let diff = a.abs_diff(*b);
            if diff > 0 {
                differing += 1;
                worst = worst.max(diff);
                if !on_meshed_band(index) {
                    off_band += 1;
                }
            }
        }
        eprintln!("frame {frame}: differing {differing} worst {worst} off-band {off_band}");
        if frame < 2 {
            assert_eq!(
                differing, 0,
                "frame {frame}: dynamic frames must be byte-exact"
            );
        } else {
            assert_eq!(
                off_band, 0,
                "frame {frame}: {off_band} bytes diverged OUTSIDE the meshed bands — \
                 passthrough content must be byte-exact at identity"
            );
            let mesh_distance: u64 = quad
                .iter()
                .zip(mesh)
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            let temporal_distance: u64 = quad_frames[frame]
                .iter()
                .zip(&quad_frames[frame - 1])
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            let temporal_worst = quad_frames[frame]
                .iter()
                .zip(&quad_frames[frame - 1])
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(
                mesh_distance * 4 < temporal_distance,
                "frame {frame}: mesh output is not materially closer to its same-frame \
                 quad control than to the previous-frame negative control ({mesh_distance} vs \
                 {temporal_distance}, worst {worst})"
            );
            assert!(
                worst <= 2,
                "frame {frame}: mesh interpolation exceeded the absolute identity envelope \
                 (differing {differing}, worst {worst})"
            );
            assert!(worst <= temporal_worst);
        }
    }
}
