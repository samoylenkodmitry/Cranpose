//! What a screen reader is told, and where each of those things actually is.
//!
//! [`build_semantics_tree_from_applier`] answers the first half and carries no
//! geometry at all; a [`LayoutBox`] carries the geometry and knows nothing about
//! semantics. Every question of the form "is this control big enough / on the
//! display / where the label says it is" needs both halves joined, and the join
//! existed in exactly one place: inside the desktop robot, behind an `AppShell`,
//! a window and an event loop. A unit test that wants to audit a widget tree's
//! touch targets could not reach it.
//!
//! This is that join, headless. One composition, one measure pass, and a tree of
//! [`PlacedSemanticsNode`] carrying **two** boxes per node, because they are two
//! different questions and a Wear scaling list is precisely where they diverge:
//!
//! - [`PlacedSemanticsNode::layout_bounds`] is the box the measure pass gave the
//!   node — what the widget asked for and what an unscaled row occupies. This is
//!   the box the desktop robot reports, and the one to assert a widget's own
//!   declared minimum against.
//! - [`PlacedSemanticsNode::touch_bounds`] is the axis-aligned box the renderer
//!   draws and the hit test inverts, ancestor graphics layers included. A row
//!   that a scaling ramp shrinks to 0.73 is only tappable where it is drawn
//!   (`cranpose-render-common`'s `a_shrunken_row_is_only_tappable_where_it_is_drawn`
//!   settles that), so this is the box a finger has to find. It is `None` for a
//!   node the hit graph carries no region for — a label, a header, anything that
//!   is described but not interactive. Gesture containers such as scrollable
//!   lists are interactive even though they do not expose a click action.
//!
//! ## Every walk here reads the RETAINED tree, deliberately
//!
//! `compute_layout` returns a `LayoutTree` built from the `Placement`s a
//! [`MeasurePolicy`](cranpose_ui_layout::MeasurePolicy) returned. The scene the
//! renderer is handed is not: [`build_graph_from_applier`] walks the retained
//! node state and drops any node whose `is_placed` is false, and `is_placed` is
//! set by `placeable.place(x, y)` — *not* by pushing a `Placement` into a vec.
//! The two disagreed once and a whole widget set laid out correctly in every
//! assertion while reaching the device as an empty screen.
//!
//! So the layout boxes here come from [`build_layout_tree_from_applier`] and not
//! from the tree `compute_layout` hands back. All three walks — layout,
//! semantics, scene — then apply the same `is_placed` filter, and a node that
//! one of them loses is lost by all of them. A caller cannot be handed bounds
//! for a control the renderer never drew.

use std::{collections::HashMap, rc::Rc};

use cranpose_core::{MemoryApplier, NodeError, NodeId};
use cranpose_foundation::PointerEvent;
use cranpose_render_common::{
    graph::ProjectiveTransform,
    graph_scene::HitGeometry,
    hit_graph::{collect_hits_from_graph, HitGraphSink},
    scene_builder::build_graph_from_applier,
};
use cranpose_ui::{
    build_layout_tree_from_applier, build_semantics_tree_from_applier, LayoutBox, LayoutEngine,
    Point, Rect, SemanticsAction, SemanticsNode, SemanticsRole, SemanticsWidgetRole, Size,
};
use cranpose_ui_graphics::RoundedCornerShape;

/// One semantics node, with the geometry it was placed and drawn at.
///
/// Field-for-field a subset of [`SemanticsNode`] plus the two boxes; the fields
/// that are dropped are the ones a geometry audit has no use for (custom
/// actions, text selection, the canvas children a drawn control publishes —
/// those already carry their own bounds).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSemanticsNode {
    pub node_id: NodeId,
    /// Where the node sits in the tree: layout, text, subcomposition …
    pub role: SemanticsRole,
    /// What a screen reader announces it as — Compose's `Role`.
    pub widget_role: Option<SemanticsWidgetRole>,
    /// The text of a `Text` node, or the content description of anything else.
    pub label: Option<String>,
    pub state_description: Option<String>,
    /// Whether the node carries a click action, which is what separates a
    /// control from something merely described.
    pub clickable: bool,
    /// Whether the renderer carries any pointer-dispatch region for the node.
    /// This also includes gesture-only containers such as scrollable lists.
    pub interactive: bool,
    pub toggled: Option<bool>,
    pub enabled: bool,
    /// The box the measure pass gave this node, in window coordinates.
    pub layout_bounds: Rect,
    /// The box the renderer draws and the hit test inverts, ancestor graphics
    /// layers included. `None` when the hit graph carries no region for it.
    pub touch_bounds: Option<Rect>,
    pub children: Vec<PlacedSemanticsNode>,
}

impl PlacedSemanticsNode {
    /// The box a finger has to find: the drawn quad where there is one, and the
    /// layout box where the node is not a hit target at all.
    pub fn target_bounds(&self) -> Rect {
        self.touch_bounds.unwrap_or(self.layout_bounds)
    }

    /// Depth-first walk, self first.
    pub fn visit(&self, visitor: &mut impl FnMut(&PlacedSemanticsNode)) {
        visitor(self);
        for child in &self.children {
            child.visit(visitor);
        }
    }

    /// Every node in the subtree, self first, in tree order.
    pub fn flatten(&self) -> Vec<&PlacedSemanticsNode> {
        let mut all = Vec::new();
        self.collect(&mut all);
        all
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a PlacedSemanticsNode>) {
        out.push(self);
        for child in &self.children {
            child.collect(out);
        }
    }

    /// Every node a tap would do something to.
    pub fn controls(&self) -> Vec<&PlacedSemanticsNode> {
        self.flatten()
            .into_iter()
            .filter(|node| node.clickable)
            .collect()
    }

    /// A name for an assertion message: the label if there is one, else the
    /// role and the node id, which at least says which one it was.
    pub fn describe(&self) -> String {
        match &self.label {
            Some(label) => format!("{label:?}"),
            None => format!("{:?}#{}", self.role, self.node_id),
        }
    }
}

/// Lay `root` out at `size` and read back its semantics with geometry.
///
/// The applier must already be carrying a runtime handle
/// (`MemoryApplier::set_runtime_handle`) — a subcomposing widget cannot be
/// measured without one. [`crate::testing::ComposeTestRule::placed_semantics`]
/// does that part; this is for a caller driving a `TestComposition` by hand.
///
/// `None` means the composition placed nothing at all, which for a root that
/// composed content is itself the answer to a bug hunt.
pub fn placed_semantics_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    size: Size,
) -> Result<Option<PlacedSemanticsNode>, NodeError> {
    applier.compute_layout(root, size)?;

    let Some(layout) = build_layout_tree_from_applier(applier, root)? else {
        return Ok(None);
    };
    let Some(semantics) = build_semantics_tree_from_applier(applier, root)? else {
        return Ok(None);
    };

    let mut layout_bounds = HashMap::new();
    index_layout_bounds(layout.root(), &mut layout_bounds);

    let mut touch_bounds = HashMap::new();
    if let Some(graph) = build_graph_from_applier(applier, root, 1.0) {
        let mut sink = TouchBoundsSink {
            bounds: &mut touch_bounds,
        };
        collect_hits_from_graph(
            &graph.root,
            ProjectiveTransform::identity(),
            &mut sink,
            None,
        );
    }

    join(semantics.root(), &layout_bounds, &touch_bounds).map(Some)
}

fn index_layout_bounds(layout_box: &LayoutBox, out: &mut HashMap<NodeId, Rect>) {
    out.insert(layout_box.node_id, layout_box.rect);
    for child in &layout_box.children {
        index_layout_bounds(child, out);
    }
}

/// Keeps the FIRST region pushed for a node.
///
/// `collect_hits_from_graph` walks in paint order and a node can reach the sink
/// more than once when it carries both a click handler and a pointer input; the
/// geometry is the same either way, and taking the first keeps the result
/// independent of how many handlers a widget happened to install.
struct TouchBoundsSink<'a> {
    bounds: &'a mut HashMap<NodeId, Rect>,
}

impl HitGraphSink for TouchBoundsSink<'_> {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        _capture_path: &[NodeId],
        geometry: HitGeometry,
        _shape: Option<RoundedCornerShape>,
        _click_actions: &[Rc<dyn Fn(Point)>],
        _pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
    ) {
        self.bounds.entry(node_id).or_insert(geometry.rect);
    }
}

/// Refuses rather than invents.
///
/// The semantics walk and the layout walk apply the SAME `is_placed` filter to
/// the same retained children, so a semantics node without a layout box cannot
/// happen while the two agree. Handing back a zero rect for one would turn a
/// framework divergence into a caller-side "0x0dp control", which is the wrong
/// bug to go and look for.
fn join(
    node: &SemanticsNode,
    layout_bounds: &HashMap<NodeId, Rect>,
    touch_bounds: &HashMap<NodeId, Rect>,
) -> Result<PlacedSemanticsNode, NodeError> {
    let bounds = layout_bounds
        .get(&node.node_id)
        .copied()
        .ok_or(NodeError::MissingContext {
            id: node.node_id,
            reason: "semantics node has no layout box: the semantics walk and the \
                         layout walk disagree about what was placed",
        })?;
    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        children.push(join(child, layout_bounds, touch_bounds)?);
    }
    Ok(PlacedSemanticsNode {
        node_id: node.node_id,
        role: node.role.clone(),
        widget_role: node.widget_role,
        label: match &node.role {
            SemanticsRole::Text { value } => Some(value.clone()),
            _ => node.description.clone(),
        },
        state_description: node.state_description.clone(),
        clickable: node
            .actions
            .iter()
            .any(|action| matches!(action, SemanticsAction::Click { .. })),
        interactive: touch_bounds.contains_key(&node.node_id),
        toggled: node.toggled,
        enabled: node.enabled,
        layout_bounds: bounds,
        touch_bounds: touch_bounds.get(&node.node_id).copied(),
        children,
    })
}
