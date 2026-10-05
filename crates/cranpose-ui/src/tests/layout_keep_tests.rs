//! A layout pass keeps a node's measurement when only nodes below it changed
//! and every changed child measures the same for it. These tests change a
//! screen frame by frame and check each incremental layout against a layout
//! of the same state from scratch.

use cranpose_core::{Applier, Composition, MemoryApplier, MutableState, NodeId, location_key};
use cranpose_foundation::lazy::LazyListScope;
use cranpose_macros::composable;
use cranpose_ui_layout::IntrinsicSize;

use crate::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier, Row,
    RowSpec, Size, Text, TextStyle,
    layout::{MeasureLayoutOptions, build_layout_tree_from_applier},
    measure_layout_with_options, rememberLazyListState,
    widgets::BoxWithConstraints,
};

#[composable]
fn KeptScreen(tick: MutableState<usize>) {
    BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                WeightRow(tick);
                AlignedBox(tick);
                IntrinsicColumn(tick);
                for row in 0..5 {
                    KeptRow(tick, row);
                }
                let list = rememberLazyListState();
                LazyColumn(
                    Modifier::empty().fill_max_width().height(120.0),
                    list,
                    LazyColumnSpec::default(),
                    move |scope| {
                        scope.items(20, move |item| KeptRow(tick, item + 10));
                    },
                );
            },
        );
    });
}

/// A row whose texts keep their width on most frames and change it on
/// others, whose weighted box changes its weight, and whose last text
/// changes its padding.
#[composable]
fn KeptRow(tick: MutableState<usize>, row: usize) {
    let value = tick.get() + row;
    let label = if value.is_multiple_of(4) {
        format!("{:>4}!", value % 1000)
    } else {
        format!("{:>4}", value % 1000)
    };
    let weight = if value.is_multiple_of(7) { 2.0 } else { 1.0 };
    let height = 4.0 + (value % 3) as f32;
    let padding = (value % 2) as f32;
    Row(
        Modifier::empty().fill_max_width().padding(2.0),
        RowSpec::default(),
        move || {
            Text(label.clone(), Modifier::empty(), TextStyle::default());
            Box(
                Modifier::empty().weight(weight).height(height),
                BoxSpec::default(),
                || {},
            );
            Text(
                format!("{}", value % 10),
                Modifier::empty().padding(padding),
                TextStyle::default(),
            );
        },
    );
}

/// A row whose only change is the weight of its first box: the boxes keep
/// their sizes at the constraints they had, so only their parent data says
/// the row has to share its width again.
#[composable]
fn WeightRow(tick: MutableState<usize>) {
    let weight = if tick.get().is_multiple_of(2) {
        1.0
    } else {
        3.0
    };
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::default(),
        move || {
            Box(
                Modifier::empty().weight(weight).height(6.0),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty().weight(1.0).height(6.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A box whose only change is its child's alignment: the child keeps its
/// size and moves.
#[composable]
fn AlignedBox(tick: MutableState<usize>) {
    let alignment = if tick.get().is_multiple_of(2) {
        Alignment::TOP_START
    } else {
        Alignment::TOP_END
    };
    Box(
        Modifier::empty().fill_max_width().height(10.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty().align(alignment).width(4.0).height(4.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A column as wide as its widest child's intrinsic width, whose filling
/// child keeps its size while the text inside it changes that width.
#[composable]
fn IntrinsicColumn(tick: MutableState<usize>) {
    let label = if tick.get().is_multiple_of(2) {
        "ab"
    } else {
        "abcdef"
    };
    Column(
        Modifier::empty().width_intrinsic(IntrinsicSize::Max),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty().fill_max_width(),
                BoxSpec::default(),
                move || {
                    Text(label, Modifier::empty(), TextStyle::default());
                },
            );
            Box(
                Modifier::empty().fill_max_width().height(2.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// Every node's id and its rect and content offset bits, in tree order.
fn layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<(NodeId, [u32; 6])> {
    fn flatten(node: &crate::LayoutBox, out: &mut Vec<(NodeId, [u32; 6])>) {
        out.push((
            node.node_id,
            [
                node.rect.x.to_bits(),
                node.rect.y.to_bits(),
                node.rect.width.to_bits(),
                node.rect.height.to_bits(),
                node.content_offset.x.to_bits(),
                node.content_offset.y.to_bits(),
            ],
        ));
        for child in &node.children {
            flatten(child, out);
        }
    }

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout_with_options(
        &mut applier,
        root,
        Size::new(400.0, 800.0),
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: false,
        },
    )
    .expect("layout pass");
    let tree = build_layout_tree_from_applier(&mut applier, root)
        .expect("live layout tree")
        .expect("composition has a root");
    applier.clear_runtime_handle();
    let mut nodes = Vec::new();
    flatten(tree.root(), &mut nodes);
    nodes
}

/// Lays the same state out again with every cached measurement stale.
fn layout_from_scratch(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
) -> Vec<(NodeId, [u32; 6])> {
    crate::render_state::invalidate_layout_cache_epoch();
    let mut applier = composition.applier_mut();
    applier
        .get_mut(root)
        .expect("root node")
        .mark_needs_measure();
    drop(applier);
    layout(composition, root)
}

#[test]
fn incremental_layout_matches_a_layout_from_scratch() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(0usize, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            KeptScreen(tick);
        })
        .expect("initial composition");
    let root = composition.root().expect("composition root");
    layout(&mut composition, root);

    for frame in 1..=48 {
        tick.set(frame);
        while composition.process_invalid_scopes().expect("recomposition") {}
        let incremental = layout(&mut composition, root);
        let from_scratch = layout_from_scratch(&mut composition, root);
        assert_eq!(
            incremental, from_scratch,
            "frame {frame}: the incremental layout differs from a layout from scratch"
        );
    }
}
