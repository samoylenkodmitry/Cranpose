use std::rc::Rc;

use cranpose_core::{Applier, MemoryApplier, NodeError};

use super::*;
use crate::{
    LayoutNode, Point, Rect, build_layout_tree_from_applier,
    layout::policies::{BoxMeasurePolicy, LeafMeasurePolicy},
    measure_layout,
};

fn range(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Constraints {
    Constraints {
        min_width,
        max_width,
        min_height,
        max_height,
    }
}

#[test]
fn a_wrapped_axis_drops_its_minimum_and_an_unbounded_one_its_maximum() {
    let tight = range(100.0, 100.0, 50.0, 50.0);
    assert_eq!(
        wrap_content_constraints(WrapAxes::Horizontal, false, tight),
        range(0.0, 100.0, 50.0, 50.0)
    );
    assert_eq!(
        wrap_content_constraints(WrapAxes::Vertical, true, tight),
        range(100.0, 100.0, 0.0, f32::INFINITY)
    );
    assert_eq!(
        wrap_content_constraints(WrapAxes::Both, true, tight),
        range(0.0, f32::INFINITY, 0.0, f32::INFINITY)
    );
}

#[test]
fn small_content_sits_at_its_alignment_in_the_minimum_the_parent_sets() {
    let placed = wrap_content_placement(
        WrapAxes::Both,
        Alignment::BOTTOM_END,
        range(100.0, 100.0, 50.0, 50.0),
        Size {
            width: 20.0,
            height: 10.0,
        },
        1.0,
    );
    assert_eq!((placed.size.width, placed.size.height), (100.0, 50.0));
    assert_eq!(
        (placed.placement_offset_x, placed.placement_offset_y),
        (80.0, 40.0)
    );
}

#[test]
fn larger_content_is_centred_outside_the_wrapper_as_compose_centres_it() {
    let placed = wrap_content_placement(
        WrapAxes::Both,
        Alignment::CENTER,
        range(0.0, 100.0, 0.0, 50.0),
        Size {
            width: 300.0,
            height: 200.0,
        },
        1.0,
    );
    assert_eq!((placed.size.width, placed.size.height), (100.0, 50.0));
    assert_eq!(
        (placed.placement_offset_x, placed.placement_offset_y),
        (-100.0, -75.0)
    );
}

#[test]
fn an_axis_that_does_not_wrap_is_not_moved() {
    let placed = wrap_content_placement(
        WrapAxes::Horizontal,
        Alignment::BOTTOM_END,
        range(100.0, 100.0, 50.0, 50.0),
        Size {
            width: 20.0,
            height: 50.0,
        },
        1.0,
    );
    assert_eq!(
        (placed.placement_offset_x, placed.placement_offset_y),
        (80.0, 0.0)
    );
}

#[test]
fn an_offset_lands_on_the_device_pixel_grid() {
    let placed = wrap_content_placement(
        WrapAxes::Horizontal,
        Alignment::CENTER,
        range(101.0, 101.0, 0.0, 10.0),
        Size {
            width: 20.0,
            height: 10.0,
        },
        1.0,
    );
    assert_eq!(
        placed.placement_offset_x, 41.0,
        "half of 81 rounds up to a whole pixel, as Compose's roundToInt does"
    );
}

/// A 300 by 200 leaf under `modifier` in a 100 by 50 box: the leaf's box in
/// the layout tree, and where its content starts inside it.
fn leaf_in_small_box(modifier: Modifier) -> Result<(Rect, Point), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut applier = MemoryApplier::new();
    let leaf = applier.create(Box::new(LayoutNode::new(
        modifier,
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 300.0,
            height: 200.0,
        })),
    )));
    let mut root = LayoutNode::new(
        Modifier::empty().size_points(100.0, 50.0),
        Rc::new(BoxMeasurePolicy::new(Alignment::TOP_START, false)),
    );
    root.children.push(leaf);
    let root = applier.create(Box::new(root));
    measure_layout(
        &mut applier,
        root,
        Size {
            width: 400.0,
            height: 400.0,
        },
    )?;
    let tree = build_layout_tree_from_applier(&mut applier, root)?.expect("layout");
    let leaf = tree.root().children.first().expect("the leaf");
    Ok((leaf.rect, leaf.content_offset))
}

#[test]
fn an_unbounded_canvas_keeps_its_own_size_in_a_smaller_parent() -> Result<(), NodeError> {
    let (rect, content) =
        leaf_in_small_box(Modifier::empty().wrap_content_size(Alignment::CENTER, true))?;
    assert_eq!(
        (rect.width, rect.height),
        (100.0, 50.0),
        "the wrapper takes the parent's size"
    );
    assert_eq!(
        (content.x, content.y),
        (-100.0, -75.0),
        "the leaf keeps its 300 by 200 and is centred over the wrapper, as in Compose"
    );
    let (_, bounded) =
        leaf_in_small_box(Modifier::empty().wrap_content_size(Alignment::CENTER, false))?;
    assert_eq!(
        (bounded.x, bounded.y),
        (0.0, 0.0),
        "bounded, the parent's maximum cuts the leaf to the wrapper"
    );
    Ok(())
}
