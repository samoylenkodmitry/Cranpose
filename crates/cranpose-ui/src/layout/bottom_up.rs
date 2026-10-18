//! A layout pass that starts from the nodes the dirty bubbles started from,
//! as Compose's relayout nodes do, instead of walking down to them through
//! every unchanged ancestor.
//!
//! Each such node measures again at the constraints its parent last gave it.
//! When it then reads the same for its parent, or the parent's measurement
//! ignores what changed, the walk up stops there and the node stays where
//! the parent placed it. Otherwise the parent measures again at its own last
//! constraints, and the walk goes on from it.
//!
//! The pass runs only when every node from a dirty node up to the root is a
//! plain layout node and no ancestor needs measure itself. It clears the
//! marks the bubbles left on those ancestors as it checks them, and keeps
//! what it cleared. When it cannot go on, it puts the marks back and the
//! pass walks down from the root, which they lead to everything left
//! undone: a parent the walk up was about to measure again is marked as
//! needing measure first.

use std::rc::Rc;

use cranpose_core::{Node, NodeError, NodeId};
use cranpose_ui_layout::Constraints;
use smallvec::SmallVec;

use super::{ForParent, LayoutBuilderState, MeasuredNode};
use crate::widgets::nodes::{LayoutDirt, LayoutNode};

/// The marks the pass cleared, with the node each was on.
type Cleared = Vec<(NodeId, LayoutDirt)>;

/// A dirty node the pass measures, with its parent and the constraints the
/// parent last gave it.
#[derive(Clone, Copy)]
struct Start {
    node: NodeId,
    parent: NodeId,
    constraints: Constraints,
}

/// Whether a dirty node can measure from where it is.
enum Plan {
    /// The node needs nothing: a pass measured it already.
    Clean,
    Start(Start),
    /// The pass has to walk down from the root.
    WalkDown,
}

/// Where the walk up from one dirty node ended.
enum Climb {
    /// At a parent that lays out as before; `measured_ancestor` when the
    /// walk measured an ancestor again on its way there.
    Stopped { measured_ancestor: bool },
    /// At the root, which measured again.
    Root(Rc<MeasuredNode>),
    /// At a node the walk up cannot measure: the pass walks down.
    WalkDown,
}

impl LayoutBuilderState {
    /// Measures `root` for a pass: from the dirty nodes up when it can,
    /// otherwise down from the root.
    pub(super) fn measure_root(
        self: &Rc<Self>,
        root: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        match self.measure_dirty_nodes_first(root, constraints)? {
            Some(measured) => Ok(measured),
            None => self.measure_node(root, constraints),
        }
    }

    /// Takes the nodes the dirty bubbles started from and measures from
    /// them up: see [`Self::measure_from_dirty_nodes`]. Every pass takes
    /// them, so a pass that walks down from the root leaves none behind.
    fn measure_dirty_nodes_first(
        self: &Rc<Self>,
        root: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let mut dirty = self.frame_arena.borrow_mut().tmp_child_ids.acquire();
        let complete = match self.applier.try_borrow_typed() {
            Ok(mut applier) => applier.take_layout_dirt_roots(&mut dirty),
            Err(_) => false,
        };
        let measured = if complete {
            self.measure_from_dirty_nodes(root, constraints, &dirty)
        } else {
            Ok(None)
        };
        self.frame_arena.borrow_mut().tmp_child_ids.release(dirty);
        measured
    }

    /// The root's measurement after measuring again the nodes `dirty` names
    /// and the ancestors their changes reach. `None` when the pass has to
    /// walk down from the root.
    fn measure_from_dirty_nodes(
        self: &Rc<Self>,
        root: NodeId,
        constraints: Constraints,
        dirty: &[NodeId],
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        if dirty.is_empty() {
            return Ok(None);
        }
        let Some(root_measured) = self.held_root_measurement(root, constraints) else {
            return Ok(None);
        };
        let mut cleared = std::mem::take(&mut self.frame_arena.borrow_mut().cleared_marks);
        cleared.clear();
        let measured = self.measure_from_dirty_nodes_clearing(
            root,
            constraints,
            dirty,
            root_measured,
            &mut cleared,
        );
        self.frame_arena.borrow_mut().cleared_marks = cleared;
        measured
    }

    /// [`Self::measure_from_dirty_nodes`] with `cleared` to keep the marks
    /// it clears in.
    fn measure_from_dirty_nodes_clearing(
        self: &Rc<Self>,
        root: NodeId,
        constraints: Constraints,
        dirty: &[NodeId],
        mut root_measured: Rc<MeasuredNode>,
        cleared: &mut Cleared,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let mut starts: SmallVec<[Start; 4]> = SmallVec::new();
        for &node in dirty {
            match self.plan(node, root, cleared) {
                Plan::Clean => {}
                Plan::Start(start) => starts.push(start),
                Plan::WalkDown => {
                    self.restore_marks(cleared);
                    return Ok(None);
                }
            }
        }
        if starts.is_empty() {
            return Ok(None);
        }
        // A dirty node measures already only when a walk up from another one
        // measured an ancestor of it again.
        let mut measured_ancestors = false;
        for start in &starts {
            match self.measure_up(start, root, constraints, measured_ancestors)? {
                Climb::Stopped { measured_ancestor } => measured_ancestors |= measured_ancestor,
                Climb::Root(measured) => {
                    root_measured = measured;
                    measured_ancestors = true;
                }
                Climb::WalkDown => {
                    self.restore_marks(cleared);
                    return Ok(None);
                }
            }
        }
        Ok(Some(root_measured))
    }

    /// The root's measurement for `constraints`, while the root is a clean
    /// layout node that holds one.
    fn held_root_measurement(
        &self,
        root: NodeId,
        constraints: Constraints,
    ) -> Option<Rc<MeasuredNode>> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        applier
            .with_node::<LayoutNode, _>(root, |node| {
                if needs_layout_itself(node)
                    || Node::is_virtual(node)
                    || node.cache_handles().epoch() < self.cache_floor
                {
                    return None;
                }
                node.cache_handles().get_measurement(constraints)
            })
            .ok()
            .flatten()
    }

    /// Checks that every node from `node` up to `root` is a plain layout
    /// node, reads the constraints its parent last gave it, and clears the
    /// marks the bubbles left on its ancestors into `cleared`.
    fn plan(&self, node: NodeId, root: NodeId, cleared: &mut Cleared) -> Plan {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return Plan::WalkDown;
        };
        let start = applier.with_node::<LayoutNode, _>(node, |node| {
            let dirty = needs_layout_itself(node) || Node::descendant_needs_layout(node);
            (dirty, plain_layout_node(node), Node::parent(node))
        });
        let Some(parent) = (match start {
            Ok((false, _, _)) => return Plan::Clean,
            Ok((true, true, parent)) if node != root => parent,
            _ => return Plan::WalkDown,
        }) else {
            return Plan::WalkDown;
        };
        let first_parent = parent;
        let mut constraints = None;
        let mut child = node;
        let mut next = Some(parent);
        while let Some(id) = next {
            let step = applier.with_node::<LayoutNode, _>(id, |ancestor| {
                if !plain_layout_node(ancestor) || needs_layout_itself(ancestor) {
                    return None;
                }
                if child == node {
                    constraints = ancestor
                        .layout_runtime_state()
                        .borrow()
                        .child_states()
                        .iter()
                        .find(|state| state.node_id == node)
                        .and_then(|state| state.measured_constraints.get());
                }
                cleared.push((id, ancestor.cache_handles().take_layout_dirt()));
                Some(Node::parent(ancestor))
            });
            let Ok(Some(parent)) = step else {
                return Plan::WalkDown;
            };
            if id == root {
                return match constraints {
                    Some(constraints) => Plan::Start(Start {
                        node,
                        parent: first_parent,
                        constraints,
                    }),
                    None => Plan::WalkDown,
                };
            }
            child = id;
            next = parent;
        }
        Plan::WalkDown
    }

    /// Puts back the marks `cleared` holds, the latest first, so a node it
    /// names twice gets the marks it had before the first.
    fn restore_marks(&self, cleared: &Cleared) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        for &(id, dirt) in cleared.iter().rev() {
            let _ = applier.with_node::<LayoutNode, _>(id, |node| {
                node.cache_handles().restore_dirt(dirt);
            });
        }
    }

    /// Measures the node `start` names again, if it still needs it, and
    /// walks up as far as its change reaches; the root measures with
    /// `root_constraints`. Only when `measured_ancestors` can a walk up from
    /// another dirty node have measured this one already.
    fn measure_up(
        self: &Rc<Self>,
        start: &Start,
        root: NodeId,
        root_constraints: Constraints,
        measured_ancestors: bool,
    ) -> Result<Climb, NodeError> {
        if measured_ancestors
            && !self
                .read_node(start.node, |node| {
                    needs_layout_itself(node) || Node::descendant_needs_layout(node)
                })
                .unwrap_or(false)
        {
            return Ok(Climb::Stopped {
                measured_ancestor: false,
            });
        }
        let mut child = start.node;
        let mut parent = start.parent;
        let mut measured = self.measure_node(child, start.constraints)?;
        let mut changed_itself = true;
        loop {
            let (unaffected, grandparent) =
                self.parent_take(child, parent, &measured, changed_itself)?;
            if unaffected {
                return Ok(Climb::Stopped {
                    measured_ancestor: !changed_itself,
                });
            }
            if parent == root {
                return Ok(Climb::Root(self.measure_node(root, root_constraints)?));
            }
            let Some(grandparent) = grandparent else {
                return Ok(Climb::WalkDown);
            };
            let Some(constraints) = self.recorded_constraints(grandparent, parent) else {
                return Ok(Climb::WalkDown);
            };
            measured = self.measure_node(parent, constraints)?;
            child = parent;
            parent = grandparent;
            changed_itself = false;
        }
    }

    fn read_node<R>(&self, node: NodeId, read: impl FnOnce(&LayoutNode) -> R) -> Option<R> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        applier
            .with_node::<LayoutNode, _>(node, |node| read(node))
            .ok()
    }

    /// The constraints `parent`'s last measure gave `child`.
    fn recorded_constraints(&self, parent: NodeId, child: NodeId) -> Option<Constraints> {
        self.read_node(parent, |parent| {
            parent
                .layout_runtime_state()
                .borrow()
                .child_states()
                .iter()
                .find(|state| state.node_id == child)
                .and_then(|state| state.measured_constraints.get())
        })
        .flatten()
    }

    /// Whether `parent` lays out as before now that `child` measured
    /// `measured`, and the parent's own parent. The child goes back where
    /// the parent placed it; a parent that has to measure again places it
    /// anew, and is marked as needing measure. A child that `changed_itself`
    /// may give new parent data.
    fn parent_take(
        &self,
        child: NodeId,
        parent: NodeId,
        measured: &Rc<MeasuredNode>,
        changed_itself: bool,
    ) -> Result<(bool, Option<NodeId>), NodeError> {
        let mut applier =
            self.applier
                .try_borrow_typed()
                .map_err(|_| NodeError::MissingContext {
                    id: parent,
                    reason: "layout applier borrowed",
                })?;
        let parent_data = applier.with_node::<LayoutNode, _>(child, |child| {
            child.set_position(child.position(), Some(parent), &self.context);
            changed_itself.then(|| child.parent_data())
        })?;
        applier.with_node::<LayoutNode, _>(parent, |node| {
            let unaffected = Self::lays_out_as_before(node, child, measured, parent_data.as_ref());
            if !unaffected {
                node.mark_needs_measure();
            }
            (unaffected, Node::parent(node))
        })
    }

    /// Whether `parent` lays out as before with `child` reading `measured`
    /// and, when it changed, `parent_data`. A parent that read the child's
    /// intrinsic sizes does not, nor does one whose policy reads the changed
    /// parent data. A parent that only ignores the child's new size keeps its
    /// measurement for its own constraints alone, as the other ones its hold
    /// names may not take the new size.
    fn lays_out_as_before(
        parent: &LayoutNode,
        child: NodeId,
        measured: &MeasuredNode,
        parent_data: Option<&cranpose_ui_layout::ParentData>,
    ) -> bool {
        let runtime_state = parent.layout_runtime_state().borrow();
        let Some(record) = runtime_state
            .child_states()
            .iter()
            .find(|state| state.node_id == child)
        else {
            return false;
        };
        if record.read_intrinsics.get() {
            return false;
        }
        if let Some(data) = parent_data
            && super::read_cell(&record.parent_data, |bound| {
                bound.as_ref().is_none_or(|bound| {
                    runtime_state
                        .measure_policy
                        .reads_parent_data_change(bound, data)
                })
            })
        {
            return false;
        }
        let now = ForParent::of(measured);
        let cache = parent.cache_handles();
        let resized = cache.read_measurement(|held, ignores_child_sizes| {
            let previous = held.children.iter().find(|entry| entry.node_id == child)?;
            if previous.for_parent.same_as(&now) {
                return Some(false);
            }
            (ignores_child_sizes && previous.for_parent.same_but_size(&now)).then_some(true)
        });
        match resized.flatten() {
            Some(false) => true,
            Some(true) => {
                if let (Some(held), Some(constraints)) =
                    (cache.held_measurement(), cache.measured_constraints())
                {
                    cache.store_resized_measurement(constraints, held);
                }
                true
            }
            None => false,
        }
    }
}

fn needs_layout_itself(node: &LayoutNode) -> bool {
    node.needs_measure() || node.needs_layout()
}

/// A layout node the pass may measure on its own: neither a virtual slot
/// node nor the root of another window.
fn plain_layout_node(node: &LayoutNode) -> bool {
    !Node::is_virtual(node) && !node.is_window_root()
}
