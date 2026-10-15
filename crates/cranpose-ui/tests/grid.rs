use std::{cell::Cell, rc::Rc};

use cranpose_ui::{
    Alignment, Box, BoxSpec, IntrinsicSize, LayoutDirection, LayoutTree, Modifier,
    ProvideLayoutDirection, Size, Text, TextStyle, set_text_measurer,
    widgets::{FlowRow, FlowSpec, Fr, Grid, GridConfig, GridFlow, GridItem, GridTrackSize},
};

use crate::{
    directional_layout::tagged, text_contract_measurer::ContractMeasurer,
    text_measured_layout_integration::layout_composition,
};

/// A box of `width` by `height` the tests find by `tag`.
fn tagged_box(tag: &'static str, item: Option<GridItem>, width: f32, height: f32) {
    let modifier = Modifier::empty();
    let modifier = match item {
        Some(item) => modifier.grid_item(item),
        None => modifier,
    };
    Box(
        modifier.size_points(width, height).content_description(tag),
        BoxSpec::default(),
        || {},
    );
}

/// Two 24 by 10 boxes that wrap: 24 wide at the smallest, 48 unbounded.
fn two_words(tag: &'static str) {
    FlowRow(
        Modifier::empty().content_description(tag),
        FlowSpec::new(),
        || {
            for _ in 0..2 {
                Box(
                    Modifier::empty().size_points(24.0, 10.0),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

/// Lays `content` out in a grid of `config` that fills a `width` by
/// `height` viewport.
fn lay_out(
    direction: LayoutDirection,
    config: GridConfig,
    (width, height): (f32, f32),
    content: impl Fn() + 'static,
) -> LayoutTree {
    let content = Rc::new(content);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let config = config.clone();
        let content = Rc::clone(&content);
        ProvideLayoutDirection(direction, move || {
            let content = Rc::clone(&content);
            Grid(config, Modifier::empty().fill_max_size(), move || content());
        });
    });
    layout_composition(&mut composition, Size::new(width, height))
}

fn position(layout: &LayoutTree, tag: &str) -> (f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y)
}

fn size(layout: &LayoutTree, tag: &str) -> (f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.width, rect.height)
}

#[test]
fn children_without_cells_fill_the_rows_and_flexible_columns_share_the_room() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new()
            .columns([
                GridTrackSize::Fixed(50.0),
                GridTrackSize::Flex(Fr(1.0)),
                GridTrackSize::Flex(Fr(2.0)),
            ])
            .gap(10.0),
        (300.0, 200.0),
        || {
            for tag in ["a", "b", "c", "d", "e"] {
                tagged_box(tag, None, 20.0, 20.0);
            }
        },
    );
    // 280 of room after two gaps: 50 fixed, then the flexible columns
    // start at their content's 20 each and share the 190 left 1 to 2.
    assert_eq!(position(&layout, "a"), (0.0, 0.0));
    assert_eq!(position(&layout, "b"), (60.0, 0.0));
    assert_eq!(position(&layout, "c"), (153.0, 0.0));
    assert_eq!(position(&layout, "d"), (0.0, 30.0));
    assert_eq!(position(&layout, "e"), (60.0, 30.0));
}

#[test]
fn areas_and_explicit_cells_come_first_and_negative_indices_count_from_the_end() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new()
            .columns([GridTrackSize::Fixed(40.0); 3])
            .rows([GridTrackSize::Fixed(30.0); 2])
            .area("header", 1, 1, 1, 3),
        (300.0, 300.0),
        || {
            Box(
                Modifier::empty()
                    .grid_item(GridItem::area("header"))
                    .fill_max_size()
                    .content_description("header"),
                BoxSpec::default(),
                || {},
            );
            tagged_box("last", Some(GridItem::at(-1, -1)), 10.0, 10.0);
            for tag in ["first", "second", "third"] {
                tagged_box(tag, None, 10.0, 10.0);
            }
        },
    );
    assert_eq!(position(&layout, "header"), (0.0, 0.0));
    assert_eq!(size(&layout, "header"), (120.0, 30.0));
    assert_eq!(position(&layout, "last"), (80.0, 30.0));
    assert_eq!(position(&layout, "first"), (0.0, 30.0));
    assert_eq!(position(&layout, "second"), (40.0, 30.0));
    assert_eq!(
        position(&layout, "third"),
        (0.0, 60.0),
        "the second row is full, so the child adds a row"
    );
}

#[test]
fn a_column_flow_fills_columns_and_unknown_areas_take_the_next_cell() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new()
            .columns([GridTrackSize::Fixed(40.0); 2])
            .rows([GridTrackSize::Fixed(30.0); 2])
            .flow(GridFlow::Column),
        (300.0, 300.0),
        || {
            tagged_box("a", None, 10.0, 10.0);
            tagged_box("b", Some(GridItem::area("missing")), 10.0, 10.0);
            tagged_box("c", None, 10.0, 10.0);
        },
    );
    assert_eq!(position(&layout, "a"), (0.0, 0.0));
    assert_eq!(position(&layout, "b"), (0.0, 30.0));
    assert_eq!(position(&layout, "c"), (40.0, 0.0));
}

#[test]
fn children_align_in_their_cells_and_right_to_left_mirrors_the_columns() {
    let config = GridConfig::new()
        .columns([GridTrackSize::Fixed(40.0), GridTrackSize::Fixed(60.0)])
        .rows([GridTrackSize::Fixed(30.0)]);
    let content = || {
        tagged_box(
            "centered",
            Some(GridItem::at(1, 1).alignment(Alignment::CENTER)),
            10.0,
            10.0,
        );
        tagged_box("start", Some(GridItem::at(1, 2)), 10.0, 10.0);
    };
    let ltr = lay_out(LayoutDirection::Ltr, config.clone(), (100.0, 30.0), content);
    assert_eq!(position(&ltr, "centered"), (15.0, 10.0));
    assert_eq!(position(&ltr, "start"), (40.0, 0.0));

    let rtl = lay_out(LayoutDirection::Rtl, config, (100.0, 30.0), content);
    assert_eq!(position(&rtl, "centered"), (75.0, 10.0));
    assert_eq!(
        position(&rtl, "start"),
        (50.0, 0.0),
        "the second column is at the left, and its start at its right"
    );
}

#[test]
fn content_tracks_take_the_smallest_or_the_unbounded_size_of_their_children() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new().columns([GridTrackSize::MinContent, GridTrackSize::MaxContent]),
        (300.0, 300.0),
        || {
            two_words("narrow");
            two_words("wide");
        },
    );
    assert_eq!(size(&layout, "narrow"), (24.0, 20.0));
    assert_eq!(position(&layout, "wide"), (24.0, 0.0));
    assert_eq!(size(&layout, "wide"), (48.0, 10.0));
}

#[test]
fn auto_columns_grow_toward_their_unbounded_size_as_the_room_allows() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new().columns([GridTrackSize::Auto, GridTrackSize::Auto]),
        (60.0, 300.0),
        || {
            two_words("left");
            two_words("right");
        },
    );
    // Each starts at 24 and could grow by 24; the 12 left go 6 to each.
    assert_eq!(position(&layout, "right"), (30.0, 0.0));
    assert_eq!(size(&layout, "left"), (24.0, 20.0));
}

#[test]
fn a_spanning_child_grows_the_content_tracks_it_covers() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new().columns([GridTrackSize::Fixed(30.0), GridTrackSize::Auto]),
        (300.0, 300.0),
        || {
            tagged_box("wide", Some(GridItem::at(1, 1).span(1, 2)), 100.0, 20.0);
            tagged_box("small", Some(GridItem::at(2, 2)), 10.0, 20.0);
        },
    );
    assert_eq!(size(&layout, "wide"), (100.0, 20.0));
    assert_eq!(
        position(&layout, "small"),
        (30.0, 20.0),
        "the auto column grows to 70; the fixed one stays 30"
    );
}

#[test]
fn percentages_share_the_room_after_the_gaps() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        GridConfig::new()
            .columns([GridTrackSize::Percentage(0.5); 2])
            .gap(10.0),
        (110.0, 100.0),
        || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .content_description("first"),
                BoxSpec::default(),
                || {},
            );
            tagged_box("second", None, 10.0, 10.0);
        },
    );
    assert_eq!(size(&layout, "first").0, 50.0);
    assert_eq!(position(&layout, "second"), (60.0, 0.0));
}

#[test]
fn a_grid_reports_its_track_widths_as_its_intrinsic_width() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Box(
            Modifier::empty().width_intrinsic(IntrinsicSize::Max),
            BoxSpec::default(),
            || {
                Grid(
                    GridConfig::new()
                        .columns([GridTrackSize::Auto, GridTrackSize::Fixed(20.0)])
                        .gap(5.0),
                    Modifier::empty().content_description("grid"),
                    || {
                        Text("aaaa", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });
    set_text_measurer(ContractMeasurer);
    let layout = layout_composition(&mut composition, Size::new(300.0, 300.0));
    assert_eq!(size(&layout, "grid").0, 49.0);
}

#[test]
fn a_child_moves_when_its_cell_changes() {
    let row_handle = Rc::new(Cell::new(None));
    let output = Rc::clone(&row_handle);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let row =
            cranpose_core::remember(|| cranpose_core::mutableStateOf(1_i32)).with(|state| *state);
        output.set(Some(row));
        Grid(
            GridConfig::new()
                .columns([GridTrackSize::Fixed(40.0)])
                .rows([GridTrackSize::Fixed(30.0); 3]),
            Modifier::empty(),
            move || tagged_box("moving", Some(GridItem::at(row.value(), 1)), 10.0, 10.0),
        );
    });
    let row = row_handle.get().expect("row state");
    for (index, top) in [(1, 0.0), (3, 60.0), (2, 30.0)] {
        row.set_value(index);
        while composition.process_invalid_scopes().expect("row update") {}
        let layout = layout_composition(&mut composition, Size::new(100.0, 100.0));
        assert_eq!(position(&layout, "moving"), (0.0, top));
    }
}

/// A custom layout that puts each child at the x its [`Column`] data gives.
#[derive(Clone, PartialEq)]
struct ColumnsPolicy;

#[derive(Debug, PartialEq)]
struct Column(f32);

impl cranpose_ui_layout::MeasurePolicy for ColumnsPolicy {
    fn measure(
        &self,
        _scope: &dyn cranpose_ui_layout::MeasureScope,
        measurables: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        constraints: cranpose_ui::Constraints,
    ) -> cranpose_ui::MeasureResult {
        let placements = measurables
            .iter()
            .map(|measurable| {
                let x = measurable
                    .parent_data()
                    .layout_data::<Column>()
                    .map_or(0.0, |column| column.0);
                let placeable = measurable.measure(constraints.loosen());
                placeable.place(x, 0.0);
                cranpose_ui_layout::Placement::new(placeable.node_id(), x, 0.0, 0)
            })
            .collect();
        cranpose_ui::MeasureResult::new(
            Size::new(constraints.max_width, constraints.max_height),
            placements,
        )
    }

    fn min_intrinsic_width(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        0.0
    }

    fn max_intrinsic_width(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        0.0
    }

    fn min_intrinsic_height(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        0.0
    }

    fn max_intrinsic_height(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        0.0
    }
}

#[test]
fn a_custom_layout_reads_the_outermost_layout_data_of_its_children() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        cranpose_ui::Layout(Modifier::empty().fill_max_size(), ColumnsPolicy, || {
            Box(
                Modifier::empty()
                    .layout_data(Column(40.0))
                    .layout_data(Column(90.0))
                    .size_points(10.0, 10.0)
                    .content_description("outer wins"),
                BoxSpec::default(),
                || {},
            );
            tagged_box("no data", None, 10.0, 10.0);
        });
    });
    let layout = layout_composition(&mut composition, Size::new(200.0, 100.0));
    assert_eq!(position(&layout, "outer wins"), (40.0, 0.0));
    assert_eq!(position(&layout, "no data"), (0.0, 0.0));
}
