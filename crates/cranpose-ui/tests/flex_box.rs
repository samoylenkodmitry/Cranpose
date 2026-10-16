use std::rc::Rc;

use cranpose_ui::{
    Box, BoxSpec, IntrinsicSize, LayoutDirection, LayoutTree, Modifier, ProvideLayoutDirection,
    Size,
    widgets::{
        FlexAlignItems, FlexAlignSelf, FlexBasis, FlexBox, FlexBoxConfig, FlexConfig,
        FlexDirection, FlexJustifyContent, FlexWrap,
    },
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

/// A box the tests find by `tag`, with `modifier` before its tag.
fn tagged_box(tag: &'static str, modifier: Modifier) {
    Box(modifier.content_description(tag), BoxSpec::default(), || {});
}

fn flex(config: FlexConfig) -> Modifier {
    Modifier::empty().flex(config)
}

/// Lays `content` out in a box of `config` that fills a `width` by
/// `height` viewport.
fn lay_out(
    direction: LayoutDirection,
    config: FlexBoxConfig,
    (width, height): (f32, f32),
    content: impl Fn() + 'static,
) -> LayoutTree {
    let content = Rc::new(content);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let content = Rc::clone(&content);
        ProvideLayoutDirection(direction, move || {
            let content = Rc::clone(&content);
            FlexBox(Modifier::empty().fill_max_size(), config, move || content());
        });
    });
    layout_composition(&mut composition, Size::new(width, height))
}

fn rect(layout: &LayoutTree, tag: &str) -> (f32, f32, f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y, rect.width, rect.height)
}

#[test]
fn growing_children_share_the_room_a_line_leaves() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new(),
        (300.0, 100.0),
        || {
            tagged_box(
                "one",
                flex(FlexConfig::new().grow(1.0)).size_points(50.0, 20.0),
            );
            tagged_box(
                "two",
                flex(FlexConfig::new().grow(2.0)).size_points(50.0, 20.0),
            );
            tagged_box("fixed", Modifier::empty().size_points(50.0, 20.0));
        },
    );
    assert_eq!(rect(&layout, "one"), (0.0, 0.0, 100.0, 20.0));
    assert_eq!(rect(&layout, "two"), (100.0, 0.0, 150.0, 20.0));
    assert_eq!(rect(&layout, "fixed"), (250.0, 0.0, 50.0, 20.0));
}

#[test]
fn shrinking_children_give_up_room_in_proportion_to_shrink_times_basis() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new(),
        (100.0, 100.0),
        || {
            let basis = FlexConfig::new().basis(FlexBasis::Dp(80.0));
            tagged_box("light", flex(basis).height(20.0));
            tagged_box("heavy", flex(basis.shrink(3.0)).height(20.0));
        },
    );
    // 60 too long: the light child gives 80 / 320 of it, the heavy one the rest.
    assert_eq!(rect(&layout, "light"), (0.0, 0.0, 65.0, 20.0));
    assert_eq!(rect(&layout, "heavy"), (65.0, 0.0, 35.0, 20.0));
}

#[test]
fn wrapping_lines_justify_their_children_and_keep_the_gaps() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new()
            .wrap(FlexWrap::Wrap)
            .justify_content(FlexJustifyContent::SpaceBetween)
            .gap(5.0),
        (100.0, 100.0),
        || {
            for tag in ["a", "b", "c"] {
                tagged_box(tag, Modifier::empty().size_points(40.0, 20.0));
            }
        },
    );
    assert_eq!(rect(&layout, "a"), (0.0, 0.0, 40.0, 20.0));
    assert_eq!(rect(&layout, "b"), (60.0, 0.0, 40.0, 20.0));
    assert_eq!(
        rect(&layout, "c"),
        (0.0, 25.0, 40.0, 20.0),
        "a line of one child starts at the start"
    );
}

#[test]
fn children_align_across_the_line_and_stretch_to_it() {
    let centered = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new().align_items(FlexAlignItems::Center),
        (100.0, 60.0),
        || {
            tagged_box("short", Modifier::empty().size_points(20.0, 10.0));
            tagged_box("tall", Modifier::empty().size_points(20.0, 30.0));
            tagged_box(
                "end",
                flex(FlexConfig::new().align_self(FlexAlignSelf::End)).size_points(20.0, 10.0),
            );
        },
    );
    // One line takes the box's 60 across.
    assert_eq!(rect(&centered, "short").1, 25.0);
    assert_eq!(rect(&centered, "tall").1, 15.0);
    assert_eq!(rect(&centered, "end").1, 50.0);

    let stretched = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new().align_items(FlexAlignItems::Stretch),
        (100.0, 60.0),
        || {
            tagged_box("free", Modifier::empty().width(20.0));
            tagged_box(
                "start",
                flex(FlexConfig::new().align_self(FlexAlignSelf::Start)).size_points(20.0, 10.0),
            );
        },
    );
    assert_eq!(rect(&stretched, "free"), (0.0, 0.0, 20.0, 60.0));
    assert_eq!(rect(&stretched, "start"), (20.0, 0.0, 20.0, 10.0));
}

#[test]
fn reversed_rows_and_right_to_left_rows_start_at_the_right() {
    let content = || {
        tagged_box("first", Modifier::empty().size_points(20.0, 10.0));
        tagged_box("second", Modifier::empty().size_points(20.0, 10.0));
    };
    let reversed = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new().direction(FlexDirection::RowReverse),
        (100.0, 50.0),
        content,
    );
    assert_eq!(rect(&reversed, "first").0, 80.0);
    assert_eq!(rect(&reversed, "second").0, 60.0);

    let rtl = lay_out(
        LayoutDirection::Rtl,
        FlexBoxConfig::new(),
        (100.0, 50.0),
        content,
    );
    assert_eq!(rect(&rtl, "first").0, 80.0);
    assert_eq!(rect(&rtl, "second").0, 60.0);

    let column = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new()
            .direction(FlexDirection::Column)
            .justify_content(FlexJustifyContent::End),
        (100.0, 50.0),
        content,
    );
    assert_eq!(rect(&column, "first"), (0.0, 30.0, 20.0, 10.0));
    assert_eq!(rect(&column, "second"), (0.0, 40.0, 20.0, 10.0));
}

#[test]
fn order_moves_a_child_and_a_fill_modifier_grows_it() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new(),
        (200.0, 50.0),
        || {
            tagged_box("filler", Modifier::empty().fill_max_width().height(20.0));
            tagged_box(
                "first",
                flex(FlexConfig::new().order(-1)).size_points(50.0, 20.0),
            );
        },
    );
    assert_eq!(rect(&layout, "first"), (0.0, 0.0, 50.0, 20.0));
    assert_eq!(rect(&layout, "filler"), (50.0, 0.0, 150.0, 20.0));
}

#[test]
fn a_percent_basis_is_a_fraction_of_the_box() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new(),
        (200.0, 50.0),
        || {
            tagged_box(
                "quarter",
                flex(FlexConfig::new().basis(FlexBasis::Percent(0.25))).height(20.0),
            );
        },
    );
    assert_eq!(rect(&layout, "quarter").2, 50.0);
}

#[test]
fn a_flex_box_reports_one_line_or_its_widest_child_as_its_intrinsic_width() {
    for (wrap, intrinsic, width) in [
        (FlexWrap::NoWrap, IntrinsicSize::Max, 80.0),
        (FlexWrap::Wrap, IntrinsicSize::Min, 40.0),
    ] {
        let mut composition = cranpose_ui::run_test_composition(move || {
            Box(
                Modifier::empty().width_intrinsic(intrinsic),
                BoxSpec::default(),
                move || {
                    FlexBox(
                        Modifier::empty().content_description("box"),
                        FlexBoxConfig::new().wrap(wrap).gap(10.0),
                        || {
                            tagged_box("a", Modifier::empty().size_points(30.0, 10.0));
                            tagged_box("b", Modifier::empty().size_points(40.0, 10.0));
                        },
                    );
                },
            );
        });
        let layout = layout_composition(&mut composition, Size::new(300.0, 300.0));
        assert_eq!(rect(&layout, "box").2, width, "{wrap:?}");
    }
}

/// A child that grows by its fill fraction, untagged: a tag's semantics
/// differ on each composition and would update its whole chain. The tagged
/// child after it shows the room it took.
fn filling_and_growing(fraction: f32) {
    Box(
        Modifier::empty()
            .fill_max_width_fraction(fraction)
            .height(20.0),
        BoxSpec::default(),
        || {},
    );
    tagged_box(
        "growing",
        flex(FlexConfig::new().grow(1.0)).size_points(50.0, 20.0),
    );
}

#[test]
fn a_fill_fraction_that_changes_grows_its_child_as_a_fresh_layout_does() {
    let viewport = (300.0, 50.0);
    let state = Rc::new(std::cell::Cell::new(None));
    let slot = Rc::clone(&state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let slot = Rc::clone(&slot);
        FlexBox(
            Modifier::empty().fill_max_size(),
            FlexBoxConfig::new(),
            move || {
                let fraction = cranpose_core::rememberMutableStateOf(|| 0.25_f32);
                slot.set(Some(fraction));
                filling_and_growing(fraction.get());
            },
        );
    });
    layout_composition(&mut composition, Size::new(viewport.0, viewport.1));
    let fraction = state.get().expect("fraction state");
    for value in [1.0, 3.0, 0.25] {
        fraction.set(value);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, Size::new(viewport.0, viewport.1));
        let fresh = lay_out(
            LayoutDirection::Ltr,
            FlexBoxConfig::new(),
            viewport,
            move || filling_and_growing(value),
        );
        assert_eq!(
            rect(&layout, "growing"),
            rect(&fresh, "growing"),
            "at {value}"
        );
        let filling =
            |layout: &LayoutTree| layout.root().children[0].node_data.resolved_modifiers();
        assert_eq!(filling(&layout), filling(&fresh), "at {value}");
    }
}

#[test]
fn baseline_alignment_puts_children_without_a_baseline_on_their_bottoms() {
    let layout = lay_out(
        LayoutDirection::Ltr,
        FlexBoxConfig::new().align_items(FlexAlignItems::Baseline),
        (100.0, 60.0),
        || {
            tagged_box("short", Modifier::empty().size_points(20.0, 10.0));
            tagged_box("tall", Modifier::empty().size_points(20.0, 30.0));
        },
    );
    assert_eq!(rect(&layout, "short").1, 20.0);
    assert_eq!(rect(&layout, "tall").1, 0.0);
}
