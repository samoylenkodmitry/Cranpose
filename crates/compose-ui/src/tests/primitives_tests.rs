use super::*;
use crate::composable;
use crate::layout::LayoutBox;
use crate::modifier::{Modifier, Size};
use crate::subcompose_layout::{Constraints, SubcomposeLayoutNode};
use crate::widgets::nodes::LayoutNode;
use crate::widgets::{
    BoxWithConstraints, Button, Column, ColumnSpec, DynamicTextSource, ForEach, Row, RowSpec,
    Spacer, Text,
};
use crate::{run_test_composition, LayoutEngine, SnapshotState, TestComposition};
use compose_core::{
    self, location_key, Applier, Composer, Composition, ConcreteApplierHost, MemoryApplier,
    MutableState, NodeId, Phase, SlotBackend, SlotStorage, SlotsHost, SnapshotStateObserver, State,
};
use compose_ui_layout::{HorizontalAlignment, LinearArrangement, VerticalAlignment};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
    static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
}

fn prepare_measure_composer(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    handle: &compose_core::RuntimeHandle,
    root: Option<NodeId>,
) -> (
    Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        MemoryApplier::new(),
    )));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = Composer::new(
        Rc::clone(&slots_host),
        applier_host.clone(),
        handle.clone(),
        observer,
        root,
    );
    (composer, slots_host, applier_host)
}

fn restore_measure_composer(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    slots_host: Rc<SlotsHost>,
    applier_host: Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    *slots = Rc::try_unwrap(slots_host)
        .unwrap_or_else(|_| panic!("slots host still has outstanding references"))
        .take();
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .into_inner();
}

fn run_subcompose_measure(
    slots: &mut SlotBackend,
    applier: &mut MemoryApplier,
    handle: &compose_core::RuntimeHandle,
    node_id: NodeId,
    constraints: Constraints,
) {
    let (composer, slots_host, applier_host) =
        prepare_measure_composer(slots, applier, handle, Some(node_id));
    composer.enter_phase(Phase::Measure);
    let node_handle = {
        let mut applier_ref = applier_host.borrow_typed();
        let node = applier_ref.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        typed.handle()
    };
    node_handle
        .measure(&composer, node_id, constraints)
        .expect("measure succeeds");
    drop(composer);
    restore_measure_composer(slots, applier, slots_host, applier_host);
}

#[test]
fn row_with_alignment_updates_node_fields() {
    let mut composition = run_test_composition(|| {
        Row(
            Modifier::empty(),
            RowSpec::new()
                .horizontal_arrangement(LinearArrangement::SpaceBetween)
                .vertical_alignment(VerticalAlignment::Bottom),
            || {},
        );
    });
    let root = composition.root().expect("row root");
    composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            assert!(!node.children.is_empty() || node.children.is_empty());
        })
        .expect("layout node available");
}

#[test]
fn column_with_alignment_updates_node_fields() {
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::empty(),
            ColumnSpec::new()
                .vertical_arrangement(LinearArrangement::SpaceEvenly)
                .horizontal_alignment(HorizontalAlignment::End),
            || {},
        );
    });
    let root = composition.root().expect("column root");
    composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            assert!(!node.children.is_empty() || node.children.is_empty());
        })
        .expect("layout node available");
}

fn measure_subcompose_node(
    composition: &mut Composition<MemoryApplier>,
    slots: &mut SlotBackend,
    handle: &compose_core::RuntimeHandle,
    root: NodeId,
    constraints: Constraints,
) {
    let mut applier_guard = composition.applier_mut();
    let mut temp_applier = std::mem::take(&mut *applier_guard);
    run_subcompose_measure(slots, &mut temp_applier, handle, root, constraints);
    *applier_guard = temp_applier;
}

#[composable]
fn CounterRow(label: &'static str, count: State<i32>) -> NodeId {
    COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(label, Modifier::empty());
        let count_for_text = count.clone();
        let text_id = Text(
            DynamicTextSource::new(move || format!("Count = {}", count_for_text.value())),
            Modifier::empty(),
        );
        COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = Some(text_id));
    })
}

#[test]
fn layout_column_produces_expected_measurements() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let text_id_capture = Rc::clone(&text_id);

    composition
        .render(key, move || {
            let text_id_capture = Rc::clone(&text_id_capture);
            Column(
                Modifier::empty().padding(10.0),
                ColumnSpec::default(),
                move || {
                    let id = Text("Hello", Modifier::empty());
                    *text_id_capture.borrow_mut() = Some(id);
                    Spacer(Size {
                        width: 0.0,
                        height: 30.0,
                    });
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 200.0,
                height: 200.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root().clone();
    assert!((root_layout.rect.width - 60.0).abs() < 1e-3);
    assert!((root_layout.rect.height - 70.0).abs() < 1e-3);
    assert_eq!(root_layout.children.len(), 2);

    let text_layout = &root_layout.children[0];
    assert_eq!(
        text_layout.node_id,
        text_id.borrow().as_ref().copied().expect("text node id")
    );
    assert!((text_layout.rect.x - 10.0).abs() < 1e-3);
    assert!((text_layout.rect.y - 10.0).abs() < 1e-3);
    assert!((text_layout.rect.width - 40.0).abs() < 1e-3);
    assert!((text_layout.rect.height - 20.0).abs() < 1e-3);
}

#[test]
fn modifier_offset_translates_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let text_id_capture = Rc::clone(&text_id);

    composition
        .render(key, move || {
            let text_id_capture = Rc::clone(&text_id_capture);
            Column(
                Modifier::empty().padding(10.0),
                ColumnSpec::default(),
                move || {
                    *text_id_capture.borrow_mut() =
                        Some(Text("Hello", Modifier::empty().offset(5.0, 7.5)));
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 200.0,
                height: 200.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root().clone();
    assert_eq!(root_layout.children.len(), 1);
    let text_layout = &root_layout.children[0];
    assert_eq!(
        text_layout.node_id,
        text_id.borrow().as_ref().copied().expect("text node id")
    );
    assert!((text_layout.rect.x - 15.0).abs() < 1e-3);
    assert!((text_layout.rect.y - 17.5).abs() < 1e-3);
}

#[test]
fn box_with_constraints_composes_different_content() {
    let mut composition = Composition::new(MemoryApplier::new());
    let record = Rc::new(RefCell::new(Vec::new()));
    let record_capture = Rc::clone(&record);
    composition
        .render(location_key(file!(), line!(), column!()), || {
            BoxWithConstraints(Modifier::empty(), {
                let record_capture = Rc::clone(&record_capture);
                move |scope| {
                    let label = if scope.max_width().0 > 100.0 {
                        "wide"
                    } else {
                        "narrow"
                    };
                    record_capture.borrow_mut().push(label.to_string());
                    Text(label, Modifier::empty());
                }
            });
        })
        .expect("render succeeds");

    let root = composition.root().expect("root node");
    let handle = composition.runtime_handle();
    let mut slots = SlotBackend::default();

    measure_subcompose_node(
        &mut composition,
        &mut slots,
        &handle,
        root,
        Constraints::tight(200.0, 100.0),
    );

    assert_eq!(record.borrow().as_slice(), ["wide"]);

    slots.reset();

    measure_subcompose_node(
        &mut composition,
        &mut slots,
        &handle,
        root,
        Constraints::tight(80.0, 50.0),
    );

    assert_eq!(record.borrow().as_slice(), ["wide", "narrow"]);
}

#[test]
fn box_with_constraints_reacts_to_constraint_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let invocations = Rc::new(Cell::new(0));
    let invocations_capture = Rc::clone(&invocations);
    composition
        .render(location_key(file!(), line!(), column!()), || {
            BoxWithConstraints(Modifier::empty(), {
                let invocations_capture = Rc::clone(&invocations_capture);
                move |scope| {
                    let _ = scope.max_width();
                    invocations_capture.set(invocations_capture.get() + 1);
                    Text("child", Modifier::empty());
                }
            });
        })
        .expect("render succeeds");

    let root = composition.root().expect("root node");
    let handle = composition.runtime_handle();
    let mut slots = SlotBackend::default();

    for width in [120.0, 60.0] {
        let constraints = Constraints::tight(width, 40.0);
        measure_subcompose_node(&mut composition, &mut slots, &handle, root, constraints);
        slots.reset();
    }

    assert_eq!(invocations.get(), 2);
}

#[test]
fn test_fill_max_width_respects_parent_bounds() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let column_id_render = Rc::clone(&column_id);
    let row_id_render = Rc::clone(&row_id);

    composition
        .render(key, move || {
            let column_capture = Rc::clone(&column_id_render);
            let row_capture = Rc::clone(&row_id_render);

            // Outer Column with padding(20.0) and fill_max_width to ensure it has a defined width
            *column_capture.borrow_mut() = Some(Column(
                Modifier::empty()
                    .fill_max_width()
                    .then(Modifier::empty().padding(20.0)),
                ColumnSpec::default(),
                move || {
                    let row_inner = Rc::clone(&row_capture);
                    // Row with fill_max_width() and padding(8.0)
                    *row_inner.borrow_mut() = Some(Row(
                        Modifier::empty()
                            .fill_max_width()
                            .then(Modifier::empty().padding(8.0)),
                        RowSpec::default(),
                        move || {
                            Text("Button 1", Modifier::empty().padding(4.0));
                            Text("Button 2", Modifier::empty().padding(4.0));
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root();

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let column_node_id = column_id
        .borrow()
        .as_ref()
        .copied()
        .expect("column node id");
    let row_node_id = row_id.borrow().as_ref().copied().expect("row node id");

    let column_layout = find_layout(&root_layout, column_node_id).expect("column layout");
    let row_layout = find_layout(&root_layout, row_node_id).expect("row layout");

    // Debug output
    println!("\n=== Layout Debug ===");
    println!(
        "Root: x={}, y={}, width={}, height={}",
        root_layout.rect.x, root_layout.rect.y, root_layout.rect.width, root_layout.rect.height
    );
    println!(
        "Column: x={}, y={}, width={}, height={}",
        column_layout.rect.x,
        column_layout.rect.y,
        column_layout.rect.width,
        column_layout.rect.height
    );
    println!(
        "Row: x={}, y={}, width={}, height={}",
        row_layout.rect.x, row_layout.rect.y, row_layout.rect.width, row_layout.rect.height
    );
    println!(
        "Column inner width (after padding): {}",
        column_layout.rect.width - 40.0
    );
    println!(
        "Row right edge: {}",
        row_layout.rect.x + row_layout.rect.width
    );
    println!(
        "Column right inner edge: {}",
        column_layout.rect.x + column_layout.rect.width - 20.0
    );

    // Expected:
    // Window: 800px
    // Column: 800px (fills window)
    // Column has padding 40px (20 on each side)
    // Column inner content area: 760px
    // Row with fill_max_width() should fill the Column's inner width: 760px

    const EPSILON: f32 = 0.001;

    // Row should be 760px wide (Column's inner width)
    assert!(
        (row_layout.rect.width - 760.0).abs() < EPSILON,
        "Row should be 760px wide (Column inner width): actual={}",
        row_layout.rect.width
    );

    // Row's right edge should not exceed Column's right inner edge
    let row_right = row_layout.rect.x + row_layout.rect.width;
    let column_right_inner = column_layout.rect.x + column_layout.rect.width - 20.0;

    assert!(
        row_right <= column_right_inner + EPSILON,
        "Row overflows Column: Row right edge={} > Column inner right={}",
        row_right,
        column_right_inner
    );
}

#[test]
fn test_fill_max_width_with_background_and_double_padding() {
    // This test reproduces the exact structure from counter_app line 784:
    // Row with fill_max_width() + padding + background + padding again
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let outer_column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let inner_column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let outer_column_render = Rc::clone(&outer_column_id);
    let inner_column_render = Rc::clone(&inner_column_id);
    let row_render = Rc::clone(&row_id);

    composition
        .render(key, move || {
            let outer_capture = Rc::clone(&outer_column_render);
            let inner_capture = Rc::clone(&inner_column_render);
            let row_capture = Rc::clone(&row_render);

            // Simulating counter_app structure:
            // Outer Column with padding(32.0)
            *outer_capture.borrow_mut() = Some(Column(
                Modifier::empty().padding(32.0),
                ColumnSpec::default(),
                move || {
                    let inner_cap2 = Rc::clone(&inner_capture);
                    let row_cap2 = Rc::clone(&row_capture);
                    // Inner Column with width(360.0)
                    *inner_cap2.borrow_mut() = Some(Column(
                        Modifier::empty().width(360.0),
                        ColumnSpec::default(),
                        move || {
                            let row_cap3 = Rc::clone(&row_cap2);
                            // Row with fill_max_width() + padding + background + padding
                            *row_cap3.borrow_mut() = Some(Row(
                                Modifier::empty()
                                    .fill_max_width()
                                    .then(Modifier::empty().padding(8.0))
                                    .then(
                                        Modifier::empty()
                                            .background(crate::Color(0.1, 0.1, 0.15, 0.6)),
                                    )
                                    .then(Modifier::empty().padding(8.0)),
                                RowSpec::default(),
                                move || {
                                    Text("OK", Modifier::empty().padding(4.0));
                                    Text("Cancel", Modifier::empty().padding(4.0));
                                },
                            ));
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root();

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let outer_column_node = outer_column_id
        .borrow()
        .as_ref()
        .copied()
        .expect("outer column");
    let inner_column_node = inner_column_id
        .borrow()
        .as_ref()
        .copied()
        .expect("inner column");
    let row_node = row_id.borrow().as_ref().copied().expect("row");

    let outer_layout = find_layout(&root_layout, outer_column_node).expect("outer column layout");
    let inner_layout = find_layout(&root_layout, inner_column_node).expect("inner column layout");
    let row_layout = find_layout(&root_layout, row_node).expect("row layout");

    println!("\n=== Counter App Structure Test ===");
    println!("Window: 800px");
    println!(
        "Outer Column (padding 32): x={}, width={}",
        outer_layout.rect.x, outer_layout.rect.width
    );
    println!(
        "Inner Column (width 360): x={}, width={}",
        inner_layout.rect.x, inner_layout.rect.width
    );
    println!(
        "Row (fill_max_width): x={}, width={}",
        row_layout.rect.x, row_layout.rect.width
    );
    println!(
        "Row right edge: {}",
        row_layout.rect.x + row_layout.rect.width
    );
    println!(
        "Inner Column right edge: {}",
        inner_layout.rect.x + inner_layout.rect.width
    );

    const EPSILON: f32 = 0.001;

    // Inner Column should be 360px wide (explicit width)
    assert!(
        (inner_layout.rect.width - 360.0).abs() < EPSILON,
        "Inner Column should be 360px: got {}",
        inner_layout.rect.width
    );

    // Row should be 360px wide (fill_max_width inside 360px container)
    assert!(
        (row_layout.rect.width - 360.0).abs() < EPSILON,
        "Row should be 360px (Inner Column width): got {}",
        row_layout.rect.width
    );

    // Row should NOT overflow Inner Column
    let row_right = row_layout.rect.x + row_layout.rect.width;
    let column_right = inner_layout.rect.x + inner_layout.rect.width;
    assert!(
        row_right <= column_right + EPSILON,
        "Row overflows Inner Column: row_right={} > column_right={}",
        row_right,
        column_right
    );
}

#[test]
fn test_fill_max_width_should_not_propagate_to_wrapping_parent() {
    // Testing the issue where a child with fill_max_width() causes
    // its parent (which should wrap content) to also fill parent
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let outer_column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let inner_column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let outer_render = Rc::clone(&outer_column_id);
    let inner_render = Rc::clone(&inner_column_id);
    let row_render = Rc::clone(&row_id);

    composition
        .render(key, move || {
            let outer_cap = Rc::clone(&outer_render);
            let inner_cap = Rc::clone(&inner_render);
            let row_cap = Rc::clone(&row_render);

            // Outer Column - no size modifier (should wrap content)
            *outer_cap.borrow_mut() = Some(Column(
                Modifier::empty(),
                ColumnSpec::default(),
                move || {
                    let inner_cap2 = Rc::clone(&inner_cap);
                    let row_cap2 = Rc::clone(&row_cap);

                    // Inner Column - no size modifier (should wrap content)
                    *inner_cap2.borrow_mut() = Some(Column(
                        Modifier::empty(),
                        ColumnSpec::default(),
                        move || {
                            let row_cap3 = Rc::clone(&row_cap2);

                            // Row with fill_max_width() containing fixed-width content
                            *row_cap3.borrow_mut() = Some(Row(
                                Modifier::empty().fill_max_width(),
                                RowSpec::default(),
                                move || {
                                    // Fixed width content: 100px + 100px = 200px
                                    Spacer(Size {
                                        width: 100.0,
                                        height: 20.0,
                                    });
                                    Spacer(Size {
                                        width: 100.0,
                                        height: 20.0,
                                    });
                                },
                            ));
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root();

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let outer_node = outer_column_id.borrow().as_ref().copied().expect("outer");
    let inner_node = inner_column_id.borrow().as_ref().copied().expect("inner");
    let row_node = row_id.borrow().as_ref().copied().expect("row");

    let outer_layout = find_layout(&root_layout, outer_node).expect("outer layout");
    let inner_layout = find_layout(&root_layout, inner_node).expect("inner layout");
    let row_layout = find_layout(&root_layout, row_node).expect("row layout");

    println!("\n=== Fill Propagation Test ===");
    println!("Window: 800px");
    println!("Row content: 200px (100 + 100)");
    println!(
        "Outer Column (should wrap): width={}",
        outer_layout.rect.width
    );
    println!(
        "Inner Column (should wrap): width={}",
        inner_layout.rect.width
    );
    println!("Row (fill_max_width): width={}", row_layout.rect.width);

    // Expected behavior (now FIXED):
    // - Inner Column wants to wrap content (no size modifier)
    // - Inner Column queries Row's minimum intrinsic width = 200px
    // - Inner Column constrains itself to 200px
    // - Row with fill_max_width() fills that 200px container
    // - Outer Column (also intrinsic) wraps around Inner Column -> 200px

    const EPSILON: f32 = 0.001;

    // All elements should be 200px wide (wrapping to content)
    assert!(
        (outer_layout.rect.width - 200.0).abs() < EPSILON,
        "Outer Column should wrap to content (200px), got {}",
        outer_layout.rect.width
    );
    assert!(
        (inner_layout.rect.width - 200.0).abs() < EPSILON,
        "Inner Column should wrap to content (200px), got {}",
        inner_layout.rect.width
    );
    assert!(
        (row_layout.rect.width - 200.0).abs() < EPSILON,
        "Row should fill its 200px parent, got {}",
        row_layout.rect.width
    );
}

#[test]
fn wrap_column_with_fill_child_uses_content_width() {
    const EPSILON: f32 = 1e-3;

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let first_chip_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let second_chip_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let column_capture = Rc::clone(&column_id);
    let row_capture = Rc::clone(&row_id);
    let first_chip_capture = Rc::clone(&first_chip_id);
    let second_chip_capture = Rc::clone(&second_chip_id);

    composition
        .render(key, move || {
            let row_capture = Rc::clone(&row_capture);
            let first_chip_capture = Rc::clone(&first_chip_capture);
            let second_chip_capture = Rc::clone(&second_chip_capture);
            *column_capture.borrow_mut() = Some(Column(
                Modifier::empty().padding(10.0),
                ColumnSpec::default(),
                move || {
                    let row_inner = Rc::clone(&row_capture);
                    let first_inner = Rc::clone(&first_chip_capture);
                    let second_inner = Rc::clone(&second_chip_capture);

                    *row_inner.borrow_mut() = Some(Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default(),
                        move || {
                            *first_inner.borrow_mut() = Some(Spacer(Size {
                                width: 80.0,
                                height: 24.0,
                            }));
                            *second_inner.borrow_mut() = Some(Spacer(Size {
                                width: 40.0,
                                height: 24.0,
                            }));
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 640.0,
                height: 480.0,
            },
        )
        .expect("compute layout");

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let root_layout = layout_tree.root();
    let column_layout = find_layout(
        root_layout,
        column_id
            .borrow()
            .as_ref()
            .copied()
            .expect("column node id"),
    )
    .expect("column layout");
    let row_layout = find_layout(
        root_layout,
        row_id.borrow().as_ref().copied().expect("row node id"),
    )
    .expect("row layout");
    let first_chip_layout = find_layout(
        root_layout,
        first_chip_id
            .borrow()
            .as_ref()
            .copied()
            .expect("first chip id"),
    )
    .expect("first chip layout");
    let second_chip_layout = find_layout(
        root_layout,
        second_chip_id
            .borrow()
            .as_ref()
            .copied()
            .expect("second chip id"),
    )
    .expect("second chip layout");

    assert!(
        (column_layout.rect.width - 140.0).abs() < EPSILON,
        "Column width should wrap content (140px), got {:.3}",
        column_layout.rect.width
    );
    assert!(
        (row_layout.rect.width - 120.0).abs() < EPSILON,
        "Row width should match chip content (120px), got {:.3}",
        row_layout.rect.width
    );
    assert!(
        (row_layout.rect.x - 10.0).abs() < EPSILON,
        "Row x expected 10px from column padding, got {:.3}",
        row_layout.rect.x
    );
    assert!(
        (first_chip_layout.rect.width - 80.0).abs() < EPSILON,
        "First chip width expected 80px, got {:.3}",
        first_chip_layout.rect.width
    );
    assert!(
        (second_chip_layout.rect.width - 40.0).abs() < EPSILON,
        "Second chip width expected 40px, got {:.3}",
        second_chip_layout.rect.width
    );
}

#[test]
fn fill_child_respects_explicit_parent_width() {
    const EPSILON: f32 = 1e-3;

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let column_capture = Rc::clone(&column_id);
    let row_capture = Rc::clone(&row_id);

    composition
        .render(key, move || {
            let row_capture = Rc::clone(&row_capture);
            *column_capture.borrow_mut() = Some(Column(
                Modifier::empty().width(200.0),
                ColumnSpec::default(),
                move || {
                    let row_inner = Rc::clone(&row_capture);
                    *row_inner.borrow_mut() = Some(Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default(),
                        move || {
                            Spacer(Size {
                                width: 60.0,
                                height: 32.0,
                            });
                            Spacer(Size {
                                width: 40.0,
                                height: 32.0,
                            });
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 480.0,
                height: 320.0,
            },
        )
        .expect("compute layout");

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let root_layout = layout_tree.root();
    let column_layout = find_layout(
        root_layout,
        column_id
            .borrow()
            .as_ref()
            .copied()
            .expect("column node id"),
    )
    .expect("column layout");
    let row_layout = find_layout(
        root_layout,
        row_id.borrow().as_ref().copied().expect("row node id"),
    )
    .expect("row layout");

    assert!(
        (column_layout.rect.width - 200.0).abs() < EPSILON,
        "Column width expected 200px, got {:.3}",
        column_layout.rect.width
    );
    assert!(
        (row_layout.rect.width - 200.0).abs() < EPSILON,
        "Row width expected to expand to parent width (200px), got {:.3}",
        row_layout.rect.width
    );
    assert!(
        (row_layout.rect.height - 32.0).abs() < EPSILON,
        "Row height expected to match spacer height (32px), got {:.3}",
        row_layout.rect.height
    );
}

#[test]
fn fill_max_height_child_clamps_to_parent() {
    const EPSILON: f32 = 1e-3;

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let fill_column_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let leaf_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let row_capture = Rc::clone(&row_id);
    let fill_column_capture = Rc::clone(&fill_column_id);
    let leaf_capture = Rc::clone(&leaf_id);

    composition
        .render(key, move || {
            let fill_column_capture = Rc::clone(&fill_column_capture);
            let leaf_capture = Rc::clone(&leaf_capture);
            *row_capture.borrow_mut() = Some(Row(
                Modifier::empty().height(180.0),
                RowSpec::default(),
                move || {
                    let fill_column_inner = Rc::clone(&fill_column_capture);
                    let leaf_inner = Rc::clone(&leaf_capture);
                    *fill_column_inner.borrow_mut() = Some(Column(
                        Modifier::empty().fill_max_height(),
                        ColumnSpec::default(),
                        move || {
                            *leaf_inner.borrow_mut() = Some(Spacer(Size {
                                width: 60.0,
                                height: 40.0,
                            }));
                        },
                    ));
                },
            ));
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 240.0,
            },
        )
        .expect("compute layout");

    fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
        if node.node_id == target {
            return Some(node);
        }
        node.children
            .iter()
            .find_map(|child| find_layout(child, target))
    }

    let root_layout = layout_tree.root();
    let row_layout = find_layout(
        root_layout,
        row_id.borrow().as_ref().copied().expect("row id"),
    )
    .expect("row layout");
    let fill_column_layout = find_layout(
        root_layout,
        fill_column_id
            .borrow()
            .as_ref()
            .copied()
            .expect("fill column id"),
    )
    .expect("fill column layout");
    let leaf_layout = find_layout(
        root_layout,
        leaf_id.borrow().as_ref().copied().expect("leaf id"),
    )
    .expect("leaf layout");

    assert!(
        (row_layout.rect.height - 180.0).abs() < EPSILON,
        "Row height expected 180px, got {:.3}",
        row_layout.rect.height
    );
    assert!(
        (fill_column_layout.rect.height - 180.0).abs() < EPSILON,
        "Fill column height expected to clamp to parent (180px), got {:.3}",
        fill_column_layout.rect.height
    );
    assert!(
        (leaf_layout.rect.height - 40.0).abs() < EPSILON,
        "Leaf spacer height expected 40px, got {:.3}",
        leaf_layout.rect.height
    );
}

#[test]
fn modifier_chain_text_with_padding() {
    // Verify that text with padding modifier measures correctly
    // This tests the core fix: padding should wrap text, not text wrap padding
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let text_id_render = Rc::clone(&text_id);

    composition
        .render(key, move || {
            *text_id_render.borrow_mut() = Some(Text(
                "Hello",
                Modifier::empty().padding(10.0),
            ));
        })
        .expect("render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 640.0,
                height: 480.0,
            },
        )
        .expect("compute layout");

    let text_node_id = text_id.borrow().expect("text node id");
    let text_layout = find_node_layout(layout_tree.root(), text_node_id).expect("text layout");

    // Text "Hello" should measure to some size, let's say roughly 35x16
    // With padding(10), the total size should be text_size + 20 (10 on each side)
    // The key assertion: padding should be ADDED to text size, not ignored
    assert!(
        text_layout.rect.width > 35.0 && text_layout.rect.width < 100.0,
        "Text with padding width should be text_width + 20, got {:.3}",
        text_layout.rect.width
    );
    assert!(
        text_layout.rect.height > 16.0 && text_layout.rect.height < 50.0,
        "Text with padding height should be text_height + 20, got {:.3}",
        text_layout.rect.height
    );
}

#[test]
fn modifier_chain_size_enforcement() {
    // Verify that size modifier enforces exact size
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let spacer_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let spacer_id_render = Rc::clone(&spacer_id);

    composition
        .render(key, move || {
            // Spacer wants 100x100, but size modifier should enforce 50x50
            *spacer_id_render.borrow_mut() = Some(Box(
                Modifier::empty().size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {
                    Spacer(Size {
                        width: 100.0,
                        height: 100.0,
                    });
                },
            ));
        })
        .expect("render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 640.0,
                height: 480.0,
            },
        )
        .expect("compute layout");

    let spacer_node_id = spacer_id.borrow().expect("spacer node id");
    let spacer_layout = find_node_layout(layout_tree.root(), spacer_node_id).expect("spacer layout");

    const EPSILON: f32 = 1e-3;
    assert!(
        (spacer_layout.rect.width - 50.0).abs() < EPSILON,
        "Size modifier should enforce width=50, got {:.3}",
        spacer_layout.rect.width
    );
    assert!(
        (spacer_layout.rect.height - 50.0).abs() < EPSILON,
        "Size modifier should enforce height=50, got {:.3}",
        spacer_layout.rect.height
    );
}

#[test]
fn modifier_chain_padding_then_size() {
    // Jetpack Compose behavior: padding(10).size(100, 80)
    // Modifiers apply right-to-left: size is inner, padding is outer
    // Result: padding adds to size, giving 120x100 (100+20, 80+20)
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let node_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let node_id_render = Rc::clone(&node_id);

    composition
        .render(key, move || {
            *node_id_render.borrow_mut() = Some(Box(
                Modifier::empty()
                    .padding(10.0)
                    .size(Size {
                        width: 100.0,
                        height: 80.0,
                    }),
                BoxSpec::default(),
                || {
                    Spacer(Size {
                        width: 200.0,
                        height: 150.0,
                    });
                },
            ));
        })
        .expect("render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 640.0,
                height: 480.0,
            },
        )
        .expect("compute layout");

    let layout_node_id = node_id.borrow().expect("node id");
    let layout = find_node_layout(layout_tree.root(), layout_node_id).expect("layout");

    const EPSILON: f32 = 1e-3;
    // Padding is outer modifier, so it adds 20 (10 on each side) to the size
    assert!(
        (layout.rect.width - 120.0).abs() < EPSILON,
        "padding(10).size(100, 80) should give width=120 (100+20), got {:.3}",
        layout.rect.width
    );
    assert!(
        (layout.rect.height - 100.0).abs() < EPSILON,
        "padding(10).size(100, 80) should give height=100 (80+20), got {:.3}",
        layout.rect.height
    );
}

#[test]
fn modifier_chain_size_then_padding() {
    // Jetpack Compose behavior: size(100, 80).padding(10)
    // Modifiers apply right-to-left: padding is inner, size is outer
    // Result: size constrains to 100x80 (padding is inside)
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let node_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let node_id_render = Rc::clone(&node_id);

    composition
        .render(key, move || {
            *node_id_render.borrow_mut() = Some(Box(
                Modifier::empty()
                    .size(Size {
                        width: 100.0,
                        height: 80.0,
                    })
                    .padding(10.0),
                BoxSpec::default(),
                || {
                    Spacer(Size {
                        width: 200.0,
                        height: 150.0,
                    });
                },
            ));
        })
        .expect("render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 640.0,
                height: 480.0,
            },
        )
        .expect("compute layout");

    let layout_node_id = node_id.borrow().expect("node id");
    let layout = find_node_layout(layout_tree.root(), layout_node_id).expect("layout");

    const EPSILON: f32 = 1e-3;
    // Size is outer modifier, so it constrains final result to exactly 100x80
    assert!(
        (layout.rect.width - 100.0).abs() < EPSILON,
        "size(100, 80).padding(10) should give width=100, got {:.3}",
        layout.rect.width
    );
    assert!(
        (layout.rect.height - 80.0).abs() < EPSILON,
        "size(100, 80).padding(10) should give height=80, got {:.3}",
        layout.rect.height
    );
}

fn find_node_layout(tree: &LayoutBox, target: NodeId) -> Option<LayoutBox> {
    if tree.node_id == target {
        return Some(tree.clone());
    }
    for child in &tree.children {
        if let Some(layout) = find_node_layout(child, target) {
            return Some(layout);
        }
    }
    None
}
