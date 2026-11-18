// An item's roots measured into the roots of an item the list let go of.

use std::{any::Any, rc::Rc};

use cranpose_foundation::lazy::{LazyItemRoots, SmallNodeVec};

use super::{LazyItemMeasureContext, RootMeasurements, lazy_list_child_constraints};
use crate::{layout::MeasuredNode, modifier::Size, subcompose_layout::SubcomposeMeasureScopeImpl};

// Measures `node_ids`, the roots of the item of `context`, into `room`, the
// roots of an item the list let go of this pass, or into new roots where it
// has none. Returns the roots and the item's extent along the list's axis and
// across it.
pub(super) fn measure_item_roots(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    node_ids: SmallNodeVec,
    room: Option<Rc<LazyItemRoots>>,
    context: &LazyItemMeasureContext,
) -> (Rc<LazyItemRoots>, f32, f32) {
    let constraints = lazy_list_child_constraints(context.axis, context.cross_axis_size);
    let mut roots = room.unwrap_or_default();
    let parts = Rc::get_mut(&mut roots).expect("a room or new roots have no other holder");
    parts.node_ids = node_ids;
    parts.child_offsets.clear();
    parts.child_cross_axis_sizes.clear();
    let mut measurements: Rc<RootMeasurements> = parts
        .measurements
        .take()
        .and_then(|kept| kept.downcast().ok())
        .unwrap_or_default();
    let measured = Rc::get_mut(&mut measurements)
        .expect("the measurements of sole roots have no other holder");
    measured.clear();

    let (mut main_size, mut cross_size) = (0.0_f32, 0.0_f32);
    for &node_id in &parts.node_ids {
        let (placeable, retained) = scope.measure_retained(node_id, constraints);
        let size = retained.as_ref().map_or_else(
            || Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            MeasuredNode::size,
        );
        let (main, cross) = if context.axis.is_vertical() {
            (size.height, size.width)
        } else {
            (size.width, size.height)
        };
        parts.child_offsets.push(main_size);
        parts.child_cross_axis_sizes.push(cross);
        if let Some(retained) = retained {
            measured.push(retained);
        }
        main_size += main;
        cross_size = cross_size.max(cross);
    }

    let complete = measured.len() == parts.node_ids.len();
    parts.measurements = complete.then_some(measurements as Rc<dyn Any>);
    (roots, main_size, cross_size)
}
