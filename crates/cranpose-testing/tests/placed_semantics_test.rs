//! The join between semantics and geometry, checked on the widget it exists for.
//!
//! A Wear scaling list is the case that separates the two boxes a
//! [`PlacedSemanticsNode`] carries: every row is measured at the same size and
//! the ones away from the centre line are DRAWN — and therefore hit — smaller.
//! A harness that reported one box for both would be indistinguishable from a
//! correct one on any other widget, and useless on this one.

use cranpose_foundation::lazy::LazyItems;
use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode};
use cranpose_ui::{
    Modifier, Size,
    round_scaling_list::CentreAnchor,
    widgets::{
        BoxSpec,
        wear::{WearScalingLazyColumn, WearScalingLazyColumnSpec, rememberWearScalingListState},
    },
};

/// The watch these widgets are dimensioned for, in layout points.
const WATCH: f32 = 454.0;
/// Every row's height, stated rather than text-measured so what is asserted
/// here is the ramp's geometry and not a font's.
const ROW: f32 = 52.0;
const ROWS: usize = 6;

fn tappable_list(size: f32) -> PlacedSemanticsNode {
    let mut rule = ComposeTestRule::new();
    rule.set_content(move || {
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            move |scope| {
                scope.items(
                    LazyItems::new(ROWS).key(|index: usize| index as u64),
                    move |index| {
                        cranpose_ui::widgets::Box(
                            Modifier::empty()
                                .fill_max_width()
                                .height(ROW)
                                .clickable(move |_| {
                                    let _ = index;
                                })
                                .semantics(move |config| {
                                    config.content_description = Some(format!("row {index}"));
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    })
    .expect("the list renders");
    rule.placed_semantics(Size::new(size, size))
        .expect("the list lays out")
        .expect("the list places something")
}

#[test]
fn every_row_reaches_the_tree_with_a_box_and_a_description() {
    let root = tappable_list(WATCH);
    let controls = root.controls();
    let labels: Vec<Option<&str>> = controls.iter().map(|node| node.label.as_deref()).collect();
    assert_eq!(
        labels,
        (0..ROWS)
            .map(|index| format!("row {index}"))
            .collect::<Vec<_>>()
            .iter()
            .map(|label| Some(label.as_str()))
            .collect::<Vec<_>>(),
        "every row is published, in tree order, with the words a reader gets"
    );
    for node in &controls {
        assert_eq!(
            node.layout_bounds.height,
            ROW,
            "{} was measured at the height it asked for",
            node.describe()
        );
        assert!(
            node.touch_bounds.is_some(),
            "{} is clickable, so the hit graph carries a region for it",
            node.describe()
        );
    }
}

#[test]
fn a_row_away_from_the_centre_line_is_drawn_smaller_than_it_was_measured() {
    // The claim the two boxes exist for. `layout_bounds` is what the row asked
    // for and gets, everywhere on the list; `touch_bounds` is the quad the
    // renderer draws and the hit test inverts, which the scaling ramp shrinks.
    // Reading only the first would report a 52pt target for a row that is 38pt
    // of screen.
    let root = tappable_list(WATCH);
    let controls = root.controls();
    let shrunk: Vec<f32> = controls
        .iter()
        .map(|node| node.target_bounds().height / node.layout_bounds.height)
        .collect();
    assert!(
        shrunk.iter().any(|scale| *scale > 0.999),
        "the anchored row sits on the centre line at full size: {shrunk:?}"
    );
    assert!(
        shrunk.iter().any(|scale| *scale < 0.8),
        "a row at the rim is drawn well under the box it was measured in, and \
         that is what the second box is for: {shrunk:?}"
    );
    for (node, scale) in controls.iter().zip(&shrunk) {
        assert!(
            *scale <= 1.001,
            "the ramp never grows a row: {} came back at {scale}",
            node.describe()
        );
    }
}

#[test]
fn a_node_that_is_not_interactive_has_no_touch_box() {
    // The distinction the whole audit turns on: `touch_bounds` is `None` for a
    // node the hit graph carries no region for, so a caller cannot mistake a
    // label for a control that merely happens to be small.
    let root = tappable_list(WATCH);
    let non_interactive: Vec<&PlacedSemanticsNode> = root
        .flatten()
        .into_iter()
        .filter(|node| !node.interactive)
        .collect();
    assert!(
        !non_interactive.is_empty(),
        "a list has non-interactive structure around its rows"
    );
    assert!(
        non_interactive
            .iter()
            .all(|node| node.touch_bounds.is_none()),
        "nothing that cannot receive pointer input carries a touch box"
    );
}
