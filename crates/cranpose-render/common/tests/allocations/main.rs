#![cfg(feature = "embedded-default-font")]

use std::{alloc::System, cell::RefCell, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_render_common::{
    SceneUpdates,
    graph::{LayerNode, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, rebuild_graph_from_applier, update_graph_from_applier,
    },
};
use cranpose_ui::{
    Color, GraphicsLayer, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, Size, Text,
    TextStyle,
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

/// Allocations per pass of `pass` over the graph of a column of `cells`
/// tilted, filled and labelled cells, like the cells of the grid benchmark.
fn allocations_per_pass(
    cells: usize,
    mut pass: impl FnMut(&mut MemoryApplier, NodeId, &mut RenderGraph),
) -> usize {
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
    let mut graph = build_graph_from_applier(&applier, root, 1.0).expect("render graph");
    assert_eq!(cell_ids(&graph.root).len(), cells, "every cell is a layer");
    pass(&mut applier, root, &mut graph);
    pass(&mut applier, root, &mut graph);
    let region = Region::new(GLOBAL);
    for _ in 0..PASSES {
        pass(&mut applier, root, &mut graph);
    }
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations / PASSES
}

/// Allocations per cell of `pass`, past what a pass costs whatever the
/// number of cells.
fn allocations_per_cell(mut pass: impl FnMut(&mut MemoryApplier, NodeId, &mut RenderGraph)) -> f64 {
    let few = allocations_per_pass(10, &mut pass);
    let many = allocations_per_pass(110, &mut pass);
    (many - few) as f64 / 100.0
}

/// Rebuilding a layer takes the allocations of the layer it replaces, so a
/// cell rebuilt with the same content allocates nothing of its own.
fn rebuilding_a_cell_reuses_the_cell_it_replaces() {
    let per_cell = allocations_per_cell(|applier, _, graph| {
        let dirty = cell_ids(&graph.root);
        assert!(
            update_graph_from_applier(applier, graph, SceneUpdates::content(&dirty), 1.0),
            "the cells rebuild in place"
        );
    });
    println!("allocations per rebuilt cell: {per_cell}");
    assert!(per_cell <= 0.5, "{per_cell} allocations per rebuilt cell");
}

/// Rebuilding the whole graph takes the allocations of the graph it
/// replaces.
fn rebuilding_the_graph_reuses_the_graph_it_replaces() {
    let per_cell = allocations_per_cell(|applier, root, graph| {
        let previous = std::mem::replace(graph, RenderGraph::new(LayerNode::default()));
        *graph =
            rebuild_graph_from_applier(applier, root, 1.0, Some(previous)).expect("render graph");
    });
    println!("allocations per cell of a rebuilt graph: {per_cell}");
    assert!(
        per_cell <= 0.5,
        "{per_cell} allocations per cell of a rebuilt graph"
    );
}

const ROW_HEIGHT: f32 = 40.0;

/// Allocations of each scene update that scrolls a lazy column of filled,
/// labelled rows by one row, the scroll and its layout left out.
fn scroll_step_allocations() -> usize {
    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::default();
    let holder = Rc::clone(&state_holder);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let list_state = rememberLazyListState();
        *holder.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().size(Size::new(240.0, 320.0)),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(400, |index| {
                    Box(
                        Modifier::empty()
                            .size(Size::new(240.0, ROW_HEIGHT))
                            .background(Color::RED)
                            .rounded_corners(3.0),
                        BoxSpec::default(),
                        move || {
                            Text(
                                format!("row {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                });
            },
        );
    });
    let root = composition.root().expect("a composed root");
    let viewport = Size::new(240.0, 320.0);
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = build_graph_from_applier(&applier, root, 1.0).expect("render graph");
    let _ = applier.take_structural_change_parents_attached_to(root);
    applier.clear_runtime_handle();
    drop(applier);
    let list_state = (*state_holder.borrow()).expect("the list state");
    let mut step = || {
        assert_ne!(list_state.dispatch_scroll_delta(-ROW_HEIGHT), 0.0);
        let mut dirty = cranpose_ui::pending_layout_repass_nodes_snapshot();
        dirty.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier.compute_layout(root, viewport).expect("layout");
        dirty.extend(applier.take_structural_change_parents_attached_to(root));
        dirty.sort_unstable();
        dirty.dedup();
        let region = Region::new(GLOBAL);
        assert!(
            update_graph_from_applier(&applier, &mut graph, SceneUpdates::content(&dirty), 1.0),
            "the scroll updates the scene in place"
        );
        let allocations = region.change().allocations;
        applier.clear_runtime_handle();
        allocations
    };
    for _ in 0..3 {
        step();
    }
    (0..PASSES).map(|_| step()).sum::<usize>() / PASSES
}

/// A row scrolling in takes the allocations of the row that scrolled out:
/// what it allocates is its own draw recording.
fn a_row_scrolling_in_reuses_the_row_that_scrolled_out() {
    let allocations = scroll_step_allocations();
    println!("allocations per one-row scroll step: {allocations}");
    assert!(
        allocations <= 6,
        "{allocations} allocations per scroll step"
    );
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
#[test]
fn scene_rebuilds_stay_within_their_allocation_budgets() {
    rebuilding_a_cell_reuses_the_cell_it_replaces();
    rebuilding_the_graph_reuses_the_graph_it_replaces();
    a_row_scrolling_in_reuses_the_row_that_scrolled_out();
}
