//! Generic Layout widget and SubcomposeLayout

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{NodeId, SlotId};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{MeasurePolicy, Placement};

use super::{nodes::LayoutNode, scopes::BoxWithConstraintsScopeImpl};
use crate::{
    composable,
    modifier::Modifier,
    subcompose_layout::{
        Constraints, MeasurePolicy as SubcomposeMeasurePolicy, MeasureResult, SubcomposeLayoutNode,
        SubcomposeLayoutScope, SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
    },
};

pub(crate) fn caller_modifier_changed(modifier: &Modifier) -> bool {
    cranpose_core::remember(|| RefCell::new(modifier.clone())).with(|last| {
        let changed = *last.borrow() != *modifier;
        if changed {
            last.replace(modifier.clone());
        }
        changed
    })
}

struct RetainedMeasurePolicy<P> {
    value: P,
    policy: Rc<dyn MeasurePolicy>,
}

#[composable]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    compose_layout(modifier, measure_policy, content)
}

/// Emits a layout node in the calling composable's group, so a widget that
/// is one layout composes in one group rather than its own and `Layout`'s.
pub(crate) fn compose_layout<F, P>(modifier: Modifier, measure_policy: P, mut content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    let policy_holder = cranpose_core::remember({
        let measure_policy = measure_policy.clone();
        move || {
            Rc::new(RefCell::new(RetainedMeasurePolicy {
                value: measure_policy.clone(),
                policy: Rc::new(measure_policy),
            }))
        }
    })
    .with(Rc::clone);
    let policy = {
        let mut holder = policy_holder.borrow_mut();
        if holder.value != measure_policy {
            holder.value = measure_policy.clone();
            holder.policy = Rc::new(measure_policy);
        }
        Rc::clone(&holder.policy)
    };
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
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(id, |node: &mut LayoutNode| {
        node.set_modifier(modifier.clone());
        node.set_measure_policy(Rc::clone(&policy));
        node.set_density(composed_density);
    }) {
        debug_assert!(false, "failed to update Layout node: {err}");
    }
    cranpose_core::push_parent(id);
    compose_under_modifier_locals(&modifier, &mut content);
    cranpose_core::pop_parent();
    id
}

fn compose_under_modifier_locals(modifier: &Modifier, content: &mut dyn FnMut()) {
    let provided = modifier.provided_composition_locals();
    if provided.is_empty() {
        content();
        return;
    }
    cranpose_core::CompositionLocalProvider(provided, content);
}

#[composable]
pub fn SubcomposeLayout(
    modifier: Modifier,
    measure_policy: impl for<'scope> Fn(
        &mut SubcomposeMeasureScopeImpl<'scope>,
        Constraints,
    ) -> MeasureResult
    + 'static,
) -> NodeId {
    cranpose_core::debug_label_current_scope("SubcomposeLayout");
    let policy_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(None::<Rc<SubcomposeMeasurePolicy>>)))
            .with(|cell| cell.clone());
    let current_policy: Rc<SubcomposeMeasurePolicy> = Rc::new(measure_policy);
    let policy_captures_changed = {
        let mut policy_cell_ref = policy_cell.borrow_mut();
        let changed = policy_cell_ref
            .as_ref()
            .is_none_or(|previous| !Rc::ptr_eq(previous, &current_policy));
        *policy_cell_ref = Some(current_policy);
        changed
    };
    let policy: Rc<SubcomposeMeasurePolicy> = cranpose_core::remember(move || {
        let policy_cell = policy_cell.clone();
        let policy: Rc<SubcomposeMeasurePolicy> =
            Rc::new(
                move |scope, constraints| match policy_cell.borrow().as_ref().cloned() {
                    Some(current) => current(scope, constraints),
                    None => empty_subcompose_measure_result(constraints),
                },
            );
        policy
    })
    .with(|policy| policy.clone());
    let id = cranpose_core::with_current_composer(|composer| {
        composer.emit_node(|| SubcomposeLayoutNode::new(modifier.clone(), Rc::clone(&policy)))
    });
    let captured_context =
        cranpose_core::with_current_composer(|composer| composer.capture_composition_context());
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(id, |node: &mut SubcomposeLayoutNode| {
        node.set_modifier(modifier.clone());
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context.clone());
        node.set_density(composed_density);
        if policy_captures_changed {
            node.invalidate_subcomposition();
        }
    }) {
        debug_assert!(false, "failed to update SubcomposeLayout node: {err}");
    }
    id
}

fn empty_subcompose_measure_result(constraints: Constraints) -> MeasureResult {
    let (width, height) = constraints.constrain(0.0, 0.0);
    MeasureResult::new(Size { width, height }, Vec::new())
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
            scope.subcompose(SlotId::new(0), constraints, move || {
                cranpose_core::debug_label_current_scope("BoxWithConstraints.slot(0)");
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
#[path = "tests/layout_tests.rs"]
mod tests;
