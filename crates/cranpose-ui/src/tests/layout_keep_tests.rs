//! A layout pass keeps a node's measurement when only nodes below it changed
//! and every changed child measures the same for it. These tests change a
//! screen frame by frame and check each incremental layout against a fresh
//! composition of the same state.

use cranpose_core::{Composition, MemoryApplier, MutableState, NodeId, location_key};
use cranpose_foundation::lazy::LazyListScope;
use cranpose_macros::composable;
use cranpose_ui_layout::{
    Constraints, IntrinsicSize, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, Layout, LazyColumn, LazyColumnSpec, Modifier, Row,
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

/// A screen whose lazy list measures at the same constraints every frame,
/// so it keeps its measurement while its items change, and whose last
/// layout reads intrinsic sizes on some frames only.
#[composable]
fn KeptListScreen(tick: MutableState<usize>) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            let list = rememberLazyListState();
            LazyColumn(
                Modifier::empty().fill_max_width().height(160.0),
                list,
                LazyColumnSpec::default(),
                move |scope| {
                    scope.item(move || GrowingItem(tick));
                    scope.item(move || AlignedBox(tick));
                    scope.item(move || SplitItem(tick));
                    scope.items(20, move |item| SameSizeRow(tick, item));
                },
            );
            IntrinsicReaderLayout(tick);
        },
    );
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

/// A box whose only change is its child's alignment, on even frames: the
/// child keeps its size and moves.
#[composable]
fn AlignedBox(tick: MutableState<usize>) {
    let alignment = if (tick.get() / 2).is_multiple_of(2) {
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

/// Measures its one child at a width of 30, and takes the child's intrinsic
/// width as its own when `read` is set.
#[derive(Clone, PartialEq)]
struct IntrinsicReader {
    read: bool,
}

impl MeasurePolicy for IntrinsicReader {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[std::boxed::Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        let Some(child) = measurables.first() else {
            return MeasureResult::new(Size::new(0.0, 0.0), Vec::new());
        };
        let width = if self.read {
            child.max_intrinsic_width(f32::INFINITY)
        } else {
            30.0
        };
        let placeable = child.measure(Constraints {
            min_width: 30.0,
            max_width: 30.0,
            min_height: 0.0,
            max_height: f32::INFINITY,
        });
        MeasureResult::new(
            Size::new(width, placeable.height()),
            vec![Placement::new(placeable.node_id(), 0.0, 0.0, 0)],
        )
    }

    fn min_intrinsic_width(&self, _: &[std::boxed::Box<dyn Measurable>], _: f32) -> f32 {
        30.0
    }

    fn max_intrinsic_width(&self, _: &[std::boxed::Box<dyn Measurable>], _: f32) -> f32 {
        30.0
    }

    fn min_intrinsic_height(&self, _: &[std::boxed::Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _: &[std::boxed::Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }
}

/// A layout that reads its child's intrinsic width on even frames only,
/// while the box inside that child changes its width on odd frames. The
/// child measures the same at the width it is given, so it keeps its
/// measurement while its intrinsic width changes.
#[composable]
fn IntrinsicReaderLayout(tick: MutableState<usize>) {
    let frame = tick.get();
    let width = if frame.div_ceil(2).is_multiple_of(2) {
        50.0
    } else {
        80.0
    };
    Layout(
        Modifier::empty(),
        IntrinsicReader {
            read: frame.is_multiple_of(2),
        },
        move || WidthBox(width),
    );
}

/// A filling box around a box of `width`, which skips its body while the
/// width stays.
#[composable]
fn WidthBox(width: f32) {
    Box(
        Modifier::empty().fill_max_width().height(10.0),
        BoxSpec::default(),
        move || {
            Box(Modifier::empty().width(width), BoxSpec::default(), || {});
        },
    );
}

/// A lazy item whose height changes on odd frames.
#[composable]
fn GrowingItem(tick: MutableState<usize>) {
    let height = if tick.get().div_ceil(2).is_multiple_of(2) {
        4.0
    } else {
        10.0
    };
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty().fill_max_width().height(height),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A lazy item that emits a second root node on some frames.
#[composable]
fn SplitItem(tick: MutableState<usize>) {
    Text("split", Modifier::empty(), TextStyle::default());
    if tick.get().is_multiple_of(6) {
        Text("extra", Modifier::empty(), TextStyle::default());
    }
}

/// A row whose text changes every frame and keeps its width.
#[composable]
fn SameSizeRow(tick: MutableState<usize>, row: usize) {
    let label = format!("{:>4}", (tick.get() + row) % 1000);
    Row(
        Modifier::empty().fill_max_width().padding(2.0),
        RowSpec::default(),
        move || {
            Text(label.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}

/// Leaves inside a parent whose width moves every frame: a text that wraps
/// at some widths, a fixed box, a padded text and an offset box. Each keeps
/// its measurement while the width it gets leaves it unchanged, and
/// measures again where the text wraps differently.
#[composable]
fn ResizedLeavesScreen(tick: MutableState<usize>) {
    let width = 40.0 + (tick.get() % 13) as f32 * 23.0;
    Column(
        Modifier::empty().width(width),
        ColumnSpec::default(),
        move || {
            Text(
                "a few words that wrap at narrow widths",
                Modifier::empty(),
                TextStyle::default(),
            );
            Box(
                Modifier::empty().size_points(30.0, 12.0),
                BoxSpec::default(),
                || {},
            );
            Text(
                "padded",
                Modifier::empty().padding(3.0),
                TextStyle::default(),
            );
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                || {
                    Box(
                        Modifier::empty().offset(4.0, 2.0).size_points(8.0, 8.0),
                        BoxSpec::default(),
                        || {},
                    );
                    Text("tail", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

/// Every node's rect and content offset bits, in tree order.
fn layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<[u32; 6]> {
    fn flatten(node: &crate::LayoutBox, out: &mut Vec<[u32; 6]>) {
        out.push([
            node.rect.x.to_bits(),
            node.rect.y.to_bits(),
            node.rect.width.to_bits(),
            node.rect.height.to_bits(),
            node.content_offset.x.to_bits(),
            node.content_offset.y.to_bits(),
        ]);
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

/// Composes `screen` at `frame` and returns the composition and its root.
fn compose(
    screen: fn(MutableState<usize>),
    frame: usize,
) -> (Composition<MemoryApplier>, MutableState<usize>, NodeId) {
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(frame, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            screen(tick);
        })
        .expect("composition");
    let root = composition.root().expect("composition root");
    (composition, tick, root)
}

/// Changes `screen` frame by frame and checks each incremental layout
/// against a fresh composition of the same frame.
fn assert_incremental_layout_matches_fresh_compositions(screen: fn(MutableState<usize>)) {
    let _app_context = crate::render_state::app_context_test_scope();
    let (mut composition, tick, root) = compose(screen, 0);
    layout(&mut composition, root);

    for frame in 1..=48 {
        tick.set(frame);
        while composition.process_invalid_scopes().expect("recomposition") {}
        let incremental = layout(&mut composition, root);
        let (mut fresh, _, fresh_root) = compose(screen, frame);
        assert_eq!(
            incremental,
            layout(&mut fresh, fresh_root),
            "frame {frame}: the incremental layout differs from a fresh composition"
        );
    }
}

#[test]
fn incremental_layout_matches_a_fresh_composition() {
    assert_incremental_layout_matches_fresh_compositions(KeptScreen);
}

#[test]
fn incremental_lazy_list_layout_matches_a_fresh_composition() {
    assert_incremental_layout_matches_fresh_compositions(KeptListScreen);
}

#[test]
fn leaves_in_a_resized_parent_match_a_fresh_composition() {
    assert_incremental_layout_matches_fresh_compositions(ResizedLeavesScreen);
}
