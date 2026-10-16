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

/// The density and layout direction a layout node composes under. A widget
/// reads them once, for its measure policy and its node: each read looks a
/// composition local up.
#[derive(Clone, Copy)]
pub(crate) struct LayoutEnvironment {
    pub(crate) density: crate::density::Density,
    pub(crate) direction: crate::layout_direction::LayoutDirection,
}

impl LayoutEnvironment {
    /// The density and layout direction in force here.
    pub(crate) fn current() -> Self {
        Self {
            density: crate::density::density(),
            direction: crate::layout_direction(),
        }
    }
}

/// Emits a layout node whose children `measure_policy` measures and places. `content` composes the
/// children. Compose's `Layout`.
#[composable]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    compose_layout(
        modifier,
        measure_policy,
        LayoutEnvironment::current(),
        content,
    )
}

/// Emits a layout node in the calling composable's group, so a widget that
/// is one layout composes in one group rather than its own and `Layout`'s.
pub(crate) fn compose_layout<F, P>(
    modifier: Modifier,
    measure_policy: P,
    environment: LayoutEnvironment,
    content: F,
) -> NodeId
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
    emit_layout(modifier, policy, environment, content)
}

pub(crate) fn compose_empty_layout(modifier: Modifier, environment: LayoutEnvironment) -> NodeId {
    let (id, provided, holds_children) = emit_layout_node(
        modifier,
        super::nodes::layout_node::empty_measure_policy(),
        environment,
    );
    if provided.is_empty() {
        cranpose_core::with_current_composer(|composer| {
            composer.compose_no_children(id, holds_children);
        });
    } else {
        cranpose_core::push_parent(id);
        compose_under_locals(provided, &mut || {});
        cranpose_core::pop_parent();
    }
    id
}

fn emit_layout<F>(
    modifier: Modifier,
    policy: Rc<dyn MeasurePolicy>,
    environment: LayoutEnvironment,
    mut content: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    let (id, provided, _) = emit_layout_node(modifier, policy, environment);
    cranpose_core::push_parent(id);
    compose_under_locals(provided, &mut content);
    cranpose_core::pop_parent();
    id
}

/// Emits and updates a layout node; returns it, the composition locals its
/// modifier provides and whether it holds children.
fn emit_layout_node(
    modifier: Modifier,
    policy: Rc<dyn MeasurePolicy>,
    environment: LayoutEnvironment,
) -> (NodeId, Vec<cranpose_core::ProvidedValue>, bool) {
    let provided = modifier.provided_composition_locals();
    let (id, holds_children) = cranpose_core::with_current_composer(|composer| {
        let id = composer.emit_recyclable_node(
            || LayoutNode::new(modifier.clone(), Rc::clone(&policy)),
            |node| *node = LayoutNode::new(modifier.clone(), Rc::clone(&policy)),
        );
        let holds_children = composer
            .with_node_mut(id, |node: &mut LayoutNode| {
                node.set_modifier(modifier);
                node.set_measure_policy(policy);
                node.set_density(environment.density);
                node.set_layout_direction(environment.direction);
                !node.children.is_empty()
            })
            .unwrap_or_else(|err| {
                debug_assert!(false, "failed to update Layout node: {err}");
                true
            });
        (id, holds_children)
    });
    (id, provided, holds_children)
}

fn compose_under_locals(provided: Vec<cranpose_core::ProvidedValue>, content: &mut dyn FnMut()) {
    if provided.is_empty() {
        content();
        return;
    }
    cranpose_core::CompositionLocalProvider(provided, content);
}

/// Emits a layout node whose `measure_policy` composes its children during measurement, so the
/// children can depend on the incoming constraints. Compose's `SubcomposeLayout`.
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
    let direction = crate::layout_direction();
    if let Err(err) = cranpose_core::with_node_mut(id, |node: &mut SubcomposeLayoutNode| {
        node.set_modifier(modifier);
        node.set_measure_policy(policy);
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        node.set_layout_direction(direction);
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

/// Composes `content` with a scope that exposes the incoming constraints. All children are placed
/// at the top start corner. Compose's `BoxWithConstraints`.
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
