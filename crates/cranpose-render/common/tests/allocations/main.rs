#![allow(clippy::print_stdout, reason = "a test prints what it measured")]

use std::{
    alloc::System,
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, MutableState, NodeId, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_render_common::{
    NodeUpdate,
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef},
};
use cranpose_ui::{
    Brush, Color, CornerRadii, GraphicsLayer, LayoutEngine, LayoutNode, LazyColumn, LazyColumnSpec,
    MeasureLayoutOptions, Modifier, Size, Text, TextStyle, measure_layout_with_options,
    widgets::{Box, BoxSpec, Column, ColumnSpec},
};
use cranpose_ui_graphics::{CommandRecording, Rect};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[macro_use]
#[path = "../../../../coroflow/tests/support/main_thread_test.rs"]
mod main_thread_test;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PASSES: usize = 10;

/// The ids of the records directly under the root of `graph`, the cells
/// of the column.
fn cell_ids(graph: &Scene) -> Vec<NodeId> {
    graph
        .root_layer()
        .into_iter()
        .flat_map(LayerRef::children)
        .filter_map(LayerRef::owner)
        .collect()
}

/// Allocations per pass of `pass` over the graph of a column of `cells`
/// tilted, filled and labelled cells, like the cells of the grid benchmark.
fn allocations_per_pass(
    cells: usize,
    mut pass: impl FnMut(&mut MemoryApplier, NodeId, &mut Scene),
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
    let mut graph = Scene::new();
    graph.rebuild_from_applier(&applier, root);
    assert!(graph.root_layer().is_some(), "render graph");
    assert_eq!(cell_ids(&graph).len(), cells, "every cell is a layer");
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
fn allocations_per_cell(mut pass: impl FnMut(&mut MemoryApplier, NodeId, &mut Scene)) -> f64 {
    let few = allocations_per_pass(10, &mut pass);
    let many = allocations_per_pass(110, &mut pass);
    (many - few) as f64 / 100.0
}

/// Rebuilding a layer takes the allocations of the layer it replaces, so a
/// cell rebuilt with the same content allocates nothing of its own.
fn rebuilding_a_cell_reuses_the_cell_it_replaces() {
    let per_cell = allocations_per_cell(|applier, root, graph| {
        let dirty: Vec<_> = cell_ids(graph)
            .into_iter()
            .map(|id| (id, NodeUpdate::Content))
            .collect();
        assert!(
            graph.update_from_applier(applier, root, &dirty),
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
        graph.rebuild_from_applier(applier, root);
    });
    println!("allocations per cell of a rebuilt graph: {per_cell}");
    assert!(
        per_cell <= 0.5,
        "{per_cell} allocations per cell of a rebuilt graph"
    );
}

/// A scene update keeps the map of its dirty nodes from the update before:
/// updating one cell allocates what the cell's rebuild allocates alone.
fn updating_one_cell_reuses_the_dirty_node_map() {
    let allocations = allocations_per_pass(10, |applier, root, graph| {
        let dirty = [(cell_ids(graph)[0], NodeUpdate::Content)];
        assert!(
            graph.update_from_applier(applier, root, &dirty),
            "the cell rebuilds in place"
        );
    });
    println!("allocations per one-cell update: {allocations}");
    assert!(
        allocations <= 2,
        "{allocations} allocations per one-cell update"
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
    let mut graph = Scene::new();
    graph.rebuild_from_applier(&applier, root);
    assert!(graph.root_layer().is_some(), "render graph");
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
        let updates: Vec<_> = dirty.iter().map(|&id| (id, NodeUpdate::Content)).collect();
        let region = Region::new(GLOBAL);
        assert!(
            graph.update_from_applier(&applier, root, &updates),
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

// Allocations a frame of `texts` texts that all change each frame:
// through four words, as a ticker's do, or to a new number, as a
// counter's do, when `numbers`: numbers of five digits, as a counter shows
// between the steps that lengthen it. Composition, the layout an app runs
// while no screen reader is connected and the scene update together.
fn ticker_frame_allocations(texts: usize, numbers: bool) -> usize {
    const WORDS: [&str; 4] = ["alpha", "bravo", "charlie", "delta"];
    const FRAME_LAYOUT: MeasureLayoutOptions = MeasureLayoutOptions {
        collect_semantics: false,
        build_layout_tree: false,
    };
    let tick_holder: Rc<RefCell<Option<MutableState<usize>>>> = Rc::default();
    let holder = Rc::clone(&tick_holder);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let tick = rememberMutableStateOf(|| 0usize);
        *holder.borrow_mut() = Some(tick);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for index in 0..texts {
                let style = TextStyle::default();
                if numbers {
                    Text(
                        format!("{}", 10_000 + tick.get() * 1000 + index),
                        Modifier::empty(),
                        style,
                    );
                } else {
                    Text(
                        WORDS[(tick.get() + index) % WORDS.len()],
                        Modifier::empty(),
                        style,
                    );
                }
            }
        });
    });
    let root = composition.root().expect("a composed root");
    let viewport = Size::new(400.0, 4000.0);
    let tick = (*tick_holder.borrow()).expect("the tick state");
    let mut graph = Scene::new();
    let mut updates = Vec::new();
    let mut frame = |count: usize| {
        tick.set(count);
        composition
            .process_invalid_scopes()
            .expect("the texts recompose");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout_with_options(&mut applier, root, viewport, FRAME_LAYOUT).expect("layout");
        if graph.root_layer().is_none() {
            graph.rebuild_from_applier(&applier, root);
        }
        updates.clear();
        applier
            .with_node::<LayoutNode, _>(root, |column| {
                let children = column.children().iter();
                updates.extend(children.map(|&id| (id, NodeUpdate::Content)));
            })
            .expect("the column");
        assert!(
            graph.update_from_applier(&applier, root, &updates),
            "the scene takes the update in place"
        );
        applier.clear_runtime_handle();
    };
    for count in 1..=8 {
        frame(count);
    }
    let region = Region::new(GLOBAL);
    for count in 9..9 + PASSES {
        frame(count);
    }
    region.change().allocations / PASSES
}

// A fixed text that shows another string each frame takes the room of a
// string and a layout it gave up before: it allocates nothing of its own.
// A counter's text allocates the string the app formats.
fn a_text_changing_each_frame_reuses_its_string_and_layout() {
    for (numbers, own) in [(false, 0.0), (true, 1.0)] {
        let few = ticker_frame_allocations(10, numbers);
        let many = ticker_frame_allocations(110, numbers);
        let per_text = many.saturating_sub(few) as f64 / 100.0;
        println!("allocations per changing text a frame, numbers {numbers}: {per_text}");
        assert!(
            per_text <= own + 0.1,
            "{per_text} allocations per changing text a frame, numbers {numbers}"
        );
    }
}

// The recordings the runs under `layer` draw from, as a reader holds them.
fn recordings(layer: LayerRef<'_>, held: &mut Vec<Rc<CommandRecording>>) {
    for item in layer.items() {
        match item {
            ItemRef::Run(run) => held.push(Rc::clone(&run.recording)),
            ItemRef::Layer(child) | ItemRef::Leaf(child) => recordings(child, held),
            ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Shape(_) => {}
        }
    }
}

// A node that draws behind its content and then around it, drawn again
// each update while a reader holds the recordings of the update before, as
// a picture does: each new recording takes the storage of one the reader
// let go of, so a steady update allocates nothing.
fn a_node_drawn_around_its_content_reuses_its_recordings() {
    let width = Rc::new(Cell::new(10.0f32));
    let drawn = Rc::clone(&width);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let drawn = Rc::clone(&drawn);
        Box(
            Modifier::empty()
                .size(Size::new(64.0, 16.0))
                .draw_behind(|scope| {
                    let bounds = Rect::from_size(scope.size());
                    scope.draw_round_rect_at(
                        bounds,
                        Brush::solid(Color::RED),
                        CornerRadii::uniform(4.0),
                    );
                })
                .draw_with_content(move |scope| {
                    scope.draw_content();
                    let bar = Rect::from_size(Size::new(drawn.get(), 16.0));
                    scope.draw_round_rect_at(
                        bar,
                        Brush::solid(Color::BLUE),
                        CornerRadii::uniform(4.0),
                    );
                }),
            BoxSpec::default(),
            || {},
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, Size::new(400.0, 400.0))
        .expect("layout");
    let mut graph = Scene::new();
    graph.rebuild_from_applier(&applier, root);
    let mut held = Vec::with_capacity(16);
    let update = |graph: &mut Scene, held: &mut Vec<Rc<CommandRecording>>| {
        width.set(width.get() + 1.0);
        assert!(
            graph.update_from_applier(&applier, root, &[(root, NodeUpdate::Draws)]),
            "the node draws again in place"
        );
        held.clear();
        recordings(graph.root_layer().expect("a scene"), held);
    };
    for _ in 0..6 {
        update(&mut graph, &mut held);
    }
    assert!(held.len() >= 2, "the node draws runs: {}", held.len());
    let region = Region::new(GLOBAL);
    for _ in 0..PASSES {
        update(&mut graph, &mut held);
    }
    let allocations = region.change().allocations;
    drop(held);
    applier.clear_runtime_handle();
    println!("allocations of {PASSES} redraws around content: {allocations}");
    assert_eq!(allocations, 0, "a redraw around content allocates");
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
fn scene_rebuilds_stay_within_their_allocation_budgets() {
    rebuilding_a_cell_reuses_the_cell_it_replaces();
    rebuilding_the_graph_reuses_the_graph_it_replaces();
    updating_one_cell_reuses_the_dirty_node_map();
    a_row_scrolling_in_reuses_the_row_that_scrolled_out();
    a_text_changing_each_frame_reuses_its_string_and_layout();
    a_node_drawn_around_its_content_reuses_its_recordings();
}

main_thread_test!(scene_rebuilds_stay_within_their_allocation_budgets);
