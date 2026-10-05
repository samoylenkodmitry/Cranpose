use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{CompositionLocalProvider, OwnedMutableState, with_current_composer};
use cranpose_ui::{
    Alignment, Box, BoxSpec, EdgeInsets, LayoutBox, Modifier, Size, composable, local_ime_insets,
    local_safe_area_insets,
};

use crate::text_measured_layout_integration::layout_composition;

#[composable]
fn InsetConsumer(value: Rc<Cell<f32>>, calls: Rc<Cell<usize>>) {
    calls.set(calls.get() + 1);
    value.set(local_ime_insets().current().bottom);
}

#[composable]
fn Unrelated(calls: Rc<Cell<usize>>) {
    calls.set(calls.get() + 1);
}

fn state(value: EdgeInsets) -> OwnedMutableState<EdgeInsets> {
    with_current_composer(|composer| {
        OwnedMutableState::with_runtime_structural_eq(value, composer.runtime_handle())
    })
}

fn bottom(value: f32) -> EdgeInsets {
    EdgeInsets::from_components(0.0, 0.0, 0.0, value)
}

fn tagged<'a>(node: &'a LayoutBox, tag: &str) -> &'a LayoutBox {
    fn find<'a>(node: &'a LayoutBox, tag: &str) -> Option<&'a LayoutBox> {
        if node
            .node_data
            .semantics()
            .and_then(|s| s.content_description.as_deref())
            == Some(tag)
        {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, tag))
    }
    find(node, tag).expect("tagged box")
}

#[test]
fn changing_insets_recomposes_only_the_reader_and_preserves_overrides() {
    let source = Rc::new(RefCell::new(None));
    let root_calls = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let unrelated = Rc::new(Cell::new(0));
    let value = Rc::new(Cell::new(0.0));
    let override_value = Rc::new(Cell::new(0.0));
    let override_calls = Rc::new(Cell::new(0));
    let mut composition = cranpose_ui::run_test_composition(|| {
        root_calls.set(root_calls.get() + 1);
        let insets = state(bottom(10.0));
        *source.borrow_mut() = Some(insets.clone());
        CompositionLocalProvider([local_ime_insets().provides_state(insets)], || {
            Box(Modifier::empty(), BoxSpec::default(), {
                let (value, calls, unrelated, override_value, override_calls) = (
                    value.clone(),
                    calls.clone(),
                    unrelated.clone(),
                    override_value.clone(),
                    override_calls.clone(),
                );
                move || {
                    InsetConsumer(value.clone(), calls.clone());
                    Unrelated(unrelated.clone());
                    CompositionLocalProvider([local_ime_insets().provides(bottom(7.0))], || {
                        InsetConsumer(override_value.clone(), override_calls.clone());
                    });
                }
            });
        });
    });
    composition.take_root_render_request();
    assert_eq!(value.get(), 10.0);
    let initial_calls = calls.get();
    source.borrow().as_ref().expect("source").set(bottom(80.0));
    assert!(
        !composition.take_root_render_request(),
        "insets requested the root"
    );
    composition.process_invalid_scopes().expect("scoped update");
    assert_eq!(value.get(), 80.0);
    assert!(calls.get() > initial_calls);
    assert_eq!(
        (root_calls.get(), unrelated.get(), override_calls.get()),
        (1, 1, 1)
    );
    assert_eq!(override_value.get(), 7.0);
    let unchanged = calls.get();
    source.borrow().as_ref().expect("source").set(bottom(80.0));
    composition.process_invalid_scopes().expect("equal update");
    assert_eq!(calls.get(), unchanged);
}

#[test]
fn inset_padding_updates_geometry_without_recomposition() {
    for ime in [false, true] {
        let source = Rc::new(RefCell::new(None));
        let builds = Rc::new(Cell::new(0));
        let mut composition = cranpose_ui::run_test_composition(|| {
            builds.set(builds.get() + 1);
            let insets = state(bottom(10.0));
            *source.borrow_mut() = Some(insets.clone());
            let local = if ime {
                local_ime_insets()
            } else {
                local_safe_area_insets()
            };
            CompositionLocalProvider([local.provides_state(insets)], || {
                let modifier = Modifier::empty().size_points(200.0, 200.0);
                let modifier = if ime {
                    modifier.ime_padding()
                } else {
                    modifier.safe_area_padding()
                };
                Box(
                    modifier,
                    BoxSpec::default().content_alignment(Alignment::BOTTOM_START),
                    || {
                        Box(
                            Modifier::empty()
                                .size_points(20.0, 10.0)
                                .content_description("child"),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            });
        });
        let before = layout_composition(&mut composition, Size::new(200.0, 200.0));
        assert_eq!(tagged(before.root(), "child").rect.y, 180.0);
        composition.take_root_render_request();
        source.borrow().as_ref().expect("source").set(bottom(60.0));
        composition.runtime_handle().drain_ui();
        assert!(!composition.take_root_render_request());
        assert!(
            !composition
                .process_invalid_scopes()
                .expect("layout-only update")
        );
        let after = layout_composition(&mut composition, Size::new(200.0, 200.0));
        assert_eq!(tagged(after.root(), "child").rect.y, 130.0);
        assert_eq!(builds.get(), 1);
        source.borrow().as_ref().expect("source").set(bottom(0.0));
        composition.runtime_handle().drain_ui();
        let hidden = layout_composition(&mut composition, Size::new(200.0, 200.0));
        assert_eq!(tagged(hidden.root(), "child").rect.y, 190.0);
    }
}

#[test]
fn retained_inset_reader_follows_provider_source_changes() {
    let captured = Rc::new(RefCell::new(None));
    let sources = Rc::new(RefCell::new(None));
    let mut composition = cranpose_ui::run_test_composition(|| {
        let a = state(bottom(12.0));
        let b = state(bottom(40.0));
        *sources.borrow_mut() = Some((a, b));
    });
    let (a, b) = sources.borrow().as_ref().expect("sources").clone();
    let render = |source: OwnedMutableState<EdgeInsets>| {
        CompositionLocalProvider([local_ime_insets().provides_state(source)], || {
            let mut captured = captured.borrow_mut();
            if captured.is_none() {
                *captured = Some(local_ime_insets().reader());
            }
        });
    };
    composition
        .render(123, || render(a.clone()))
        .expect("first provider");
    assert_eq!(
        captured.borrow().as_ref().expect("reader").value().bottom,
        12.0
    );
    composition
        .render(123, || render(b.clone()))
        .expect("replace provider source");
    assert_eq!(
        captured.borrow().as_ref().expect("reader").value().bottom,
        40.0
    );
    a.set(bottom(90.0));
    assert_eq!(
        captured.borrow().as_ref().expect("reader").value().bottom,
        40.0
    );
    b.set(bottom(70.0));
    assert_eq!(
        captured.borrow().as_ref().expect("reader").value().bottom,
        70.0
    );
}

#[test]
fn scaffold_updates_content_padding_without_rerunning_its_owner() {
    let source = Rc::new(RefCell::new(None));
    let builds = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(0.0));
    let mut composition = cranpose_ui::run_test_composition(|| {
        builds.set(builds.get() + 1);
        let insets = state(bottom(15.0));
        *source.borrow_mut() = Some(insets.clone());
        CompositionLocalProvider([local_ime_insets().provides_state(insets)], || {
            let seen = seen.clone();
            cranpose_ui::widgets::Scaffold(
                Modifier::empty().fill_max_size(),
                || {},
                || {},
                move |padding| {
                    seen.set(padding.bottom);
                    Box(
                        padding.apply_to(Modifier::empty().fill_max_size()),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    layout_composition(&mut composition, Size::new(200.0, 200.0));
    assert_eq!(seen.get(), 15.0);
    source.borrow().as_ref().expect("source").set(bottom(70.0));
    composition.runtime_handle().drain_ui();
    composition.process_invalid_scopes().expect("update");
    layout_composition(&mut composition, Size::new(200.0, 200.0));
    assert_eq!(seen.get(), 70.0);
    assert_eq!(builds.get(), 1);
}
