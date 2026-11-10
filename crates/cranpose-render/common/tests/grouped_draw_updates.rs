// A node whose draws change records each draw once per update, also when
// its draws no longer fit the items it drew before: a later draw opens a
// group that a scope transform makes, a later draw draws around the
// content, or the draw itself opens a group from this update on; as a leaf
// and as a layer that holds a child.

use std::{cell::Cell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_render_common::NodeUpdate;
use cranpose_ui::{Box, BoxSpec, Brush, Color, Modifier, Size, run_test_composition};
use cranpose_ui_graphics::DrawScope;

use crate::scene_probe::{initial_graph, solid_rect_colors, with_applier};

const VIEWPORT: Size = Size::new(80.0, 60.0);

// What follows the counted draw, or how the counted draw changes.
#[derive(Clone, Copy)]
enum Case {
    GroupedAfter,
    AroundContentAfter,
    GroupsFromTheUpdate,
}

fn assert_draw_records_once_per_update(case: Case, with_child: bool) {
    let calls = Rc::new(Cell::new(0));
    let color = Rc::new(Cell::new(Color::BLUE));
    let grouped = Rc::new(Cell::new(false));
    let node_slot = Rc::new(Cell::new(None::<NodeId>));
    let (draw_calls, draw_color, draw_grouped, node_id) = (
        Rc::clone(&calls),
        Rc::clone(&color),
        Rc::clone(&grouped),
        Rc::clone(&node_slot),
    );
    let mut composition = run_test_composition(move || {
        let (draw_calls, draw_color, draw_grouped) = (
            Rc::clone(&draw_calls),
            Rc::clone(&draw_color),
            Rc::clone(&draw_grouped),
        );
        let counted =
            Modifier::empty()
                .size(VIEWPORT)
                .draw_behind(move |scope: &mut dyn DrawScope| {
                    draw_calls.set(draw_calls.get() + 1);
                    let mut rect = |scope: &mut dyn DrawScope| {
                        scope.draw_rect(Brush::solid(draw_color.get()));
                    };
                    if draw_grouped.get() {
                        scope.translate(0.0, 0.0, &mut rect);
                    } else {
                        rect(scope);
                    }
                });
        let modifier = match case {
            Case::GroupedAfter => counted.draw_behind(|scope: &mut dyn DrawScope| {
                scope.translate(2.0, 1.0, &mut |scope: &mut dyn DrawScope| {
                    scope.draw_rect(Brush::solid(Color::RED));
                });
            }),
            Case::AroundContentAfter => counted.draw_with_content(|scope: &mut dyn DrawScope| {
                scope.draw_content();
                scope.draw_rect(Brush::solid(Color::RED));
            }),
            Case::GroupsFromTheUpdate => counted.draw_behind(|scope: &mut dyn DrawScope| {
                scope.draw_rect(Brush::solid(Color::RED));
            }),
        };
        let node = Box(modifier, BoxSpec::default(), move || {
            if with_child {
                Box(
                    Modifier::empty().size(Size::new(10.0, 10.0)),
                    BoxSpec::default(),
                    || {},
                );
            }
        });
        node_id.set(Some(node));
    });
    let root = composition.root().expect("a root");
    let node = node_slot.get().expect("the drawing node");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    assert_eq!(calls.get(), 1, "the first scene records the draw once");

    color.set(Color::GREEN);
    grouped.set(matches!(case, Case::GroupsFromTheUpdate));
    with_applier(&mut composition, |applier| {
        assert!(
            scene.update_from_applier(applier, root, &[(node, NodeUpdate::Draws)]),
            "the update applies"
        );
    });

    assert_eq!(calls.get(), 2, "one update records the draw once");
    assert_eq!(solid_rect_colors(&scene), [Color::GREEN, Color::RED]);
}

#[test]
fn a_draw_before_a_grouped_draw_records_once_per_update() {
    assert_draw_records_once_per_update(Case::GroupedAfter, false);
    assert_draw_records_once_per_update(Case::GroupedAfter, true);
}

#[test]
fn a_draw_before_a_draw_around_the_content_records_once_per_update() {
    assert_draw_records_once_per_update(Case::AroundContentAfter, false);
    assert_draw_records_once_per_update(Case::AroundContentAfter, true);
}

#[test]
fn a_draw_that_opens_a_group_from_an_update_on_records_once_in_it() {
    assert_draw_records_once_per_update(Case::GroupsFromTheUpdate, false);
    assert_draw_records_once_per_update(Case::GroupsFromTheUpdate, true);
}
