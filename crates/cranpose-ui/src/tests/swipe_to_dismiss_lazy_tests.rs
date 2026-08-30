use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_ui_graphics::Size as ViewportSize;

use super::*;

fn compose_lazy_swipe_rows() -> TestComposition {
    run_test_composition(|| {
        let list_state = rememberLazyListState();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(5, |index| {
                    SwipeToDismiss(
                        Modifier::empty().fill_max_width().height(48.0),
                        SwipeToDismissSpec::default(),
                        || {},
                        move || {
                            Text(
                                format!("Row {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                });
            },
        );
    })
}

fn measure(composition: &mut TestComposition, root: NodeId) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout(
        &mut applier,
        root,
        ViewportSize {
            width: 320.0,
            height: 480.0,
        },
    )
    .expect("layout measurement");
    applier.clear_runtime_handle();
}

fn layout_texts(tree: &crate::LayoutTree) -> Vec<String> {
    fn walk(node: &crate::LayoutBox, out: &mut Vec<String>) {
        if let Some(text) = node.node_data.modifier_slices().text_content() {
            out.push(text.to_string());
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree.root(), &mut out);
    out
}

fn semantics_descriptions(tree: &crate::SemanticsTree) -> Vec<String> {
    fn walk(node: &crate::SemanticsNode, out: &mut Vec<String>) {
        if let Some(description) = &node.description {
            out.push(description.clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree.root(), &mut out);
    out
}

#[test]
fn swipe_to_dismiss_renders_content_inside_lazy_column_item() {
    let mut composition = compose_lazy_swipe_rows();
    let root = composition.root().expect("lazy column root");
    measure(&mut composition, root);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = crate::build_layout_tree_from_applier(&mut applier, root)
        .expect("layout tree build")
        .expect("layout tree present");
    applier.clear_runtime_handle();

    let texts = layout_texts(&tree);
    assert!(
        texts.iter().any(|text| text == "Row 0"),
        "SwipeToDismiss content must appear in the applier-traversal layout build, got {texts:?}"
    );
}

#[test]
fn swipe_to_dismiss_exposes_semantics_inside_lazy_column_item() {
    let mut composition = compose_lazy_swipe_rows();
    let root = composition.root().expect("lazy column root");
    measure(&mut composition, root);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = crate::build_semantics_tree_from_applier(&mut applier, root)
        .expect("semantics tree build")
        .expect("semantics tree present");
    applier.clear_runtime_handle();

    let descriptions = semantics_descriptions(&tree);
    assert!(
        descriptions
            .iter()
            .any(|description| description == "Row 0"),
        "SwipeToDismiss semantics must appear in the applier-traversal build, got {descriptions:?}"
    );
}

#[test]
fn a_keyed_lazy_item_supplies_its_key_to_its_content() {
    let seen: Rc<RefCell<Vec<Option<u64>>>> = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&seen);
    let mut composition = run_test_composition(move || {
        let recorder = Rc::clone(&recorder);
        let list_state = rememberLazyListState();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            move |scope| {
                let recorder = Rc::clone(&recorder);
                scope.items(
                    LazyItems::new(3).key(move |index: usize| 700 + index as u64),
                    move |index| {
                        recorder
                            .borrow_mut()
                            .push(crate::lazy_item::lazy_item_key());
                        Text(
                            format!("Row {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
    let root = composition.root().expect("lazy column root");
    measure(&mut composition, root);

    let keys = seen.borrow().clone();
    assert!(!keys.is_empty(), "no lazy item was composed");
    assert!(
        keys.iter().all(Option::is_some),
        "a keyed item must report an identity, got {keys:?}"
    );
    let distinct: std::collections::HashSet<_> = keys.iter().copied().collect();
    assert!(
        distinct.len() > 1,
        "each item must report its own identity, got {keys:?}"
    );
}

#[test]
fn an_unkeyed_lazy_item_reports_no_identity() {
    let seen: Rc<RefCell<Vec<Option<u64>>>> = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&seen);
    let mut composition = run_test_composition(move || {
        let recorder = Rc::clone(&recorder);
        let list_state = rememberLazyListState();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            move |scope| {
                let recorder = Rc::clone(&recorder);
                scope.items(3, move |index| {
                    recorder
                        .borrow_mut()
                        .push(crate::lazy_item::lazy_item_key());
                    Text(
                        format!("Row {index}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
    let root = composition.root().expect("lazy column root");
    measure(&mut composition, root);

    let keys = seen.borrow().clone();
    assert!(!keys.is_empty(), "no lazy item was composed");
    assert!(
        keys.iter().all(Option::is_none),
        "an index is not an identity, got {keys:?}"
    );
}
