#![cfg(feature = "embedded-default-font")]
#![allow(non_snake_case)]

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MemoryApplier, MutableState, NodeId};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_render_common::scene_builder::{
    build_graph_from_applier, update_graph_from_applier_report,
};
use cranpose_ui::{
    LayoutEngine, LinearArrangement, Modifier, Size, Text, TextStyle,
    widgets::{LazyColumn, LazyColumnSpec},
};

mod scene_probe;

use scene_probe::painted_text;

const VIEWPORT: Size = Size {
    width: 320.0,
    height: 480.0,
};

#[cranpose_ui::composable]
fn TrimmableList(dropped: MutableState<usize>) {
    let state = rememberLazyListState();
    let visible: Vec<usize> = (dropped.get().min(3)..3).collect();
    let keys = visible.clone();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        state,
        LazyColumnSpec::default()
            .content_padding(100.0, 128.0)
            .vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move |scope| {
            let bodies = visible.clone();
            let keys = keys.clone();
            scope.item(|| {
                Text(
                    "Header".to_string(),
                    Modifier::empty().fill_max_width().height(40.0),
                    TextStyle::default(),
                );
            });
            scope.items(
                LazyItems::new(bodies.len())
                    .key(move |order: usize| keys.get(order).copied().unwrap_or(order) as u64),
                move |order| {
                    let Some(&body) = bodies.get(order) else {
                        return;
                    };
                    Text(
                        format!("Body {body}"),
                        Modifier::empty().fill_max_width().height(96.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

/// The scope the app shell hands the scoped scene update: every node whose
/// geometry the layout pass changed, plus every parent whose child set did.
fn scoped_scene_nodes(applier: &mut MemoryApplier, root: NodeId) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    for node in cranpose_ui::take_geometry_scene_nodes() {
        if let Some(node) = applier.scene_node_attached_to(node, root)
            && !nodes.contains(&node)
        {
            nodes.push(node);
        }
    }
    for node in applier.take_structural_change_parents_attached_to(root) {
        if !nodes.contains(&node) {
            nodes.push(node);
        }
    }
    nodes
}

#[test]
fn dropping_leading_keyed_lazy_rows_one_at_a_time_repaints_the_survivors() {
    let dropped_cell: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let mut composition = cranpose_ui::run_test_composition({
        let dropped_cell = Rc::clone(&dropped_cell);
        move || {
            let dropped = cranpose_core::rememberMutableStateOf(|| 0usize);
            *dropped_cell.borrow_mut() = Some(dropped);
            TrimmableList(dropped);
        }
    });

    let root = composition.root().expect("list root");
    let handle = composition.runtime_handle();
    let mut graph = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        applier.compute_layout(root, VIEWPORT).expect("layout");
        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("render graph");
        applier.clear_runtime_handle();
        graph
    };
    let _ = scoped_scene_nodes(&mut composition.applier_mut(), root);

    let mut painted = Vec::new();
    painted_text(&graph.root, &mut painted);
    assert_eq!(
        painted,
        vec![
            "Header".to_string(),
            "Body 0".to_string(),
            "Body 1".to_string(),
            "Body 2".to_string()
        ],
        "every row should paint before the first is dropped"
    );

    let dropped = dropped_cell.borrow().expect("state captured");
    for count in 1..=2usize {
        dropped.set(count);
        while composition
            .process_invalid_scopes()
            .expect("drop the leading lazy row")
        {}

        {
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle.clone());
            applier.compute_layout(root, VIEWPORT).expect("relayout");
            let dirty = scoped_scene_nodes(&mut applier, root);
            assert!(
                !dirty.is_empty(),
                "dropping row {} told the scene phase nothing had moved",
                count - 1
            );
            if !update_graph_from_applier_report(&mut applier, &mut graph, &dirty, 1.0).applied() {
                graph = build_graph_from_applier(&mut applier, root, 1.0)
                    .expect("rebuilt render graph");
            }
            applier.clear_runtime_handle();
        }

        let mut painted = Vec::new();
        painted_text(&graph.root, &mut painted);
        let mut expected = vec!["Header".to_string()];
        expected.extend((count..3).map(|body| format!("Body {body}")));
        assert_eq!(
            painted, expected,
            "after dropping {count} leading row(s) the renderer must be handed exactly the \
             rows that are left, in order"
        );
    }
}
