//! Generic Layout widget and SubcomposeLayout

#![allow(non_snake_case)]

use super::nodes::LayoutNode;
use super::scopes::{BoxWithConstraintsScope, BoxWithConstraintsScopeImpl};
use crate::composable;
use crate::modifier::Modifier;
use crate::subcompose_layout::{
    Constraints, MeasurePolicy as SubcomposeMeasurePolicy, MeasureResult, SubcomposeLayoutNode,
    SubcomposeLayoutScope, SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
use compose_core::{NodeId, SlotId};
use compose_ui_layout::{MeasurePolicy, Placement};
use std::cell::RefCell;
use std::rc::Rc;

#[composable]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    let policy: Rc<dyn MeasurePolicy> = Rc::new(measure_policy);
    let id = compose_core::with_current_composer(|composer| {
        composer.emit_node(|| LayoutNode::new(modifier.clone(), Rc::clone(&policy)))
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut LayoutNode| {
        node.set_modifier(modifier.clone());
        node.set_measure_policy(Rc::clone(&policy));
    }) {
        debug_assert!(false, "failed to update Layout node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable]
pub fn SubcomposeLayout(
    modifier: Modifier,
    measure_policy: impl for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult
        + 'static,
) -> NodeId {
    let policy: Rc<SubcomposeMeasurePolicy> = Rc::new(measure_policy);
    let id = compose_core::with_current_composer(|composer| {
        composer.emit_node(|| SubcomposeLayoutNode::new(modifier.clone(), Rc::clone(&policy)))
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SubcomposeLayoutNode| {
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
        let width_dp = if scope_impl.max_width().0.is_finite() {
            scope_impl.max_width()
        } else {
            scope_impl.min_width()
        };
        let height_dp = if scope_impl.max_height().0.is_finite() {
            scope_impl.max_height()
        } else {
            scope_impl.min_height()
        };
        let width = scope_impl.to_px(width_dp);
        let height = scope_impl.to_px(height_dp);
        let placements: Vec<Placement> = measurables
            .into_iter()
            .map(|measurable| Placement::new(measurable.node_id(), 0.0, 0.0, 0))
            .collect();
        scope.layout(width, height, placements)
    })
}
