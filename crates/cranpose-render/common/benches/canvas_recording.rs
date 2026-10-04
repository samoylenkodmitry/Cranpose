use std::{alloc::System, cell::Cell, hint::black_box, rc::Rc, sync::Arc, time::Instant};

use cranpose_render_common::{
    SceneUpdates,
    graph::{DrawRunNode, LayerNode, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, rebuild_graph_from_applier,
        update_graph_from_applier_report,
    },
};
use cranpose_ui::{Canvas, LayoutEngine, Modifier, Size, run_test_composition};
use cranpose_ui_graphics::{Brush, Color, DrawScope, GraphicsLayer, Point, ShapeRecorder, Stroke};
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

fn emit_arcs(scope: &mut dyn DrawScope, arcs: usize, angle: f32) {
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
                emit_arcs(scope, arcs, angle);
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

struct PropertyModeResult {
    elapsed_ns: u128,
    allocations: usize,
    reallocations: usize,
    bytes_allocated: usize,
    bytes_deallocated: usize,
    callbacks: usize,
    fingerprint: u64,
    primitive_count: usize,
    rotation_bits: u32,
    transform_points: [u32; 4],
}

fn rotation_bits(layer: &LayerNode, expected: f32) -> Option<u32> {
    if layer.graphics_layer.rotation_z == expected {
        return Some(layer.graphics_layer.rotation_z.to_bits());
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) => rotation_bits(layer, expected),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    })
}

fn transform_points(layer: &LayerNode, node_id: cranpose_core::NodeId) -> Option<[u32; 4]> {
    if layer.node_id == Some(node_id) {
        let origin = layer.transform_to_parent.map_point(Point::default());
        let far_corner = layer
            .transform_to_parent
            .map_point(Point::new(256.0, 256.0));
        return Some([
            origin.x.to_bits(),
            origin.y.to_bits(),
            far_corner.x.to_bits(),
            far_corner.y.to_bits(),
        ]);
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(layer) => transform_points(layer, node_id),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    })
}

fn run_property_mode(
    arcs: usize,
    layers_only: bool,
    applier: &cranpose_core::MemoryApplier,
    canvas_node: cranpose_core::NodeId,
    angle: &Cell<f32>,
    draw_calls: &Cell<usize>,
    graph: &mut RenderGraph,
) -> PropertyModeResult {
    let mut update = |tick: usize| {
        let angle_value = 0.125 + tick as f32 * 0.37;
        angle.set(angle_value);
        let updates = if layers_only {
            SceneUpdates {
                content: &[],
                layers: std::slice::from_ref(&canvas_node),
            }
        } else {
            SceneUpdates::content(std::slice::from_ref(&canvas_node))
        };
        assert!(
            update_graph_from_applier_report(applier, graph, updates, 1.0).applied(),
            "scoped property benchmark update must be applied"
        );
    };
    for tick in 0..WARMUP {
        update(tick);
    }
    let callbacks_before = draw_calls.get();
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for tick in WARMUP..WARMUP + FRAMES {
        update(tick);
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let stats = region.change();
    let callbacks = draw_calls.get() - callbacks_before;
    let run = canvas_run(&graph.root).expect("property-updated Canvas");
    assert_eq!(run.recording.len(), arcs);
    let expected_rotation = 0.125 + (WARMUP + FRAMES - 1) as f32 * 0.37;
    let rotation_bits = rotation_bits(&graph.root, expected_rotation)
        .expect("graphics-layer rotation reaches the retained scene");
    let transform_points = transform_points(&graph.root, canvas_node)
        .expect("Canvas node transform reaches the retained scene");
    PropertyModeResult {
        elapsed_ns,
        allocations: stats.allocations,
        reallocations: stats.reallocations,
        bytes_allocated: stats.bytes_allocated,
        bytes_deallocated: stats.bytes_deallocated,
        callbacks,
        fingerprint: run.recording.fingerprint(),
        primitive_count: run.recording.len(),
        rotation_bits,
        transform_points,
    }
}

fn run_property_case(arcs: usize, layers_first: bool) {
    clear_command_recordings_for_tests();
    let angle = Rc::new(Cell::new(0.125f32));
    let draw_calls = Rc::new(Cell::new(0usize));
    let canvas_node = Rc::new(Cell::new(None));
    let layer_angle = Rc::clone(&angle);
    let canvas_draw_calls = Rc::clone(&draw_calls);
    let canvas_node_slot = Rc::clone(&canvas_node);
    let mut composition = run_test_composition(move || {
        let layer_angle = Rc::clone(&layer_angle);
        let canvas_draw_calls = Rc::clone(&canvas_draw_calls);
        let canvas_id = Canvas(
            Modifier::empty()
                .size(Size::new(256.0, 256.0))
                .graphics_layer(move || GraphicsLayer {
                    rotation_z: layer_angle.get(),
                    ..GraphicsLayer::default()
                }),
            move |scope| {
                canvas_draw_calls.set(canvas_draw_calls.get() + 1);
                emit_arcs(scope, arcs, 0.0);
            },
        );
        canvas_node_slot.set(Some(canvas_id));
    });
    let composition_root = composition.root().expect("composed Canvas root");
    let canvas_node = canvas_node.get().expect("Canvas node was composed");
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(composition_root, Size::new(256.0, 256.0))
        .expect("Canvas layout");
    let initial = build_graph_from_applier(&applier, composition_root, 1.0).expect("Canvas graph");
    let initial_transform =
        transform_points(&initial.root, canvas_node).expect("initial Canvas node transform");
    let mut content_graph = initial.clone();
    let mut layer_graph = initial;
    let initial_calls = draw_calls.get();
    let measure = |layers_only, graph: &mut RenderGraph| {
        run_property_mode(
            arcs,
            layers_only,
            &applier,
            canvas_node,
            &angle,
            &draw_calls,
            graph,
        )
    };
    let (content, layer) = if layers_first {
        let layer = measure(true, &mut layer_graph);
        (measure(false, &mut content_graph), layer)
    } else {
        let content = measure(false, &mut content_graph);
        (content, measure(true, &mut layer_graph))
    };
    let content_calls = content.callbacks;
    assert_eq!(content.rotation_bits, layer.rotation_bits);
    assert_eq!(content.transform_points, layer.transform_points);
    assert_ne!(initial_transform, content.transform_points);
    assert_eq!(content.fingerprint, layer.fingerprint);
    assert_eq!(content.primitive_count, arcs);
    assert_eq!(layer.primitive_count, arcs);
    assert_eq!(content_calls, FRAMES);
    assert_eq!(layer.callbacks, 0);
    assert_eq!(draw_calls.get(), initial_calls + WARMUP + FRAMES);
    applier.clear_runtime_handle();
    for (mode, result) in [("content", content), ("layers", layer)] {
        println!(
            "{{\"benchmark\":\"canvas_property_animation\",\"mode\":\"{mode}\",\"arcs\":{arcs},\"frames\":{FRAMES},\"canvas_callbacks\":{},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"ns_per_frame\":{},\"fingerprint\":{},\"primitive_count\":{},\"rotation_bits\":{},\"transform_points\":[{},{},{},{}]}}",
            result.callbacks,
            result.allocations,
            result.reallocations,
            result.bytes_allocated,
            result.bytes_deallocated,
            result.elapsed_ns / FRAMES as u128,
            result.fingerprint,
            result.primitive_count,
            result.rotation_bits,
            result.transform_points[0],
            result.transform_points[1],
            result.transform_points[2],
            result.transform_points[3],
        );
    }
}

fn main() {
    let layers_first = std::env::args().any(|arg| arg == "--layers-first");
    for arcs in [32, 1024, 17_000] {
        for readers in [0, 1, 2, 4] {
            run_case(arcs, readers);
        }
        run_property_case(arcs, layers_first);
    }
}
