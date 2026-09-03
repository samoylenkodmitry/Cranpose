mod support;

use support::graph::*;

const SIZE: u32 = 400;
const RIM_RECT: Rect = Rect {
    x: 40.0,
    y: 40.0,
    width: 320.0,
    height: 320.0,
};
const RIM_STROKE_WIDTH: f32 = 8.0;
const RIM_RADIUS: f32 = 160.0;

fn record_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.03, 0.03, 0.06, 1.0)),
    );
    for i in 0..5u32 {
        let angle = i as f32 * (std::f32::consts::TAU / 5.0);
        scope.draw_circle(
            Brush::solid(Color(0.9, 0.5, 0.2, 1.0)),
            Point::new(
                200.0 + angle.cos() * RIM_RADIUS,
                200.0 + angle.sin() * RIM_RADIUS,
            ),
            7.0,
        );
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
                200.0 + angle.cos() * RIM_RADIUS,
                200.0 + angle.sin() * RIM_RADIUS,
            ),
            6.0,
        );
    }
    for m in 0..6u32 {
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.9)),
            Point::new(30.0 + m as f32 * 68.0, 14.0),
            5.0,
        );
    }
}

fn rim_graph() -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record_scene(&mut scope);
    draw_run_graph(SIZE, scope)
}

fn render_arm(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    support::stable_capture(renderer, graph, SIZE)
}

#[test]
fn rim_band_mesh_matches_the_quad_expansion() {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RIM_MESH", None);
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rim mesh parity: headless WGPU init failed: {err}");
            return;
        }
    };
    if !renderer.instanced_quads_active() {
        eprintln!("skipping rim mesh parity: instanced quads inactive (uniform mode)");
        return;
    }

    let graph = rim_graph();

    let emitted_before = renderer.rim_meshes_emitted();
    let meshed = render_arm(&mut renderer, &graph);
    let emitted_meshed = renderer.rim_meshes_emitted();
    assert!(
        emitted_meshed > emitted_before,
        "arm A must actually draw the rim as a band mesh (counter stayed at {emitted_before})"
    );

    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RIM_MESH", Some("0"));
    let quad = render_arm(&mut renderer, &graph);
    let emitted_after_off = renderer.rim_meshes_emitted();
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_RIM_MESH", None);
    assert_eq!(
        emitted_after_off, emitted_meshed,
        "arm B must draw every rim through the quad expansion"
    );

    assert_eq!(meshed.len(), quad.len());
    let mut differing = 0usize;
    let mut beyond_one = 0usize;
    let mut worst = 0u8;
    for (a, b) in meshed.iter().zip(&quad) {
        let diff = a.abs_diff(*b);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
            if diff > 1 {
                beyond_one += 1;
            }
        }
    }
    eprintln!(
        "rim-meshed-vs-quad: differing {differing} (beyond ±1: {beyond_one}) worst {worst}; \
         rims meshed {}",
        emitted_meshed - emitted_before
    );
    assert_eq!(
        differing, 0,
        "{differing} bytes diverged ({beyond_one} beyond ±1, worst {worst}) — \
         the rim band mesh must rasterize byte-identically to the bounding quad"
    );
}
