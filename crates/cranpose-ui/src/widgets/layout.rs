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
            *last.borrow_mut() = modifier.clone();
        }
        changed
    })
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
/// Like Compose's, it is inline: a state its content reads recomposes the
/// composable that called it.
#[composable(no_skip)]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    compose_layout(
        modifier,
        &measure_policy,
        LayoutEnvironment::current(),
        content,
    )
}

/// Emits a layout node in the calling composable's group, so a widget that
/// is one layout composes in one group rather than its own and `Layout`'s.
pub(crate) fn compose_layout<F, P>(
    modifier: Modifier,
    measure_policy: &P,
    environment: LayoutEnvironment,
    mut content: F,
) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq,
{
    // One composer lookup for the node and its children's bracket: each
    // lookup reads a thread-local.
    cranpose_core::with_current_composer(|composer| {
        let mut modifier = modifier;
        let (id, provided, _) =
            emit_layout_node(composer, &mut modifier, measure_policy, environment);
        composer.push_parent(id);
        compose_under_locals(provided, &mut content);
        composer.pop_parent();
        id
    })
}

pub(crate) fn compose_empty_layout(
    mut modifier: impl NodeModifier,
    environment: LayoutEnvironment,
) -> NodeId {
    cranpose_core::with_current_composer(|composer| {
        let (id, provided, holds_children) = emit_layout_node(
            composer,
            &mut modifier,
            &crate::layout::policies::EmptyMeasurePolicy,
            environment,
        );
        if provided.is_empty() {
            composer.compose_no_children(id, holds_children);
        } else {
            composer.push_parent(id);
            compose_under_locals(provided, &mut || {});
            composer.pop_parent();
        }
        id
    })
}

// The steps of emitting a layout node that know its policy's type;
// `emit_layout_node` takes the rest, compiled once for every policy type.
trait PolicyUpdate {
    // A node with `modifier`, measured by the policy.
    fn new_node(&self, modifier: Modifier) -> LayoutNode;
    // Gives `node` the policy, unless the one it holds equals it.
    fn update(&self, node: &mut LayoutNode);
}

impl<P: MeasurePolicy + Clone + PartialEq> PolicyUpdate for P {
    fn new_node(&self, modifier: Modifier) -> LayoutNode {
        LayoutNode::new(modifier, held_policy(self))
    }

    fn update(&self, node: &mut LayoutNode) {
        let held = {
            let runtime_state = node.layout_runtime_state().borrow();
            let current: &dyn std::any::Any = runtime_state.measure_policy();
            current.downcast_ref::<P>() == Some(self)
        };
        if !held {
            node.set_measure_policy(held_policy(self));
        }
    }
}

// The thread's one empty policy, which every leaf shares; a held immutable
// policy equal to `policy`; or else `policy` in an allocation of its own.
fn held_policy<P: MeasurePolicy + Clone + PartialEq>(policy: &P) -> Rc<dyn MeasurePolicy> {
    if std::any::TypeId::of::<P>()
        == std::any::TypeId::of::<crate::layout::policies::EmptyMeasurePolicy>()
    {
        return crate::widgets::nodes::layout_node::empty_measure_policy();
    }
    if policy.is_immutable() {
        return shared_policy(
            &|held: &dyn std::any::Any| held.downcast_ref::<P>() == Some(policy),
            &|| Rc::new(policy.clone()),
        );
    }
    Rc::new(policy.clone())
}

// The held immutable policy `equal` finds, or the one `make` makes. One body
// for every policy type: a generic body that reads the thread's state takes
// the state's first-read code into every crate that compiles it.
#[inline(never)]
fn shared_policy(
    equal: &dyn Fn(&dyn std::any::Any) -> bool,
    make: &dyn Fn() -> Rc<dyn MeasurePolicy>,
) -> Rc<dyn MeasurePolicy> {
    crate::thread_state::with(|state| state.shared_policies.borrow_mut().shared(equal, make))
}

// What a layout node's modifier is set from: a modifier, or what makes one
// only when the node does not hold it already.
pub(crate) trait NodeModifier {
    // The composition locals the modifier provides.
    fn provided_locals(&self) -> Vec<cranpose_core::ProvidedValue>;
    // The modifier, for a node made new.
    fn to_modifier(&self) -> Modifier;
    // Sets the modifier on `node`; a modifier may give its value to the node
    // and stay empty.
    fn set_on(&mut self, node: &mut LayoutNode);
}

impl NodeModifier for Modifier {
    fn provided_locals(&self) -> Vec<cranpose_core::ProvidedValue> {
        self.provided_composition_locals()
    }

    fn to_modifier(&self) -> Modifier {
        self.clone()
    }

    fn set_on(&mut self, node: &mut LayoutNode) {
        node.set_modifier(std::mem::take(self));
    }
}

// Emits and updates a layout node; returns it, the composition locals its
// modifier provides and whether it holds children. The node keeps the policy
// it holds while `policy` equals it. Out of line, one function for every
// policy and modifier.
#[inline(never)]
fn emit_layout_node(
    composer: &cranpose_core::Composer,
    modifier: &mut dyn NodeModifier,
    policy: &dyn PolicyUpdate,
    environment: LayoutEnvironment,
) -> (NodeId, Vec<cranpose_core::ProvidedValue>, bool) {
    let provided = modifier.provided_locals();
    let id = composer.emit_recyclable_node(
        || policy.new_node(modifier.to_modifier()),
        |node| *node = policy.new_node(modifier.to_modifier()),
    );
    let holds_children = composer
        .with_node_mut(id, |node: &mut LayoutNode| {
            modifier.set_on(node);
            policy.update(node);
            node.set_density(environment.density);
            node.set_layout_direction(environment.direction);
            !node.children.is_empty()
        })
        .unwrap_or_else(|err| {
            debug_assert!(false, "failed to update Layout node: {err}");
            true
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
            .with(|cell| Rc::clone(cell));
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
        let policy_cell = Rc::clone(&policy_cell);
        let policy: Rc<SubcomposeMeasurePolicy> =
            Rc::new(
                move |scope, constraints| match policy_cell.borrow().as_ref().cloned() {
                    Some(current) => current(scope, constraints),
                    None => empty_subcompose_measure_result(constraints),
                },
            );
        policy
    })
    .with(|policy| Rc::clone(policy));
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
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0.0));
        }

        width = width.clamp(constraints.min_width, constraints.max_width);
        height = height.clamp(constraints.min_height, constraints.max_height);
        scope.layout(width, height, placements)
    })
}

#[cfg(test)]
#[path = "tests/layout_tests.rs"]
mod tests;
