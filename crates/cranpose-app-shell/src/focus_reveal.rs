use std::rc::Rc;

use cranpose_core::{NodeId, run_in_mutable_snapshot};
use cranpose_foundation::{ScrollAxisRange, SemanticsScrollBy};
use cranpose_render_common::Renderer;
use cranpose_ui::{LayoutBox, SemanticsNode};
use cranpose_ui_graphics::Rect;

use crate::AppShell;

impl<R: Renderer> AppShell<R>
where
    R::Error: std::fmt::Debug,
{
    /// Reveals a screen reader's target without changing keyboard focus.
    ///
    /// Hidden, disabled, removed and modal-background targets are rejected.
    /// Returns whether a scroll action reports movement.
    pub fn accessibility_reveal(&mut self, node_id: NodeId) -> bool {
        let app_context = Rc::clone(&self.app.app_context);
        app_context.enter(|| self.reveal_node_in_context(node_id))
    }

    pub(crate) fn reveal_new_focus(&mut self) {
        let focused = cranpose_ui::active_focus_target();
        if focused == self.app.revealed_focus {
            return;
        }
        self.app.revealed_focus = focused;
        let Some(focused) = focused else { return };
        self.reveal_node_in_context(focused);
    }

    fn reveal_node_in_context(&mut self, focused: NodeId) -> bool {
        let mut revealed = false;
        for index in 0..self.surfaces.len() {
            let ancestors = self.surfaces[index]
                .semantics_tree_for_input(&mut self.app)
                .and_then(|tree| scroll_ancestors(tree.root(), focused))
                .unwrap_or_default();
            for ancestor in ancestors {
                revealed |= self.reveal_within_container(index, focused, ancestor);
            }
        }
        revealed
    }

    fn reveal_within_container(&mut self, index: usize, focused: NodeId, ancestor: NodeId) -> bool {
        let mut revealed = false;
        let mut previous_distance = f32::INFINITY;
        loop {
            let surface = &mut self.surfaces[index];
            let request = surface
                .layout_tree_in_context(&mut self.app)
                .and_then(|tree| {
                    Some((
                        bounds_of(tree.root(), focused)?,
                        bounds_of(tree.root(), ancestor)?,
                    ))
                })
                .and_then(|(target, viewport)| {
                    let tree = surface.semantics_tree_for_input(&mut self.app)?;
                    scroll_request(semantics_of(tree.root(), ancestor)?, target, viewport)
                });
            let Some((action, dx, dy)) = request else {
                break;
            };
            let distance = dx.abs() + dy.abs();
            if distance >= previous_distance {
                break;
            }
            previous_distance = distance;
            if !run_in_mutable_snapshot(|| action.invoke(dx, dy)).unwrap_or(false) {
                break;
            }
            revealed = true;
            self.app.request_layout_pass();
            self.run_layout_phase_in_context();
        }
        revealed
    }
}

fn scroll_ancestors(node: &SemanticsNode, target: NodeId) -> Option<Vec<NodeId>> {
    if node.hidden || !node.enabled {
        return None;
    }
    if node.node_id == target {
        return Some(Vec::new());
    }
    let mut path = node
        .children
        .iter()
        .find_map(|node| scroll_ancestors(node, target))?;
    if node.scroll_by.is_some() {
        path.push(node.node_id);
    }
    Some(path)
}

fn semantics_of(node: &SemanticsNode, target: NodeId) -> Option<&SemanticsNode> {
    if node.node_id == target {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|node| semantics_of(node, target))
}

fn bounds_of(node: &LayoutBox, target: NodeId) -> Option<Rect> {
    if node.node_id == target {
        return Some(node.rect);
    }
    node.children
        .iter()
        .find_map(|node| bounds_of(node, target))
}

fn scroll_request(
    node: &SemanticsNode,
    target: Rect,
    viewport: Rect,
) -> Option<(SemanticsScrollBy, f32, f32)> {
    let action = node.scroll_by.clone()?;
    let dx = axis_delta(
        target.x,
        target.width,
        viewport.x,
        viewport.width,
        node.horizontal_scroll,
    );
    let dy = axis_delta(
        target.y,
        target.height,
        viewport.y,
        viewport.height,
        node.vertical_scroll,
    );
    (dx != 0.0 || dy != 0.0).then_some((action, dx, dy))
}

fn axis_delta(
    start: f32,
    size: f32,
    viewport_start: f32,
    viewport_size: f32,
    range: Option<ScrollAxisRange>,
) -> f32 {
    let Some(range) = range else { return 0.0 };
    if ![start, size, viewport_start, viewport_size]
        .iter()
        .all(|value| value.is_finite())
        || size <= 0.0
        || viewport_size <= 0.0
    {
        return 0.0;
    }
    let visible_end = viewport_start + viewport_size;
    let delta = if start < viewport_start && start + size < visible_end {
        (start - viewport_start).max(start + size - visible_end)
    } else if start + size > visible_end && start > viewport_start {
        (start + size - visible_end).min(start - viewport_start)
    } else {
        0.0
    };
    let delta = if range.reverse { -delta } else { delta };
    if (delta > 0.0 && range.can_scroll_forward()) || (delta < 0.0 && range.can_scroll_backward()) {
        delta
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "tests/focus_reveal_tests.rs"]
mod tests;
