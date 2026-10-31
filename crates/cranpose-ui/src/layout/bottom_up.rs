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
//! A subcompose node keeps no record of what its policy gave each child or
//! read of it, so the walk up treats it as the walk down does. A child that
//! did not change itself measures again at the constraints its own cache
//! holds, and the subcompose node keeps its measurement when the child reads
//! the same for it; for a child that changed itself, the walk starts at the
//! subcompose node, whose policy measures the child again. A child the walk
//! measured stays marked for the policy, which may keep measurements of its
//! own, as a lazy list keeps its items'. A slot host lays out nothing
//! itself: the walk passes over it, and a dirty one starts the walk at its
//! subcompose node.
//!
//! The pass runs only when every node from a dirty node up to the root is a
//! layout node, a subcompose node or a slot host, none of them the root of
//! another window, and no ancestor needs measure itself. It clears the
//! marks the bubbles left on those ancestors as it checks them, and keeps
//! what it cleared; a node a subcompose node lays out keeps its marks for
//! the policy. When it cannot go on, it puts the marks back and the pass
//! walks down from the root, which they lead to everything left undone: a
//! parent the walk up was about to measure again is marked as needing
//! measure first.

use std::rc::Rc;

use cranpose_core::{MemoryApplier, Node, NodeError, NodeId};
use cranpose_ui_layout::Constraints;
use smallvec::SmallVec;

use super::{
    ForParent, LayoutBuilderState, MeasuredNode,
    live_layout_node::{LiveLayoutNode, read_live_layout_node},
};
use crate::widgets::nodes::{LayoutDirt, LayoutNode, NodeCache};

// The marks the pass cleared, with the node each was on.
type Cleared = Vec<(NodeId, LayoutDirt)>;

// A dirty node the pass measures, with its parent and the constraints the
// parent last gave it.
#[derive(Clone, Copy)]
struct Start {
    node: NodeId,
    parent: NodeId,
    constraints: Constraints,
    // The node's own layout changed, not only a node below it: it may give
    // new parent data.
    changed_itself: bool,
    // The dirty node the walk starts for: `node`, or a node below it that
    // `node`'s subcompose policy measures again.
    dirty: NodeId,
}

// What the plan reads of a dirty node: the node itself, or the subcompose
// node a dirty slot host stands for.
struct DirtyNode {
    node: NodeId,
    dirt: LayoutDirt,
    window_root: bool,
    parent: Option<NodeId>,
    // The constraints the node's own cache holds, for a subcompose parent.
    own_constraints: Option<Constraints>,
}

// The plan's walk from a dirty node up to the root. A subcompose node
// keeps no record of what it gave a child or read of it, so for a child
// that changed itself, or holds no constraints, the walk starts at the
// subcompose node, whose policy measures the child again.
struct PlanWalk {
    dirty: NodeId,
    // The node the walk up measures first.
    start: NodeId,
    changed_itself: bool,
    // The constraints `start`'s own cache holds, for a subcompose parent.
    own: Option<Constraints>,
    // The node that lays out `start`, once the walk passed it.
    first_parent: Option<NodeId>,
    // The constraints `first_parent` gave `start`.
    constraints: Option<Constraints>,
    // The last node the walk passed.
    child: NodeId,
}

impl PlanWalk {
    fn new(dirty: &DirtyNode) -> Self {
        Self {
            dirty: dirty.node,
            start: dirty.node,
            changed_itself: dirty.dirt.measure || dirty.dirt.layout,
            own: dirty.own_constraints,
            first_parent: None,
            constraints: None,
            child: dirty.node,
        }
    }

    // What the walk finds at `ancestor`, the node `id` names; the marks it
    // clears go into `cleared`.
    fn visit(&mut self, id: NodeId, ancestor: &LiveLayoutNode<'_>, cleared: &mut Cleared) -> Step {
        if ancestor.is_virtual() {
            return Step::SlotHost(ancestor.parent());
        }
        if ancestor.is_window_root() || needs_layout_itself(ancestor.cache()) {
            return Step::WalkDown;
        }
        if self.child == self.start {
            self.constraints = match ancestor {
                LiveLayoutNode::Layout(node) => given_constraints(node, self.child),
                LiveLayoutNode::Subcompose(_) => match self.own.filter(|_| !self.changed_itself) {
                    Some(constraints) => Some(constraints),
                    None => {
                        return Step::StartHere(
                            ancestor.parent(),
                            own_constraints(ancestor.cache()),
                        );
                    }
                },
            };
        }
        cleared.push((id, ancestor.cache().take_layout_dirt()));
        Step::Up(ancestor.parent())
    }

    fn passed(&mut self, id: NodeId) {
        self.first_parent.get_or_insert(id);
        self.child = id;
    }

    // Starts the walk up at the subcompose node `id`, whose own cache holds
    // `own`.
    fn start_at(&mut self, id: NodeId, own: Option<Constraints>) {
        self.start = id;
        self.changed_itself = false;
        self.own = own;
        self.first_parent = None;
        self.child = id;
    }

    fn into_plan(self) -> Plan {
        match (self.first_parent, self.constraints) {
            (Some(parent), Some(constraints)) => Plan::Start(Start {
                node: self.start,
                parent,
                constraints,
                changed_itself: self.changed_itself,
                dirty: self.dirty,
            }),
            _ => Plan::WalkDown,
        }
    }
}

// What the plan finds at an ancestor of a dirty node.
enum Step {
    // A layout or subcompose node it cleared, with its parent.
    Up(Option<NodeId>),
    // A slot host, with the subcompose node it belongs to.
    SlotHost(Option<NodeId>),
    // A subcompose node whose policy has to measure the walk's start again,
    // with its parent and the constraints its own cache holds.
    StartHere(Option<NodeId>, Option<Constraints>),
    WalkDown,
}

// Whether a dirty node can measure from where it is.
enum Plan {
    // The node needs nothing: a pass measured it already.
    Clean,
    Start(Start),
    // The pass has to walk down from the root.
    WalkDown,
}

// Where the walk up from one dirty node ended.
enum Climb {
    // At a parent that lays out as before; `measured_ancestor` when the
    // walk measured an ancestor again on its way there.
    Stopped { measured_ancestor: bool },
    // At the root, which measured again.
    Root(Rc<MeasuredNode>),
    // At a node the walk up cannot measure: the pass walks down.
    WalkDown,
}

impl LayoutBuilderState {
    // Measures `root` for a pass: from the dirty nodes up when it can,
    // otherwise down from the root.
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

    // Takes the nodes the dirty bubbles started from and measures from
    // them up: see [`Self::measure_from_dirty_nodes`]. Every pass takes
    // them, so a pass that walks down from the root leaves none behind.
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

    // The root's measurement after measuring again the nodes `dirty` names
    // and the ancestors their changes reach. `None` when the pass has to
    // walk down from the root.
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

    // [`Self::measure_from_dirty_nodes`] with `cleared` to keep the marks
    // it clears in.
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
        // A subcompose node that composed while it measured may have marked
        // ancestors the walks up did not measure again. The walk down clears
        // a node's marks once it measured the node, and so does this pass.
        if self
            .applier
            .try_borrow_typed()
            .is_ok_and(|applier| applier.has_layout_dirt_roots())
        {
            self.clear_marks(cleared);
        }
        Ok(Some(root_measured))
    }

    // The root's measurement for `constraints`, while the root is a clean
    // layout node that holds one.
    fn held_root_measurement(
        &self,
        root: NodeId,
        constraints: Constraints,
    ) -> Option<Rc<MeasuredNode>> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        applier
            .with_node::<LayoutNode, _>(root, |node| {
                if needs_layout_itself(node.cache())
                    || Node::is_virtual(node)
                    || node.cache().epoch() < self.cache_floor
                {
                    return None;
                }
                node.cache().get_measurement(constraints)
            })
            .ok()
            .flatten()
    }

    // Checks that every node from `node` up to `root` is a layout node or
    // a subcompose node, passing over slot hosts, reads the constraints the
    // node's parent last gave it, and clears the marks the bubbles left on
    // its ancestors into `cleared`.
    fn plan(&self, node: NodeId, root: NodeId, cleared: &mut Cleared) -> Plan {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return Plan::WalkDown;
        };
        let dirty = match read_dirty_node(&mut applier, node) {
            Some(dirty) if !dirty.dirt.any() => return Plan::Clean,
            Some(dirty) if !dirty.window_root && dirty.node != root => dirty,
            _ => return Plan::WalkDown,
        };
        let mut walk = PlanWalk::new(&dirty);
        let mut next = dirty.parent;
        while let Some(id) = next {
            let step = read_live_layout_node(&mut applier, id, |ancestor| {
                walk.visit(id, &ancestor, cleared)
            });
            next = match step {
                Ok(Some(Step::Up(parent))) => {
                    walk.passed(id);
                    if id == root {
                        return walk.into_plan();
                    }
                    parent
                }
                Ok(Some(Step::StartHere(parent, own))) if id != root => {
                    walk.start_at(id, own);
                    parent
                }
                Ok(Some(Step::SlotHost(parent))) => {
                    // The node below the slot host keeps its marks for the
                    // subcompose node's policy.
                    if walk.child != walk.start
                        && let Some(&(content, dirt)) = cleared.last()
                    {
                        let restored = read_live_layout_node(&mut applier, content, |content| {
                            content.cache().restore_dirt(dirt);
                        });
                        if !matches!(restored, Ok(Some(()))) {
                            return Plan::WalkDown;
                        }
                        cleared.pop();
                    }
                    parent
                }
                _ => return Plan::WalkDown,
            };
        }
        Plan::WalkDown
    }

    // Clears the marks of the nodes `cleared` names.
    fn clear_marks(&self, cleared: &Cleared) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        for &(id, _) in cleared {
            let read = read_live_layout_node(&mut applier, id, |node| {
                node.cache().clear_layout_dirt();
            });
            if read.is_err() {
                return;
            }
        }
    }

    // Puts back the marks `cleared` holds, the latest first, so a node it
    // names twice gets the marks it had before the first.
    fn restore_marks(&self, cleared: &Cleared) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        for &(id, dirt) in cleared.iter().rev() {
            let _ = read_live_layout_node(&mut applier, id, |node| {
                node.cache().restore_dirt(dirt);
            });
        }
    }

    // Measures the node `start` names again, if it still needs it, and
    // walks up as far as its change reaches; the root measures with
    // `root_constraints`. Only when `measured_ancestors` can a walk up from
    // another dirty node have measured this one already.
    fn measure_up(
        self: &Rc<Self>,
        start: &Start,
        root: NodeId,
        root_constraints: Constraints,
        measured_ancestors: bool,
    ) -> Result<Climb, NodeError> {
        if measured_ancestors
            && !self
                .read_node(start.dirty, |node| node.cache().dirt().any())
                .unwrap_or(false)
        {
            return Ok(Climb::Stopped {
                measured_ancestor: false,
            });
        }
        let mut child = start.node;
        let mut parent = start.parent;
        let mut measured = self.measure_node(child, start.constraints)?;
        let mut changed_itself = start.changed_itself;
        loop {
            let (unaffected, grandparent) =
                self.parent_take(child, parent, &measured, changed_itself)?;
            if unaffected {
                return Ok(Climb::Stopped {
                    measured_ancestor: child != start.dirty,
                });
            }
            if parent == root {
                return Ok(Climb::Root(self.measure_node(root, root_constraints)?));
            }
            let Some((grandparent, constraints)) = grandparent
                .and_then(|grandparent| self.layout_parent_constraints(grandparent, parent))
            else {
                return Ok(Climb::WalkDown);
            };
            measured = self.measure_node(parent, constraints)?;
            child = parent;
            parent = grandparent;
            changed_itself = false;
        }
    }

    fn read_node<R>(&self, node: NodeId, read: impl FnOnce(LiveLayoutNode<'_>) -> R) -> Option<R> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        read_live_layout_node(&mut applier, node, read)
            .ok()
            .flatten()
    }

    // The node that lays out `child`: its parent `parent` or, when that is
    // a slot host, the subcompose node above it; and the constraints it
    // last gave `child`. A subcompose node keeps no record of them, and
    // `child`'s own cache holds them.
    fn layout_parent_constraints(
        &self,
        parent: NodeId,
        child: NodeId,
    ) -> Option<(NodeId, Constraints)> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        let recorded = read_live_layout_node(&mut applier, parent, |node| match node {
            LiveLayoutNode::Layout(node) if !Node::is_virtual(node) => {
                Ok(given_constraints(node, child))
            }
            LiveLayoutNode::Layout(host) => Err(Node::parent(host)),
            LiveLayoutNode::Subcompose(_) => Err(Some(parent)),
        })
        .ok()??;
        match recorded {
            Ok(constraints) => Some((parent, constraints?)),
            Err(layout_parent) => {
                let constraints = read_live_layout_node(&mut applier, child, |child| {
                    own_constraints(child.cache())
                })
                .ok()??;
                Some((layout_parent?, constraints?))
            }
        }
    }

    // Whether `parent` lays out as before now that `child` measured
    // `measured`, and the parent's own parent. The child goes back where
    // the parent placed it; a parent that has to measure again places it
    // anew, and is marked as needing measure. A child that `changed_itself`
    // may give new parent data; a subcompose parent never sees one, as the
    // walk starts at it instead. A child of a subcompose parent stays
    // marked for its policy, which did not see it measure again.
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
        let parent_data = read_live_layout_node(&mut applier, child, |child| {
            child.stay_placed(parent, &self.context);
            changed_itself.then(|| child.parent_data())
        })?
        .flatten();
        let taken = read_live_layout_node(&mut applier, parent, |node| {
            let unaffected = match &node {
                LiveLayoutNode::Layout(node) => {
                    Self::lays_out_as_before(node, child, measured, parent_data.as_ref())
                }
                LiveLayoutNode::Subcompose(node) => {
                    keeps_measurement(node.cache(), child, measured)
                }
            };
            if !unaffected {
                node.cache().mark_needs_measure();
            }
            let subcompose = matches!(node, LiveLayoutNode::Subcompose(_));
            (unaffected, subcompose, node.parent())
        })?;
        drop(applier);
        let Some((unaffected, subcompose, grandparent)) = taken else {
            return Ok((false, None));
        };
        if subcompose {
            self.mark_measured_without_policy(child);
        }
        Ok((unaffected, grandparent))
    }

    // Whether `parent` lays out as before with `child` reading `measured`
    // and, when it changed, `parent_data`. A parent that read the child's
    // intrinsic sizes does not, nor does one whose policy reads the changed
    // parent data.
    fn lays_out_as_before(
        parent: &LayoutNode,
        child: NodeId,
        measured: &MeasuredNode,
        parent_data: Option<&cranpose_ui_layout::ParentData>,
    ) -> bool {
        {
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
        }
        keeps_measurement(parent.cache(), child, measured)
    }
}

// Reads the node `node` names, or the subcompose node above it when it is
// a slot host, which lays out nothing itself.
fn read_dirty_node(applier: &mut MemoryApplier, mut node: NodeId) -> Option<DirtyNode> {
    loop {
        let read = read_live_layout_node(applier, node, |live| {
            if live.is_virtual() {
                return Err(live.parent());
            }
            Ok(DirtyNode {
                node,
                dirt: live.cache().dirt(),
                window_root: live.is_window_root(),
                parent: live.parent(),
                own_constraints: own_constraints(live.cache()),
            })
        });
        match read.ok()?? {
            Ok(dirty) => return Some(dirty),
            Err(parent) => node = parent?,
        }
    }
}

// The constraints `parent`'s last measure gave `child`.
fn given_constraints(parent: &LayoutNode, child: NodeId) -> Option<Constraints> {
    parent
        .layout_runtime_state()
        .borrow()
        .child_states()
        .iter()
        .find(|state| state.node_id == child)
        .and_then(|state| state.measured_constraints.get())
}

// The constraints a node's cache holds, while no parent read its intrinsic
// sizes: what a subcompose node gave it.
fn own_constraints(cache: &NodeCache) -> Option<Constraints> {
    (!cache.has_intrinsics())
        .then(|| cache.measured_constraints())
        .flatten()
}

// Whether the node whose cache is `cache` keeps its measurement now that
// `child` measured `measured`: the child reads the same for it, or only
// its size changed and the measurement ignores its children's sizes. Such
// a measurement then holds for its own constraints alone, as the other
// ones its hold names may not take the new size.
fn keeps_measurement(cache: &NodeCache, child: NodeId, measured: &MeasuredNode) -> bool {
    let now = ForParent::of(measured);
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

fn needs_layout_itself(cache: &NodeCache) -> bool {
    let dirt = cache.dirt();
    dirt.measure || dirt.layout
}
