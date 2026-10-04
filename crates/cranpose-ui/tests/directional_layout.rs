use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LayoutBox, LayoutDirection, Modifier, ProvideLayoutDirection,
    Row, RowSpec, Size,
};

use crate::text_measured_layout_integration::layout_composition;

fn tagged<'a>(node: &'a LayoutBox, tag: &str) -> &'a LayoutBox {
    fn find<'a>(node: &'a LayoutBox, tag: &str) -> Option<&'a LayoutBox> {
        if node
            .node_data
            .semantics()
            .and_then(|config| config.content_description.as_deref())
            == Some(tag)
        {
            return Some(node);
        }
        node.children.iter().find_map(|node| find(node, tag))
    }
    find(node, tag).expect("tagged box")
}

#[test]
fn rtl_rows_columns_boxes_and_flow_rows_resolve_start_from_the_right() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        ProvideLayoutDirection(LayoutDirection::Rtl, || {
            Column(
                Modifier::empty().width(200.0),
                ColumnSpec::default(),
                || {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default(),
                        || {
                            Box(
                                Modifier::empty()
                                    .size_points(20.0, 10.0)
                                    .content_description("first"),
                                BoxSpec::default(),
                                || {},
                            );
                            Box(
                                Modifier::empty()
                                    .size_points(30.0, 10.0)
                                    .content_description("second"),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                    Box(
                        Modifier::empty()
                            .size_points(40.0, 10.0)
                            .content_description("column"),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty().size_points(200.0, 20.0),
                        BoxSpec::default(),
                        || {
                            Box(
                                Modifier::empty()
                                    .size_points(25.0, 10.0)
                                    .content_description("box"),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                    cranpose_ui::widgets::FlowRow(
                        Modifier::empty().width(200.0),
                        cranpose_ui::widgets::FlowRowSpec::default(),
                        || {
                            Box(
                                Modifier::empty()
                                    .size_points(60.0, 10.0)
                                    .content_description("flow"),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                },
            );
        });
    });
    let layout = layout_composition(&mut composition, Size::new(200.0, 300.0));
    for (tag, expected) in [
        ("first", 180.0),
        ("second", 150.0),
        ("column", 160.0),
        ("box", 175.0),
        ("flow", 140.0),
    ] {
        assert_eq!(tagged(layout.root(), tag).rect.x, expected, "{tag}");
    }
}

#[test]
fn relative_offsets_follow_the_provider_and_absolute_offsets_keep_physical_coordinates() {
    let modifier = Modifier::empty().offset(15.0, 2.0).size_points(20.0, 10.0);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let modifier = modifier.clone();
        ProvideLayoutDirection(LayoutDirection::Rtl, move || {
            Box(
                Modifier::empty().size_points(200.0, 50.0),
                BoxSpec::default(),
                move || {
                    Box(
                        modifier.clone().content_description("relative"),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .absolute_offset(15.0, 2.0)
                            .size_points(20.0, 10.0)
                            .content_description("absolute"),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    let layout = layout_composition(&mut composition, Size::new(200.0, 50.0));
    assert_eq!(tagged(layout.root(), "relative").rect.x, 165.0);
    assert_eq!(tagged(layout.root(), "absolute").rect.x, 195.0);
    assert_eq!(tagged(layout.root(), "relative").rect.y, 2.0);
}

#[test]
fn lazy_lists_update_positions_after_direction_changes_and_scroll() {
    let mut composition = cranpose_ui::run_test_composition(|| {});
    let direction = cranpose_core::MutableState::with_runtime(
        LayoutDirection::Ltr,
        composition.runtime_handle(),
    );
    let row_state = std::rc::Rc::new(std::cell::Cell::new(None));
    let captured_state = row_state.clone();
    composition
        .render(992, move || {
            DirectionalLists(direction, captured_state.clone());
        })
        .expect("list composition");
    for (mode, expected) in [
        (LayoutDirection::Ltr, [0.0, 0.0, 0.0, 20.0]),
        (LayoutDirection::Rtl, [180.0, 170.0, 180.0, 150.0]),
        (LayoutDirection::Ltr, [0.0, 0.0, 0.0, 20.0]),
        (LayoutDirection::Rtl, [180.0, 170.0, 180.0, 150.0]),
    ] {
        direction.set_value(mode);
        while composition
            .process_invalid_scopes()
            .expect("direction change")
        {}
        let layout = layout_composition(&mut composition, Size::new(200.0, 100.0));
        for (tag, expected) in [
            "vertical first",
            "vertical second",
            "horizontal first",
            "horizontal second",
        ]
        .into_iter()
        .zip(expected)
        {
            assert_eq!(
                tagged(layout.root(), tag).rect.x,
                expected,
                "{mode:?}: {tag}"
            );
        }
    }
    row_state
        .get()
        .expect("row state")
        .dispatch_scroll_delta(-10.0);
    while composition.process_invalid_scopes().expect("scroll") {}
    let layout = layout_composition(&mut composition, Size::new(200.0, 100.0));
    assert_eq!(tagged(layout.root(), "horizontal first").rect.x, 190.0);
    assert_eq!(tagged(layout.root(), "horizontal second").rect.x, 160.0);
}

#[cranpose_ui::composable]
fn DirectionalLists(
    direction: cranpose_core::MutableState<LayoutDirection>,
    captured_state: std::rc::Rc<std::cell::Cell<Option<cranpose_foundation::lazy::LazyListState>>>,
) {
    ProvideLayoutDirection(direction.value(), move || {
        Column(
            Modifier::empty().width(200.0),
            ColumnSpec::default(),
            move || {
                cranpose_ui::LazyColumn(
                    Modifier::empty().size_points(200.0, 30.0),
                    rememberLazyListState(),
                    cranpose_ui::LazyColumnSpec::default(),
                    |scope| {
                        scope.items(LazyItems::new(1), |_| {
                            Box(
                                Modifier::empty()
                                    .size_points(20.0, 10.0)
                                    .content_description("vertical first"),
                                BoxSpec::default(),
                                || {},
                            );
                            Box(
                                Modifier::empty()
                                    .size_points(30.0, 10.0)
                                    .content_description("vertical second"),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
                let state = rememberLazyListState();
                captured_state.set(Some(state));
                cranpose_ui::LazyRow(
                    Modifier::empty().size_points(200.0, 20.0),
                    state,
                    cranpose_ui::LazyRowSpec::default(),
                    |scope| {
                        scope.items(LazyItems::new(6), |index| {
                            if index == 0 {
                                Box(
                                    Modifier::empty()
                                        .size_points(20.0, 10.0)
                                        .content_description("horizontal first"),
                                    BoxSpec::default(),
                                    || {},
                                );
                                Box(
                                    Modifier::empty()
                                        .size_points(30.0, 10.0)
                                        .content_description("horizontal second"),
                                    BoxSpec::default(),
                                    || {},
                                );
                            } else {
                                Box(
                                    Modifier::empty().size_points(50.0, 10.0),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        });
                    },
                );
            },
        );
    });
}
