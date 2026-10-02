use std::{alloc::System, hint::black_box, time::Instant};

use cranpose_render_common::{
    graph::{LayerNode, RenderGraph},
    scene_builder::{build_graph_from_applier, rebuild_graph_from_applier},
};
use cranpose_ui::{
    Color, GraphicsLayer, LayoutEngine, Modifier, Size, Text, TextStyle,
    widgets::{Box, BoxSpec, Column, ColumnSpec},
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn main() {
    let mut args = std::env::args().skip(1);
    let cells = args
        .next()
        .map_or(2400, |value| value.parse().expect("cell count"));
    let explicit_stride = args
        .next()
        .map_or(0, |value| value.parse().expect("layer stride"));
    let passes = args
        .next()
        .map_or(200, |value| value.parse().expect("pass count"));
    let mut composition = cranpose_ui::run_test_composition(move || {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for index in 0..cells {
                let mut modifier = Modifier::empty().size(Size::new(160.0, 20.0));
                if explicit_stride != 0 && index % explicit_stride == 0 {
                    modifier = modifier.graphics_layer_value(GraphicsLayer {
                        rotation_z: 2.0,
                        ..Default::default()
                    });
                }
                Box(
                    modifier.background(Color::RED),
                    BoxSpec::default(),
                    move || {
                        Text(
                            format!("quote {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            }
        });
    });
    let root = composition.root().expect("composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, Size::new(400.0, cells as f32 * 24.0))
        .expect("layout");
    let region = Region::new(GLOBAL);
    let mut graph = build_graph_from_applier(&applier, root, 1.0).expect("scene");
    let creation = region.change();
    let rebuild = |graph: &mut RenderGraph| {
        let previous = std::mem::replace(
            graph,
            RenderGraph {
                root: LayerNode::default(),
            },
        );
        *graph =
            rebuild_graph_from_applier(&applier, root, 1.0, Some(previous)).expect("rebuilt scene");
        black_box(&*graph);
    };
    for _ in 0..10 {
        rebuild(&mut graph);
    }
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for _ in 0..passes {
        rebuild(&mut graph);
    }
    let elapsed = started.elapsed();
    let updates = region.change();
    println!(
        "cells={cells} explicit_stride={explicit_stride} nodes={} graph_heap_bytes={} retained_scene_bytes={} creation_allocations={} rebuild_allocations={} rebuild_allocated_bytes={} rebuild_us={:.3}",
        graph.node_count(),
        graph.heap_bytes(),
        creation.bytes_allocated as isize - creation.bytes_deallocated as isize,
        creation.allocations,
        updates.allocations as f64 / passes as f64,
        updates.bytes_allocated as f64 / passes as f64,
        elapsed.as_secs_f64() * 1_000_000.0 / passes as f64,
    );
    applier.clear_runtime_handle();
}
