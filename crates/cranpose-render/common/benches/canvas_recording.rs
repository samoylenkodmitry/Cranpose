use std::{alloc::System, cell::Cell, hint::black_box, rc::Rc, sync::Arc, time::Instant};

use cranpose_render_common::{
    SceneUpdates,
    graph::{DrawRunNode, LayerNode, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, rebuild_graph_from_applier,
        update_graph_from_applier_report,
    },
    style_shared::DrawPlacement,
};
use cranpose_ui::{Box, BoxSpec, Canvas, LayoutEngine, Modifier, Size, run_test_composition};
use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, DrawPrimitive, DrawScope, DrawScopeDefault, DrawTextMeasurer,
    DrawTextStyle, EdgeInsets, GraphicsLayer, Point, ShapeRecorder, Stroke, TextMeasurement,
    estimate_text_measurement,
};
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
    emit_arcs_at(scope, arcs, angle, Point::new(128.0, 128.0));
}

fn emit_arcs_at(scope: &mut dyn DrawScope, arcs: usize, angle: f32, center: Point) {
    emit_arcs_range(scope, 0, arcs, angle, center);
}

fn emit_arcs_range(
    scope: &mut dyn DrawScope,
    first_arc: usize,
    arcs: usize,
    angle: f32,
    center: Point,
) {
    for index in first_arc..first_arc + arcs {
        scope.draw_arc(
            Brush::solid(Color::BLUE),
            center,
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

struct BenchmarkTextMeasurer;

impl DrawTextMeasurer for BenchmarkTextMeasurer {
    fn measure_text(&self, text: &str, style: &DrawTextStyle) -> TextMeasurement {
        estimate_text_measurement(text, style)
    }
}

#[derive(Clone, Copy)]
enum ArcRecordingPath {
    Direct,
    Inset,
}

struct InsetBenchmarkResult {
    elapsed_ns: u128,
    allocations: usize,
    reallocations: usize,
    bytes_allocated: usize,
    bytes_deallocated: usize,
    fingerprint: u64,
    primitive_count: usize,
    recording: CommandRecording,
}

#[derive(Debug, PartialEq)]
struct ArcGeometry {
    center: Point,
    radius: f32,
    start_angle: f32,
    sweep_angle: f32,
    brush: Brush,
    stroke: Option<Stroke>,
    inner_radius: f32,
}

fn arc_geometry(recording: &CommandRecording) -> Vec<ArcGeometry> {
    recording
        .primitives_with_markers()
        .map(|primitive| match primitive {
            DrawPrimitive::Arc {
                center,
                radius,
                start_angle,
                sweep_angle,
                brush,
                stroke,
                inner_radius,
                ..
            } => ArcGeometry {
                center,
                radius,
                start_angle,
                sweep_angle,
                brush,
                stroke,
                inner_radius,
            },
            other => panic!("arc benchmark recorded an unexpected primitive: {other:?}"),
        })
        .collect()
}

fn record_arc_frame(
    storage: CommandRecording,
    measurer: &Rc<dyn DrawTextMeasurer>,
    path: ArcRecordingPath,
    arcs: usize,
    angle: f32,
) -> CommandRecording {
    let mut scope = DrawScopeDefault::with_text_measurer_reusing(
        Size::new(256.0, 256.0),
        Rc::clone(measurer),
        storage,
    );
    match path {
        ArcRecordingPath::Direct => emit_arcs(&mut scope, arcs, angle),
        ArcRecordingPath::Inset => scope.inset(EdgeInsets::uniform(8.0), |inner| {
            emit_arcs_at(inner, arcs, angle, Point::new(120.0, 120.0));
        }),
    }
    scope.finish()
}

fn run_inset_recording_case(
    arcs: usize,
    path: ArcRecordingPath,
    measurer: &Rc<dyn DrawTextMeasurer>,
) -> InsetBenchmarkResult {
    let mut recording = Some(CommandRecording::default());
    let mut frame = |tick: usize| {
        let next = record_arc_frame(
            recording.take().expect("previous frame storage"),
            measurer,
            path,
            arcs,
            tick as f32 * 0.005,
        );
        assert_eq!(next.len(), arcs);
        black_box(&next);
        recording = Some(next);
    };
    for tick in 0..WARMUP {
        frame(tick);
    }
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for tick in WARMUP..WARMUP + FRAMES {
        frame(tick);
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let stats = region.change();
    let recording = recording.expect("last frame recording");
    let fingerprint = recording.fingerprint();
    let primitive_count = recording.len();
    InsetBenchmarkResult {
        elapsed_ns,
        allocations: stats.allocations,
        reallocations: stats.reallocations,
        bytes_allocated: stats.bytes_allocated,
        bytes_deallocated: stats.bytes_deallocated,
        fingerprint,
        primitive_count,
        recording,
    }
}

fn run_inset_recording_benchmark(arcs: usize) {
    let measurer: Rc<dyn DrawTextMeasurer> = Rc::new(BenchmarkTextMeasurer);
    let direct = run_inset_recording_case(arcs, ArcRecordingPath::Direct, &measurer);
    let inset = run_inset_recording_case(arcs, ArcRecordingPath::Inset, &measurer);
    assert_eq!(direct.primitive_count, arcs);
    assert_eq!(inset.primitive_count, arcs);
    assert_eq!(
        arc_geometry(&direct.recording),
        arc_geometry(&inset.recording),
        "inset translation preserves each arc's intended geometry and style"
    );
    for (path, result) in [("direct", direct), ("inset", inset)] {
        println!(
            "{{\"benchmark\":\"canvas_inset_recording\",\"path\":\"{path}\",\"arcs\":{arcs},\"frames\":{FRAMES},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"ns_per_frame\":{},\"fingerprint\":{},\"primitive_count\":{}}}",
            result.allocations,
            result.reallocations,
            result.bytes_allocated,
            result.bytes_deallocated,
            result.elapsed_ns / FRAMES as u128,
            result.fingerprint,
            result.primitive_count,
        );
    }
}

fn collect_runs_for_node<'a>(
    layer: &'a LayerNode,
    node_id: cranpose_core::NodeId,
    runs: &mut Vec<&'a DrawRunNode>,
) {
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => collect_runs_for_node(child, node_id, runs),
            RenderNode::DrawRun(run) if run.command.is_some_and(|id| id.node_id == node_id) => {
                runs.push(run);
            }
            RenderNode::DrawRun(_) | RenderNode::Primitive(_) => {}
        }
    }
}

fn run_with_content_case(arcs: usize) {
    clear_command_recordings_for_tests();
    let callback_calls = Rc::new(Cell::new(0usize));
    let angle = Rc::new(Cell::new(0.0f32));
    let parent_slot = Rc::new(Cell::new(None));
    let content_slot = Rc::new(Cell::new(None));
    let draw_calls = Rc::clone(&callback_calls);
    let draw_angle = Rc::clone(&angle);
    let parent_id = Rc::clone(&parent_slot);
    let content_id = Rc::clone(&content_slot);
    let mut composition = run_test_composition(move || {
        let draw_calls = Rc::clone(&draw_calls);
        let draw_angle = Rc::clone(&draw_angle);
        let parent_id = Rc::clone(&parent_id);
        let content_id = Rc::clone(&content_id);
        let parent = Box(
            Modifier::empty()
                .size(Size::new(256.0, 256.0))
                .draw_with_content(move |scope| {
                    draw_calls.set(draw_calls.get() + 1);
                    let angle = draw_angle.get();
                    let before = arcs / 2;
                    emit_arcs_range(scope, 0, before, angle, Point::new(128.0, 128.0));
                    scope.draw_content();
                    emit_arcs_range(
                        scope,
                        before,
                        arcs - before,
                        angle,
                        Point::new(128.0, 128.0),
                    );
                }),
            BoxSpec::default(),
            move || {
                let content = Canvas(Modifier::empty().size(Size::new(64.0, 64.0)), |scope| {
                    scope.draw_rect(Brush::solid(Color::RED));
                });
                content_id.set(Some(content));
            },
        );
        parent_id.set(Some(parent));
    });
    let root = composition.root().expect("with-content root");
    let parent = parent_slot.get().expect("with-content parent");
    let content = content_slot.get().expect("with-content child");
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, Size::new(256.0, 256.0))
        .expect("with-content layout");
    let mut graph = build_graph_from_applier(&applier, root, 1.0).expect("with-content graph");
    let initial_runs = {
        let mut runs = Vec::new();
        collect_runs_for_node(&graph.root, parent, &mut runs);
        runs
    };
    assert_eq!(
        initial_runs.len(),
        2,
        "callback output has behind and overlay runs"
    );
    assert!(
        initial_runs
            .iter()
            .all(|run| run.recording.content_markers() == 1)
    );
    let initial_fingerprint = initial_runs[0].recording.fingerprint();
    assert_eq!(initial_runs[1].recording.fingerprint(), initial_fingerprint);
    let mut content_runs = Vec::new();
    collect_runs_for_node(&graph.root, content, &mut content_runs);
    assert_eq!(
        content_runs.len(),
        1,
        "the marker has visible child content"
    );
    assert!(matches!(
        content_runs[0].primitives().next(),
        Some(DrawPrimitive::Rect {
            brush: Brush::Solid(color),
            ..
        }) if color == Color::RED
    ));

    let update = |tick: usize, graph: &mut RenderGraph| {
        angle.set(tick as f32 * 0.005);
        assert!(
            update_graph_from_applier_report(
                &applier,
                graph,
                SceneUpdates::content(std::slice::from_ref(&parent)),
                1.0,
            )
            .applied(),
            "content update is applied"
        );
    };
    for tick in 0..WARMUP {
        update(tick, &mut graph);
    }
    let callbacks_before = callback_calls.get();
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for tick in WARMUP..WARMUP + FRAMES {
        update(tick, &mut graph);
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let stats = region.change();
    let callbacks = callback_calls.get() - callbacks_before;
    let mut runs = Vec::new();
    collect_runs_for_node(&graph.root, parent, &mut runs);
    assert_eq!(runs.len(), 2);
    let behind = runs
        .iter()
        .find(|run| {
            run.command
                .is_some_and(|id| id.placement == DrawPlacement::Behind)
        })
        .expect("before-content arc run");
    let overlay = runs
        .iter()
        .find(|run| {
            run.command
                .is_some_and(|id| id.placement == DrawPlacement::Overlay)
        })
        .expect("after-content arc run");
    assert_eq!(
        behind.recording.fingerprint(),
        overlay.recording.fingerprint()
    );
    assert_eq!(behind.recording.content_markers(), 1);
    assert_eq!(
        behind.recording.len_in(&behind.segments) + overlay.recording.len_in(&overlay.segments),
        arcs
    );
    let fingerprint = behind.recording.fingerprint();
    assert_ne!(
        initial_fingerprint, fingerprint,
        "updated arcs change the public recording"
    );
    applier.clear_runtime_handle();
    println!(
        "{{\"benchmark\":\"canvas_with_content\",\"arcs\":{arcs},\"frames\":{FRAMES},\"canvas_callbacks\":{callbacks},\"callbacks_per_frame\":{},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"ns_per_frame\":{},\"fingerprint\":{fingerprint},\"primitive_count\":{arcs},\"content_markers\":1}}",
        callbacks as f64 / FRAMES as f64,
        stats.allocations,
        stats.reallocations,
        stats.bytes_allocated,
        stats.bytes_deallocated,
        elapsed_ns / FRAMES as u128,
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
    if std::env::args().any(|arg| arg == "--with-content-only") {
        for arcs in [32, 1024, 17_000] {
            run_with_content_case(arcs);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--inset-only") {
        for arcs in [32, 1024, 17_000] {
            run_inset_recording_benchmark(arcs);
        }
        return;
    }
    let layers_first = std::env::args().any(|arg| arg == "--layers-first");
    for arcs in [32, 1024, 17_000] {
        for readers in [0, 1, 2, 4] {
            run_case(arcs, readers);
        }
        run_property_case(arcs, layers_first);
    }
}
