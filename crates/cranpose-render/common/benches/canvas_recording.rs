use std::{alloc::System, cell::Cell, hint::black_box, rc::Rc, sync::Arc, time::Instant};

use cranpose_render_common::{
    graph::{DrawRunNode, LayerNode, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, rebuild_graph_from_applier,
    },
};
use cranpose_ui::{Canvas, LayoutEngine, Modifier, Size, run_test_composition};
use cranpose_ui_graphics::{Brush, Color, Point, ShapeRecorder, Stroke};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const WARMUP: usize = 32;
const FRAMES: usize = 240;

fn canvas_run(layer: &LayerNode) -> Option<&DrawRunNode> {
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) => canvas_run(layer),
        RenderNode::DrawRun(run) => Some(run),
        RenderNode::Primitive(_) => None,
    })
}

fn run_case(arcs: usize, readers: usize) {
    clear_command_recordings_for_tests();
    let angle = Rc::new(Cell::new(0.0f32));
    let draw_angle = Rc::clone(&angle);
    let mut composition = run_test_composition(move || {
        let draw_angle = Rc::clone(&draw_angle);
        Canvas(
            Modifier::empty().size(Size::new(256.0, 256.0)),
            move |scope| {
                let angle = draw_angle.get();
                for index in 0..arcs {
                    scope.draw_arc(
                        Brush::solid(Color::BLUE),
                        Point::new(128.0, 128.0),
                        40.0 + (index % 64) as f32 * 0.5,
                        angle + (index / 64) as f32 * 0.03,
                        0.0125,
                        Stroke::new(1.0),
                    );
                }
            },
        );
    });
    let root = composition.root().expect("composed Canvas root");
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, Size::new(256.0, 256.0))
        .expect("Canvas layout");
    let mut graph = build_graph_from_applier(&applier, root, 1.0).expect("Canvas graph");
    let initial_fingerprint = canvas_run(&graph.root)
        .expect("initial Canvas")
        .recording
        .fingerprint();
    let mut retained: [Option<Arc<ShapeRecorder>>; 4] = std::array::from_fn(|_| None);
    let mut frame = |tick: usize, graph: &mut RenderGraph| {
        angle.set(tick as f32 * 0.005);
        let previous = std::mem::replace(
            graph,
            RenderGraph {
                root: LayerNode::default(),
            },
        );
        *graph =
            rebuild_graph_from_applier(&applier, root, 1.0, Some(previous)).expect("redraw Canvas");
        let run = canvas_run(&graph.root).expect("recorded Canvas");
        assert_eq!(run.recording.len(), arcs);
        if readers != 0 {
            retained[tick % readers] = Some(Arc::clone(run.recording.shape_recorder()));
        }
        black_box(run);
    };
    for tick in 0..WARMUP {
        frame(tick, &mut graph);
    }
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for tick in WARMUP..WARMUP + FRAMES {
        frame(tick, &mut graph);
    }
    let elapsed = started.elapsed().as_nanos();
    let stats = region.change();
    let fingerprint = canvas_run(&graph.root)
        .expect("last Canvas")
        .recording
        .fingerprint();
    assert_ne!(
        initial_fingerprint, fingerprint,
        "Canvas angle must advance"
    );
    applier.clear_runtime_handle();
    println!(
        "{{\"benchmark\":\"canvas_recording\",\"arcs\":{arcs},\"retained_generations\":{readers},\"frames\":{FRAMES},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"ns_per_frame\":{},\"fingerprint\":{fingerprint}}}",
        stats.allocations,
        stats.reallocations,
        stats.bytes_allocated,
        stats.bytes_deallocated,
        elapsed / FRAMES as u128,
    );
}

fn main() {
    for arcs in [32, 1024, 17_000] {
        for readers in [0, 1, 2, 4] {
            run_case(arcs, readers);
        }
    }
}
