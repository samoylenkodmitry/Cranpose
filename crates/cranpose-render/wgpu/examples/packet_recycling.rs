use std::{alloc::System, hint::black_box, time::Instant};

use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{
    Box, BoxSpec, Color, Column, ColumnSpec, GraphicsLayer, LayoutEngine, Modifier, Size,
    composable, run_test_composition,
};
use cranpose_ui_graphics::CompositingStrategy;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const WARMUP_FRAMES: usize = 32;
const FRAMES: usize = 512;

fn isolated() -> Modifier {
    Modifier::empty()
        .size(Size::new(160.0, 24.0))
        .graphics_layer_value(GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        })
}

#[composable]
fn FlatRow() {
    Box(
        Modifier::empty()
            .size(Size::new(160.0, 24.0))
            .background(Color::BLUE),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn IsolatedChain(depth: usize) {
    if depth == 1 {
        Box(isolated().background(Color::RED), BoxSpec::default(), || {});
    } else {
        Box(isolated(), BoxSpec::default(), move || {
            IsolatedChain(depth - 1);
        });
    }
}

fn run_case(rows: usize, depth: usize, in_flight: usize) {
    let height = rows as f32 * 24.0;
    let mut composition = run_test_composition(move || {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for _ in 0..rows {
                if depth == 0 {
                    FlatRow();
                } else {
                    IsolatedChain(depth);
                }
            }
        });
    });
    let root = composition.root().expect("composed root");
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let viewport = Size::new(200.0, height);
    applier.compute_layout(root, viewport).expect("layout");
    let mut renderer = WgpuRenderer::new(&[]);
    renderer
        .rebuild_scene_from_applier(&mut applier, root, viewport)
        .expect("scene graph");
    applier.clear_runtime_handle();

    let frame = |renderer: &mut WgpuRenderer| {
        let first = renderer
            .build_frame_packet_for_tests(200, height as u32)
            .expect("first packet collected without a GPU");
        if in_flight == 1 {
            renderer.return_held_packet_for_tests(black_box(first));
        } else {
            let second = renderer
                .build_frame_packet_for_tests(200, height as u32)
                .expect("second packet collected without a GPU");
            renderer.return_held_packet_for_tests(black_box(first));
            renderer.return_held_packet_for_tests(black_box(second));
        }
    };
    for _ in 0..WARMUP_FRAMES {
        frame(&mut renderer);
    }
    let region = Region::new(GLOBAL);
    let start = Instant::now();
    for _ in 0..FRAMES {
        frame(&mut renderer);
    }
    let elapsed = start.elapsed().as_nanos();
    let stats = region.change();
    let frames = FRAMES * in_flight;
    println!(
        "{{\"benchmark\":\"packet_recycling\",\"rows\":{rows},\"isolated_layers_per_row\":{depth},\"in_flight\":{in_flight},\"frames\":{frames},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
        stats.allocations,
        stats.reallocations,
        stats.bytes_allocated,
        stats.bytes_deallocated,
        elapsed / frames as u128,
    );
}

fn main() {
    for rows in [16, 64] {
        run_case(rows, 0, 1);
    }
    for rows in [1, 4, 8, 16, 64] {
        run_case(rows, 2, 1);
    }
    for depth in [1, 4, 8, 16, 32, 64] {
        run_case(1, depth, 1);
    }
    run_case(64, 2, 2);
    run_case(1, 64, 2);
}
