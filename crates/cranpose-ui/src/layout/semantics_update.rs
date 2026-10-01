use cranpose_core::{
    Applier, MemoryApplier, NodeError, NodeId,
    collections::map::{HashMap, HashSet},
};
use cranpose_foundation::SemanticsReach;

use super::{
    GeometryRect, LayoutNode, LayoutState, Point, SemanticsConfiguration, SemanticsNode,
    SemanticsPlacement, SemanticsRole, SemanticsTree, Size, SubcomposeLayoutNode,
    modal_takes_space, role_from_modifier_slices, semantics_node_from_parts, top_modal_path,
};

pub(super) fn semantics_placement(
    state: &LayoutState,
    origin: Option<Point>,
) -> (GeometryRect, Point) {
    let placement = SemanticsPlacement::of(state);
    let (top_left, content) = match origin {
        Some(origin) => placement.place(origin),
        None => SemanticsPlacement {
            position: Point::default(),
            ..placement
        }
        .place(Point::default()),
    };
    (
        GeometryRect::from_origin_size(top_left, state.size()),
        content,
    )
}

#[derive(Clone, Debug, Default)]
pub(super) struct SemanticsTracking {
    synced: Option<u64>,
    live: Vec<NodeId>,
    changed: Vec<NodeId>,
    marked: HashSet<NodeId>,
    child_stack: Vec<NodeId>,
    has_modal: bool,
}

impl SemanticsTracking {
    fn mark_changed_paths(&mut self, applier: &mut MemoryApplier) {
        self.marked.clear();
        for &start in self.changed.iter().chain(&self.live) {
            let mut current = Some(start);
            while let Some(id) = current {
                if !self.marked.insert(id) {
                    break;
                }
                let Ok(node) = applier.get_mut(id) else {
                    break;
                };
                node.mark_descendant_needs_semantics();
                current = node.parent();
            }
        }
    }
}

/// Builds a semantics snapshot from retained layout state in the live applier tree.
///
/// This is the on-demand counterpart to
/// [`build_layout_tree_from_applier`](super::build_layout_tree_from_applier).
/// It follows the currently placed child set, including subcompose active
/// children, and leaves every node's semantics dirty flags to the tree an
/// update keeps.
pub fn build_semantics_tree_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<Option<SemanticsTree>, NodeError> {
    let mut node = SemanticsNode::default();
    let mut walk = SemanticsUpdate::new(applier, Walk::Fresh, Vec::new(), Vec::new());
    Ok(walk
        .node(root, None, &mut node, false)?
        .then(|| SemanticsTree::new(node)))
}

/// Brings `tree` up to date with the placed nodes under `root`, as
/// [`build_semantics_tree_from_applier`] would build it, reusing what `tree`
/// held.
///
/// While the tree is kept across frames, an update visits only the nodes
/// whose semantics or layout changed since the last one, the nodes whose
/// semantics follow live state, and the paths to them. A node that did not
/// change keeps what it reported; one whose parent moved takes its new
/// bounds from where layout put it inside its parent, without being read
/// again. `tree` is `None` afterwards when `root` is not placed.
pub fn update_semantics_tree_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    tree: &mut Option<SemanticsTree>,
) -> Result<(), NodeError> {
    let (mut node, known, mut tracking) = match tree.take() {
        Some(previous) => {
            let known = previous.full.node_id == root;
            (previous.full, known, previous.tracking)
        }
        None => (
            SemanticsNode::default(),
            false,
            SemanticsTracking::default(),
        ),
    };
    let sync = crate::render_state::with_current_semantics_layout_log(|log| {
        log.begin_sync(&mut tracking.changed)
    });
    let incremental =
        known && sync.is_some_and(|sync| sync.continuous && tracking.synced == Some(sync.epoch));
    let walk = if incremental {
        tracking.mark_changed_paths(applier);
        Walk::Changed
    } else {
        Walk::Every
    };
    let live = std::mem::take(&mut tracking.live);
    let child_stack = std::mem::take(&mut tracking.child_stack);
    let mut update = SemanticsUpdate::new(applier, walk, live, child_stack);
    if !update.node(root, None, &mut node, known)? {
        return Ok(());
    }
    let (modal, has_modal) = if update.saw_modal || tracking.has_modal {
        (top_modal_path(&node), contains_modal(&node))
    } else {
        (None, false)
    };
    tracking.live = update.live;
    tracking.child_stack = update.child_stack;
    tracking.has_modal = has_modal;
    tracking.synced = sync.map(|sync| sync.epoch);
    *tree = Some(SemanticsTree {
        full: node,
        modal,
        tracking,
    });
    Ok(())
}

fn contains_modal(node: &SemanticsNode) -> bool {
    node.details().is_modal || node.children.iter().any(contains_modal)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Walk {
    Changed,
    Every,
    Fresh,
}

#[derive(Clone, Copy)]
enum Peek {
    Skip,
    Clean(Point),
    Visit,
}

#[expect(
    clippy::too_many_arguments,
    reason = "one node's report is written from the parts its visit read"
)]
fn write_report(
    node: &mut SemanticsNode,
    node_id: NodeId,
    generation: u32,
    same: bool,
    merged: Option<(SemanticsRole, Option<SemanticsConfiguration>)>,
    reach: SemanticsReach,
    state: &LayoutState,
    origin: Option<Point>,
) -> Point {
    let (bounds, content_origin) = semantics_placement(state, origin);
    match merged {
        Some((role, config)) => {
            let mut children = std::mem::take(&mut node.children);
            if !same {
                children.clear();
            }
            let held_details = node.details.take();
            *node = semantics_node_from_parts(
                node_id,
                generation,
                role,
                config,
                children,
                held_details,
                bounds,
            );
        }
        None => {
            let is_modal = reach.is_modal
                && modal_takes_space(Size {
                    width: bounds.width,
                    height: bounds.height,
                });
            if node.details().is_modal != is_modal {
                node.update_details(|details| details.is_modal = is_modal);
            }
            node.bounds = bounds;
        }
    }
    node.placement = SemanticsPlacement::of(state);
    content_origin
}

const SUBCOMPOSE_REACH: SemanticsReach = SemanticsReach {
    is_modal: false,
    hidden: false,
    merges_live_state: true,
};

struct SemanticsUpdate<'a> {
    applier: &'a mut MemoryApplier,
    walk: Walk,
    child_stack: Vec<NodeId>,
    live: Vec<NodeId>,
    saw_modal: bool,
}

impl<'a> SemanticsUpdate<'a> {
    fn new(
        applier: &'a mut MemoryApplier,
        walk: Walk,
        mut live: Vec<NodeId>,
        mut child_stack: Vec<NodeId>,
    ) -> Self {
        live.clear();
        child_stack.clear();
        Self {
            applier,
            walk,
            child_stack,
            live,
            saw_modal: false,
        }
    }

    fn node(
        &mut self,
        node_id: NodeId,
        origin: Option<Point>,
        node: &mut SemanticsNode,
        known: bool,
    ) -> Result<bool, NodeError> {
        let generation = self.applier.node_generation(node_id);
        let same = known && node.node_generation == generation;
        let first_child = self.child_stack.len();
        let Some(content_origin) = self.visit(node_id, origin, node, generation, same)? else {
            return Ok(false);
        };
        self.saw_modal |= node.details().is_modal;
        self.children(&mut node.children, first_child, content_origin)?;
        Ok(true)
    }

    fn visit(
        &mut self,
        node_id: NodeId,
        origin: Option<Point>,
        node: &mut SemanticsNode,
        generation: u32,
        same: bool,
    ) -> Result<Option<Point>, NodeError> {
        let child_stack = &mut self.child_stack;
        let live = &mut self.live;
        let fresh = self.walk == Walk::Fresh;
        match self.applier.with_node::<LayoutNode, _>(node_id, |layout| {
            let state = layout.layout_state();
            if !state.is_placed() {
                return None;
            }
            let reach = if fresh {
                SemanticsReach::default()
            } else {
                layout.semantics_reach()
            };
            let keep = !fresh && same && !layout.semantics_changed() && !reach.merges_live_state;
            let merged = (!keep).then(|| {
                (
                    role_from_modifier_slices(&layout.modifier_slices_snapshot()),
                    layout.semantics_configuration(),
                )
            });
            let content_origin = write_report(
                node, node_id, generation, same, merged, reach, &state, origin,
            );
            if reach.merges_live_state {
                live.push(node_id);
            }
            child_stack.extend_from_slice(&layout.children);
            if !fresh {
                layout.clear_needs_semantics();
            }
            Some(content_origin)
        }) {
            Ok(visit) => return Ok(visit),
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => {}
            Err(err) => return Err(err),
        }
        match self
            .applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |subcompose| {
                let state = subcompose.layout_state();
                if !state.is_placed() {
                    return None;
                }
                let merged = Some((
                    SemanticsRole::Subcompose,
                    subcompose.semantics_configuration(),
                ));
                let content_origin = write_report(
                    node,
                    node_id,
                    generation,
                    same,
                    merged,
                    SUBCOMPOSE_REACH,
                    &state,
                    origin,
                );
                if !fresh {
                    live.push(node_id);
                    subcompose.clear_needs_semantics();
                }
                subcompose.with_active_children(|children| child_stack.extend_from_slice(children));
                Some(content_origin)
            }) {
            Ok(visit) => Ok(visit),
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn peek(&mut self, child_id: NodeId) -> Result<Peek, NodeError> {
        let changed_only = self.walk == Walk::Changed;
        match self.applier.with_node::<LayoutNode, _>(child_id, |layout| {
            if layout.is_window_root() {
                Peek::Skip
            } else if changed_only && !layout.needs_semantics() && layout.is_placed() {
                Peek::Clean(layout.position())
            } else {
                Peek::Visit
            }
        }) {
            Ok(peek) => Ok(peek),
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(Peek::Visit),
            Err(err) => Err(err),
        }
    }

    fn child(
        &mut self,
        child_id: NodeId,
        peek: Peek,
        origin: Point,
        node: &mut SemanticsNode,
        known: bool,
    ) -> Result<bool, NodeError> {
        match peek {
            Peek::Clean(position) if known && position == node.placement.position => {
                move_with_parent(node, origin);
                Ok(true)
            }
            _ => self.node(child_id, Some(origin), node, known),
        }
    }

    fn children(
        &mut self,
        children: &mut Vec<SemanticsNode>,
        first_child: usize,
        content: Point,
    ) -> Result<(), NodeError> {
        let end = self.child_stack.len();
        let mut held = HeldChildren::default();
        let mut in_place = true;
        let mut kept = 0;
        for index in first_child..end {
            let child_id = self.child_stack[index];
            let peek = self.peek(child_id)?;
            if matches!(peek, Peek::Skip) {
                continue;
            }
            if in_place {
                if let Some(offset) = children[kept..]
                    .iter()
                    .position(|child| child.node_id == child_id)
                {
                    held.extend(children.drain(kept..kept + offset));
                    if self.child(child_id, peek, content, &mut children[kept], true)? {
                        kept += 1;
                    } else {
                        children.remove(kept);
                    }
                    continue;
                }
                held.extend(children.drain(kept..));
                in_place = false;
            }
            let taken = held.take(child_id);
            let known = taken.is_some();
            let mut child = taken.unwrap_or_default();
            if self.child(child_id, peek, content, &mut child, known)? {
                if children.capacity() == 0 {
                    children.reserve_exact((end - index).min(4));
                }
                children.push(child);
            }
        }
        if in_place {
            children.truncate(kept);
        }
        self.child_stack.truncate(first_child);
        Ok(())
    }
}

fn move_with_parent(node: &mut SemanticsNode, origin: Point) {
    let (top_left, content) = node.placement.place(origin);
    if top_left.x.to_bits() == node.bounds.x.to_bits()
        && top_left.y.to_bits() == node.bounds.y.to_bits()
    {
        return;
    }
    node.bounds.x = top_left.x;
    node.bounds.y = top_left.y;
    for child in &mut node.children {
        move_with_parent(child, content);
    }
}

#[derive(Default)]
struct HeldChildren {
    nodes: Vec<SemanticsNode>,
    positions: HashMap<NodeId, usize>,
}

impl HeldChildren {
    fn extend(&mut self, nodes: impl Iterator<Item = SemanticsNode>) {
        for node in nodes {
            self.positions.insert(node.node_id, self.nodes.len());
            self.nodes.push(node);
        }
    }

    fn take(&mut self, node_id: NodeId) -> Option<SemanticsNode> {
        let position = self.positions.remove(&node_id)?;
        let node = self.nodes.swap_remove(position);
        if let Some(moved) = self.nodes.get(position) {
            self.positions.insert(moved.node_id, position);
        }
        Some(node)
    }
}
