#![cfg(feature = "embedded-default-font")]

use std::alloc::System;

use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{LayerNode, RenderNode},
    scene_builder::{build_graph_from_applier, update_graph_from_applier},
};
use cranpose_ui::{
    Color, GraphicsLayer, LayoutEngine, Modifier, Size, Text, TextStyle,
    widgets::{Box, BoxSpec, Column, ColumnSpec},
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PASSES: usize = 10;

/// The ids of the layers directly under `layer`, the cells of the column.
fn cell_ids(layer: &LayerNode) -> Vec<NodeId> {
    layer
        .children
        .iter()
        .filter_map(|child| match child {
            RenderNode::Layer(cell) => cell.node_id.or(cell.wraps),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
        })
        .collect()
}

/// Allocations per pass of rebuilding every cell of a column of `cells`
/// tilted, filled and labelled cells, like the cells of the grid benchmark.
fn rebuild_allocations(cells: usize) -> usize {
    let mut composition = cranpose_ui::run_test_composition(move || {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for index in 0..cells {
                Box(
                    Modifier::empty()
                        .size(Size::new(40.0, 12.0))
                        .padding(1.0)
                        .graphics_layer_value(GraphicsLayer {
                            rotation_z: 2.0,
                            ..Default::default()
                        })
                        .background(Color::RED)
                        .rounded_corners(3.0),
                    BoxSpec::default(),
                    move || {
                        Text(
                            format!("cell {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            }
        });
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, Size::new(400.0, 4000.0))
        .expect("layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("render graph");
    let dirty = cell_ids(&graph.root);
    assert_eq!(dirty.len(), cells, "every cell is a layer of the column");
    let mut rebuild = |graph: &mut _| {
        assert!(
            update_graph_from_applier(&mut applier, graph, &dirty, 1.0),
            "the cells rebuild in place"
        );
    };
    rebuild(&mut graph);
    rebuild(&mut graph);
    let region = Region::new(GLOBAL);
    for _ in 0..PASSES {
        rebuild(&mut graph);
    }
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations / PASSES
}

/// Rebuilding a layer takes the allocations of the layer it replaces, so a
/// cell rebuilt with the same content allocates nothing of its own.
fn rebuilding_a_cell_reuses_the_cell_it_replaces() {
    let few = rebuild_allocations(10);
    let many = rebuild_allocations(110);
    let per_cell = (many - few) as f64 / 100.0;
    println!("allocations per rebuilt cell: {per_cell}");
    assert!(per_cell <= 0.5, "{per_cell} allocations per rebuilt cell");
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
#[test]
fn scene_rebuilds_stay_within_their_allocation_budgets() {
    rebuilding_a_cell_reuses_the_cell_it_replaces();
}
