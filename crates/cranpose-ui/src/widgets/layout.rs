//! Generic Layout widget and SubcomposeLayout

#![allow(non_snake_case)]

use super::nodes::LayoutNode;
use super::scopes::BoxWithConstraintsScopeImpl;
use crate::composable;
use crate::modifier::Modifier;
use crate::subcompose_layout::{
    Constraints, MeasurePolicy as SubcomposeMeasurePolicy, MeasureResult, SubcomposeLayoutNode,
    SubcomposeLayoutScope, SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
use cranpose_core::{NodeId, SlotId};
use cranpose_ui_layout::{MeasurePolicy, Placement};
use std::cell::RefCell;
use std::rc::Rc;

#[composable]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    let policy: Rc<dyn MeasurePolicy> = Rc::new(measure_policy);
    let modifier_for_reset = modifier.clone();
    let policy_for_reset = Rc::clone(&policy);
    let id = cranpose_core::with_current_composer(|composer| {
        composer.emit_recyclable_node(
            || LayoutNode::new(modifier.clone(), Rc::clone(&policy)),
            move |node| {
                *node = LayoutNode::new(modifier_for_reset.clone(), Rc::clone(&policy_for_reset));
            },
        )
    });
    if let Err(err) = cranpose_core::with_node_mut(id, |node: &mut LayoutNode| {
        node.set_modifier(modifier.clone());
        node.set_measure_policy(Rc::clone(&policy));
    }) {
        debug_assert!(false, "failed to update Layout node: {err}");
    }
    cranpose_core::push_parent(id);
    content();
    cranpose_core::pop_parent();
    id
}

#[composable]
pub fn SubcomposeLayout(
    modifier: Modifier,
    measure_policy: impl for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult
        + 'static,
) -> NodeId {
    let policy: Rc<SubcomposeMeasurePolicy> = Rc::new(measure_policy);
    let id = cranpose_core::with_current_composer(|composer| {
        composer.emit_node(|| SubcomposeLayoutNode::new(modifier.clone(), Rc::clone(&policy)))
    });
    if let Err(err) = cranpose_core::with_node_mut(id, |node: &mut SubcomposeLayoutNode| {
        node.set_modifier(modifier.clone());
        node.set_measure_policy(Rc::clone(&policy));
    }) {
        debug_assert!(false, "failed to update SubcomposeLayout node: {err}");
    }
    id
}

#[composable(no_skip)]
pub fn BoxWithConstraints<F>(modifier: Modifier, content: F) -> NodeId
where
    F: FnMut(BoxWithConstraintsScopeImpl) + 'static,
{
    let content_ref: Rc<RefCell<F>> = Rc::new(RefCell::new(content));
    SubcomposeLayout(modifier, move |scope, constraints| {
        let scope_impl = BoxWithConstraintsScopeImpl::new(constraints);
        let scope_for_content = scope_impl;
        let measurables = {
            let content_ref = Rc::clone(&content_ref);
            scope.subcompose(SlotId::new(0), move || {
                let mut content = content_ref.borrow_mut();
                content(scope_for_content);
            })
        };
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };

        let mut width = 0.0_f32;
        let mut height = 0.0_f32;
        let mut placements = Vec::with_capacity(measurables.len());

        for measurable in measurables {
            let placeable = scope.measure(measurable, child_constraints);
            width = width.max(placeable.width());
            height = height.max(placeable.height());
            placeable.place(0.0, 0.0);
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }

        width = width.clamp(constraints.min_width, constraints.max_width);
        height = height.clamp(constraints.min_height, constraints.max_height);
        scope.layout(width, height, placements)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_core::{location_key, Composition, MemoryApplier, MutableState};
    use std::cell::Cell;

    #[test]
    fn layout_recomposes_when_content_reads_state() {
        thread_local! {
            static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
        }

        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0_i32, runtime);

        composition
            .render(location_key(file!(), line!(), column!()), {
                let observed_state = state;
                move || {
                    Layout(
                        Modifier::empty(),
                        crate::layout::policies::EmptyMeasurePolicy,
                        {
                            let observed_state = observed_state;
                            move || {
                                let _ = observed_state.value();
                                INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
                            }
                        },
                    );
                }
            })
            .expect("initial layout render");

        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        state.set_value(1);
        composition
            .process_invalid_scopes()
            .expect("layout content recomposition");

        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
    }
}
