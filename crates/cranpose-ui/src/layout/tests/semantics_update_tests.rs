use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{Applier, MemoryApplier, Node, NodeError, NodeId};
use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState,
    SemanticsConfiguration, SemanticsNode as SemanticsModifier, SemanticsReach,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement};

use super::{
    LayoutNode, MeasureLayoutOptions, SemanticsNode, SemanticsTree,
    build_semantics_tree_from_applier, core::Measurable, measure_layout_with_options,
    update_semantics_tree_from_applier,
};
use crate::{
    layout::policies::LeafMeasurePolicy,
    modifier::{Modifier, Size},
};

const ROW_HEIGHT: f32 = 20.0;

/// Stacks its children one row apart, the whole stack moved up by `offset`.
struct ScrolledStackPolicy {
    offset: Rc<Cell<f32>>,
}

impl MeasurePolicy for ScrolledStackPolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        for (index, measurable) in measurables.iter().enumerate() {
            let placeable = measurable.measure(constraints);
            let y = index as f32 * ROW_HEIGHT - self.offset.get();
            placements.push(Placement::new(placeable.node_id(), 0.0, y, 0));
        }
        MeasureResult::new(
            Size {
                width: constraints.max_width,
                height: constraints.max_height,
            },
            placements,
        )
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }
}

/// A label that reports only what its updates bring, counting its merges.
#[derive(Debug)]
struct StableLabelElement {
    label: Rc<Cell<&'static str>>,
    merges: Rc<Cell<usize>>,
}

impl PartialEq for StableLabelElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.label, &other.label) && Rc::ptr_eq(&self.merges, &other.merges)
    }
}

impl Hash for StableLabelElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.label).hash(state);
    }
}

impl ModifierNodeElement for StableLabelElement {
    type Node = StableLabelNode;

    fn create(&self) -> Self::Node {
        StableLabelNode {
            label: Rc::clone(&self.label),
            merges: Rc::clone(&self.merges),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.label = Rc::clone(&self.label);
        node.merges = Rc::clone(&self.merges);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::SEMANTICS
    }
}

struct StableLabelNode {
    label: Rc<Cell<&'static str>>,
    merges: Rc<Cell<usize>>,
    state: NodeState,
}

impl DelegatableNode for StableLabelNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for StableLabelNode {
    fn as_semantics_node(&self) -> Option<&dyn SemanticsModifier> {
        Some(self)
    }

    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsModifier> {
        Some(self)
    }
}

impl SemanticsModifier for StableLabelNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        self.merges.set(self.merges.get() + 1);
        config.content_description = Some(self.label.get().to_string());
    }

    fn reach(&self) -> SemanticsReach {
        SemanticsReach::default()
    }
}

fn mounted(applier: &mut MemoryApplier, node: LayoutNode) -> NodeId {
    let id = applier.create(Box::new(node));
    applier
        .with_node::<LayoutNode, _>(id, |node| node.set_node_id(id))
        .unwrap_or_else(|err| panic!("node #{id} was just created: {err}"));
    id
}

struct Scroller {
    applier: MemoryApplier,
    root: NodeId,
    offset: Rc<Cell<f32>>,
    merges: Rc<Cell<usize>>,
    tree: Option<SemanticsTree>,
}

impl Scroller {
    fn new() -> Self {
        let offset = Rc::new(Cell::new(0.0));
        let merges = Rc::new(Cell::new(0));
        let mut applier = MemoryApplier::new();
        let root = mounted(
            &mut applier,
            LayoutNode::new(
                Modifier::from_element(StableLabelElement {
                    label: Rc::new(Cell::new("List")),
                    merges: Rc::clone(&merges),
                }),
                Rc::new(ScrolledStackPolicy {
                    offset: Rc::clone(&offset),
                }),
            ),
        );
        Self {
            applier,
            root,
            offset,
            merges,
            tree: None,
        }
    }

    fn row_with(&mut self, modifier: Modifier) -> NodeId {
        mounted(
            &mut self.applier,
            LayoutNode::new(
                modifier,
                Rc::new(LeafMeasurePolicy::new(Size {
                    width: 100.0,
                    height: ROW_HEIGHT,
                })),
            ),
        )
    }

    fn row(&mut self, label: &Rc<Cell<&'static str>>) -> NodeId {
        let element = StableLabelElement {
            label: Rc::clone(label),
            merges: Rc::clone(&self.merges),
        };
        self.row_with(Modifier::from_element(element))
    }

    fn labelled_row(&mut self, label: &'static str) -> NodeId {
        self.row(&Rc::new(Cell::new(label)))
    }

    fn stack(&mut self, children: &[NodeId]) -> Result<NodeId, NodeError> {
        let mut stack = LayoutNode::new(
            Modifier::empty(),
            Rc::new(ScrolledStackPolicy {
                offset: Rc::new(Cell::new(0.0)),
            }),
        );
        stack.children.extend_from_slice(children);
        let stack = mounted(&mut self.applier, stack);
        for &child in children {
            self.applier
                .with_node::<LayoutNode, _>(child, |row| row.set_parent_for_bubbling(stack))?;
        }
        Ok(stack)
    }

    fn with_row<R>(&mut self, row: NodeId, f: impl FnOnce(&mut LayoutNode) -> R) -> R {
        self.applier
            .with_node::<LayoutNode, _>(row, f)
            .unwrap_or_else(|err| panic!("row #{row} is a layout node: {err}"))
    }

    fn set_children(&mut self, children: &[NodeId]) -> Result<(), NodeError> {
        let root = self.root;
        for &child in children {
            self.applier
                .with_node::<LayoutNode, _>(child, |row| row.set_parent_for_bubbling(root))?;
        }
        self.applier.with_node::<LayoutNode, _>(root, |root| {
            root.children.clear();
            root.children.extend_from_slice(children);
            root.mark_needs_measure();
        })
    }

    /// Lays the tree out again, as the shell's pass that moved nodes does:
    /// collecting no semantics on the way.
    fn relayout(&mut self) -> Result<(), NodeError> {
        self.applier
            .with_node::<LayoutNode, _>(self.root, |root| root.mark_needs_measure())?;
        measure_layout_with_options(
            &mut self.applier,
            self.root,
            Size::new(100.0, 100.0),
            MeasureLayoutOptions {
                collect_semantics: false,
                build_layout_tree: false,
            },
        )?;
        Ok(())
    }

    /// Updates the kept tree, checks it against a tree built afresh, and
    /// answers how many stable labels the update merged.
    fn update(&mut self) -> Result<usize, NodeError> {
        let before = self.merges.get();
        update_semantics_tree_from_applier(&mut self.applier, self.root, &mut self.tree)?;
        let merged = self.merges.get() - before;
        let fresh = build_semantics_tree_from_applier(&mut self.applier, self.root)?;
        assert_eq!(
            self.tree.as_ref().map(|tree| outline(tree.root())),
            fresh.as_ref().map(|tree| outline(tree.root())),
            "an updated tree says what a fresh one does"
        );
        Ok(merged)
    }

    fn rows(&self) -> Vec<(NodeId, Option<String>, f32)> {
        self.tree.as_ref().map_or_else(Vec::new, |tree| {
            tree.root()
                .children
                .iter()
                .map(|row| (row.node_id, row.description.clone(), row.bounds.y))
                .collect()
        })
    }
}

/// What a reader could tell apart in a tree: each node's id, generation,
/// label, state, bounds and modality, in order.
fn outline(node: &SemanticsNode) -> Vec<String> {
    let mut lines = Vec::new();
    fn walk(node: &SemanticsNode, depth: usize, lines: &mut Vec<String>) {
        lines.push(format!(
            "{:depth$}#{} g{} {:?} {:?} {:?} modal={} focusable={}",
            "",
            node.node_id,
            node.node_generation,
            node.description,
            node.state_description,
            node.bounds,
            node.is_modal,
            node.focusable,
        ));
        for child in &node.children {
            walk(child, depth + 1, lines);
        }
    }
    walk(node, 0, &mut lines);
    lines
}

#[test]
fn new_semantics_children_allocate_for_the_known_sibling_count() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    for count in [0, 1, 2, 3, 4] {
        let mut scroller = Scroller::new();
        let rows: Vec<_> = (0..count).map(|_| scroller.labelled_row("Row")).collect();
        scroller.set_children(&rows)?;
        scroller.relayout()?;
        scroller.update()?;
        let fresh = build_semantics_tree_from_applier(&mut scroller.applier, scroller.root)?;
        for tree in [scroller.tree.as_ref(), fresh.as_ref()] {
            let children = &tree.expect("placed root").root().children;
            assert_eq!(children.len(), count);
            assert_eq!(children.capacity(), count, "sibling count {count}");
            assert!(children.iter().all(|child| child.children.capacity() == 0));
        }
        let before = scroller
            .tree
            .as_ref()
            .expect("placed root")
            .root()
            .children
            .as_ptr();
        scroller.offset.set(5.0);
        scroller.relayout()?;
        scroller.update()?;
        let children = &scroller.tree.as_ref().expect("placed root").root().children;
        assert_eq!(children.as_ptr(), before, "moving retains the allocation");
        assert_eq!(children.capacity(), count);
    }
    Ok(())
}

#[test]
fn unplaced_children_do_not_allocate_semantics_sibling_storage() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let first = scroller.labelled_row("First");
    let second = scroller.labelled_row("Second");
    scroller.set_children(&[first, second])?;
    scroller.relayout()?;
    for row in [first, second] {
        scroller.with_row(row, |row| row.clear_placed());
    }
    scroller.update()?;
    assert_eq!(
        scroller
            .tree
            .as_ref()
            .expect("placed root")
            .root()
            .children
            .capacity(),
        0
    );
    scroller.relayout()?;
    scroller.update()?;
    let children = &scroller.tree.as_ref().expect("placed root").root().children;
    assert_eq!(children.len(), 2);
    assert_eq!(children.capacity(), 2);
    Ok(())
}

#[test]
fn unplaced_siblings_do_not_inflate_the_initial_semantics_allocation() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let rows: Vec<_> = (0..100).map(|_| scroller.labelled_row("Row")).collect();
    scroller.set_children(&rows)?;
    scroller.relayout()?;
    for &row in &rows[1..] {
        scroller.with_row(row, |row| row.clear_placed());
    }
    scroller.update()?;
    let children = &scroller.tree.as_ref().expect("placed root").root().children;
    assert_eq!(children.len(), 1);
    assert!(children.capacity() <= 4);
    Ok(())
}

#[test]
fn a_moved_row_keeps_its_report_and_takes_its_new_bounds() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let first = scroller.labelled_row("First");
    let second = scroller.labelled_row("Second");
    scroller.set_children(&[first, second])?;
    scroller.relayout()?;
    assert_eq!(
        scroller.update()?,
        3,
        "a new tree merges the list and every row"
    );

    scroller.offset.set(5.0);
    scroller.relayout()?;
    assert_eq!(scroller.update()?, 0, "moving merges nothing");
    assert_eq!(
        scroller.rows(),
        vec![
            (first, Some("First".into()), -5.0),
            (second, Some("Second".into()), ROW_HEIGHT - 5.0),
        ],
        "the rows keep their reports and move with the stack"
    );
    Ok(())
}

#[test]
fn a_row_that_marks_its_semantics_merges_again() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let label = Rc::new(Cell::new("Before"));
    let first = scroller.row(&label);
    let second = scroller.labelled_row("Second");
    scroller.set_children(&[first, second])?;
    scroller.relayout()?;
    scroller.update()?;

    label.set("After");
    scroller
        .applier
        .with_node::<LayoutNode, _>(first, |row| row.mark_needs_semantics())?;
    assert_eq!(scroller.update()?, 1, "only the marked row merges");
    assert_eq!(scroller.rows()[0].1.as_deref(), Some("After"));
    Ok(())
}

#[test]
fn a_change_below_merges_only_the_node_that_changed() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let label = Rc::new(Cell::new("Before"));
    let row = scroller.row(&label);
    scroller.set_children(&[row])?;
    scroller.relayout()?;
    scroller.update()?;

    label.set("After");
    cranpose_core::bubble_semantics_dirty(&mut scroller.applier, row);
    let root_dirty = scroller
        .applier
        .with_node::<LayoutNode, _>(scroller.root, |root| {
            (root.needs_semantics(), root.semantics_changed())
        })?;
    assert_eq!(
        root_dirty,
        (true, false),
        "the list only learns that something below it changed"
    );
    assert_eq!(scroller.update()?, 1, "the list keeps its report");
    assert_eq!(scroller.rows()[0].1.as_deref(), Some("After"));
    Ok(())
}

#[test]
fn rows_that_come_and_go_are_matched_by_id() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let rows: Vec<NodeId> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|label| scroller.labelled_row(label))
        .collect();
    scroller.set_children(&rows)?;
    scroller.relayout()?;
    assert_eq!(scroller.update()?, 5);

    let appended = scroller.labelled_row("e");
    scroller.set_children(&[rows[1], rows[2], rows[3], appended])?;
    scroller.relayout()?;
    assert_eq!(
        scroller.update()?,
        1,
        "a row leaving the top and one joining"
    );

    let prepended = scroller.labelled_row("x");
    scroller.set_children(&[prepended, rows[1], rows[2], rows[3], appended])?;
    scroller.relayout()?;
    assert_eq!(scroller.update()?, 1, "a row joining the top");

    scroller.set_children(&[appended, rows[3], prepended, rows[2], rows[1]])?;
    scroller.relayout()?;
    assert_eq!(scroller.update()?, 0, "reordered rows are found by id");
    assert_eq!(
        scroller
            .rows()
            .iter()
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        vec![appended, rows[3], prepended, rows[2], rows[1]],
        "reordered rows follow the new order"
    );
    Ok(())
}

#[test]
fn a_row_left_unplaced_leaves_the_tree_and_comes_back() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let rows: Vec<NodeId> = ["a", "b", "c"]
        .into_iter()
        .map(|label| scroller.labelled_row(label))
        .collect();
    scroller.set_children(&rows)?;
    scroller.relayout()?;
    scroller.update()?;

    scroller
        .applier
        .with_node::<LayoutNode, _>(rows[1], |row| row.clear_placed())?;
    scroller.update()?;
    assert_eq!(
        scroller
            .rows()
            .iter()
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        vec![rows[0], rows[2]]
    );

    scroller.relayout()?;
    assert_eq!(scroller.update()?, 1, "the returning row merges again");
    assert_eq!(scroller.rows().len(), 3);
    Ok(())
}

#[test]
fn a_recorder_reading_live_state_merges_on_every_update() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let position = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&position);
    let list = scroller.row_with(Modifier::empty().semantics(move |config| {
        config.state_description = Some(format!("at {}", recorded.get()));
    }));
    scroller.set_children(&[list])?;
    scroller.relayout()?;
    scroller.update()?;

    position.set(3);
    scroller.offset.set(1.0);
    scroller.relayout()?;
    scroller.update()?;
    let state = scroller
        .tree
        .as_ref()
        .and_then(|tree| tree.root().children.first())
        .and_then(|row| row.state_description.clone());
    assert_eq!(state.as_deref(), Some("at 3"));
    Ok(())
}

#[test]
fn a_moved_row_keeps_a_stable_recorders_report() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let runs = Rc::new(Cell::new(0));
    let (stable_runs, live_runs) = (Rc::clone(&runs), Rc::new(Cell::new(0)));
    let counted = Rc::clone(&live_runs);
    let stable = scroller.row_with(Modifier::empty().stable_semantics(move |config| {
        stable_runs.set(stable_runs.get() + 1);
        config.content_description = Some("Saved".into());
    }));
    let live = scroller.row_with(Modifier::empty().semantics(move |config| {
        counted.set(counted.get() + 1);
        config.content_description = Some("Clock".into());
    }));
    scroller.set_children(&[stable, live])?;
    scroller.relayout()?;
    scroller.update()?;

    let mut moved = || -> Result<(usize, usize), NodeError> {
        let before = (runs.get(), live_runs.get());
        scroller.offset.set(scroller.offset.get() + 1.0);
        scroller.relayout()?;
        let merged = scroller.update()?;
        assert_eq!(merged, 0);
        // `update` checks against a fresh tree, which runs each recorder once.
        Ok((runs.get() - before.0 - 1, live_runs.get() - before.1 - 1))
    };
    moved()?;
    assert_eq!(
        moved()?,
        (0, 1),
        "a move runs the live recorder again and leaves the stable one"
    );
    Ok(())
}

#[test]
fn the_top_modal_follows_updates() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let modal = Rc::new(Cell::new(false));
    let recorded = Rc::clone(&modal);
    let row = scroller.labelled_row("Page");
    let dialog = scroller
        .row_with(Modifier::empty().semantics(move |config| config.is_modal = recorded.get()));
    scroller.set_children(&[row, dialog])?;
    scroller.relayout()?;
    scroller.update()?;
    let root_id = |scroller: &Scroller| scroller.tree.as_ref().map(|tree| tree.root().node_id);
    assert_eq!(root_id(&scroller), Some(scroller.root));

    modal.set(true);
    scroller.relayout()?;
    scroller.update()?;
    assert_eq!(root_id(&scroller), Some(dialog));

    modal.set(false);
    scroller.relayout()?;
    scroller.update()?;
    assert_eq!(root_id(&scroller), Some(scroller.root));
    Ok(())
}

#[test]
fn a_scrolled_subtree_moves_without_being_read() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let deep = scroller.labelled_row("Deep");
    let inner = scroller.stack(&[deep])?;
    let card = scroller.stack(&[inner])?;
    let other = scroller.labelled_row("Other");
    scroller.set_children(&[card, other])?;
    scroller.relayout()?;
    scroller.update()?;

    for offset in [0.1, 0.35, 7.3, 7.3] {
        scroller.offset.set(offset);
        scroller.relayout()?;
        scroller.with_row(deep, |row| row.mark_needs_semantics());
        assert_eq!(scroller.update()?, 0, "moving merges nothing");
        assert!(
            scroller.with_row(deep, |row| row.semantics_changed()),
            "a row whose parents moved it is not read again"
        );
        scroller.with_row(deep, |row| row.clear_needs_semantics());
    }
    let deep_y = scroller
        .tree
        .as_ref()
        .and_then(|tree| tree.root().children.first())
        .and_then(|card| card.children.first())
        .and_then(|inner| inner.children.first())
        .map(|deep| deep.bounds.y);
    assert_eq!(deep_y, Some(-7.3), "the deep row moved with the list");
    Ok(())
}

#[test]
fn a_live_recorder_below_a_clean_subtree_merges_on_every_update() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let position = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&position);
    let live = scroller.row_with(Modifier::empty().semantics(move |config| {
        config.state_description = Some(format!("at {}", recorded.get()));
    }));
    let inner = scroller.stack(&[live])?;
    let card = scroller.stack(&[inner])?;
    scroller.set_children(&[card])?;
    scroller.relayout()?;
    scroller.update()?;

    for at in 1..3 {
        position.set(at);
        scroller.update()?;
    }
    let state = scroller
        .tree
        .as_ref()
        .and_then(|tree| tree.root().children.first())
        .and_then(|card| card.children.first())
        .and_then(|inner| inner.children.first())
        .and_then(|live| live.state_description.clone());
    assert_eq!(state.as_deref(), Some("at 2"));
    Ok(())
}

#[test]
fn a_modal_in_a_subtree_an_update_keeps_stays_on_top() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let label = Rc::new(Cell::new("Before"));
    let page = scroller.row(&label);
    let dialog = scroller.row_with(Modifier::empty().stable_semantics(|config| {
        config.is_modal = true;
    }));
    let card = scroller.stack(&[dialog])?;
    scroller.set_children(&[page, card])?;
    scroller.relayout()?;
    scroller.update()?;
    let root_id = |scroller: &Scroller| scroller.tree.as_ref().map(|tree| tree.root().node_id);
    assert_eq!(root_id(&scroller), Some(dialog));

    label.set("After");
    scroller.with_row(page, |row| row.mark_needs_semantics());
    cranpose_core::bubble_semantics_dirty(&mut scroller.applier, page);
    assert_eq!(scroller.update()?, 1, "only the page merges");
    assert_eq!(
        root_id(&scroller),
        Some(dialog),
        "the dialog the update did not read still takes the window over"
    );
    Ok(())
}

#[test]
fn a_row_unplaced_deep_in_a_subtree_leaves_the_tree_and_comes_back() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let rows: Vec<NodeId> = ["a", "b"]
        .into_iter()
        .map(|label| scroller.labelled_row(label))
        .collect();
    let inner = scroller.stack(&rows)?;
    let card = scroller.stack(&[inner])?;
    scroller.set_children(&[card])?;
    scroller.relayout()?;
    scroller.update()?;

    let inner_rows = |scroller: &Scroller| {
        scroller
            .tree
            .as_ref()
            .and_then(|tree| tree.root().children.first())
            .and_then(|card| card.children.first())
            .map(|inner| {
                inner
                    .children
                    .iter()
                    .map(|row| row.node_id)
                    .collect::<Vec<_>>()
            })
    };
    scroller.with_row(rows[0], |row| row.clear_placed());
    scroller.update()?;
    assert_eq!(inner_rows(&scroller), Some(vec![rows[1]]));

    scroller.with_row(inner, |stack| stack.mark_needs_measure());
    cranpose_core::bubble_measure_dirty(&mut scroller.applier, inner);
    scroller.relayout()?;
    scroller.update()?;
    assert_eq!(inner_rows(&scroller), Some(rows));
    Ok(())
}

#[test]
fn a_row_that_grows_moves_the_rows_after_it() -> Result<(), NodeError> {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut scroller = Scroller::new();
    let height = Rc::new(Cell::new(ROW_HEIGHT));
    let tall = mounted(
        &mut scroller.applier,
        LayoutNode::new(
            Modifier::empty().content_description("Tall"),
            Rc::new(GrowingLeafPolicy {
                height: Rc::clone(&height),
            }),
        ),
    );
    let below = scroller.labelled_row("Below");
    let inner = scroller.stack(&[tall, below])?;
    let card = scroller.stack(&[inner])?;
    scroller.set_children(&[card])?;
    scroller.relayout()?;
    scroller.update()?;

    height.set(ROW_HEIGHT * 1.5);
    scroller.with_row(tall, |row| row.mark_needs_measure());
    cranpose_core::bubble_measure_dirty(&mut scroller.applier, tall);
    scroller.relayout()?;
    assert_eq!(scroller.update()?, 0, "growing merges nothing");
    let heights = scroller
        .tree
        .as_ref()
        .and_then(|tree| tree.root().children.first())
        .and_then(|card| card.children.first())
        .map(|inner| {
            inner
                .children
                .iter()
                .map(|row| row.bounds.height)
                .collect::<Vec<_>>()
        });
    assert_eq!(heights, Some(vec![ROW_HEIGHT * 1.5, ROW_HEIGHT]));
    Ok(())
}

struct GrowingLeafPolicy {
    height: Rc<Cell<f32>>,
}

impl MeasurePolicy for GrowingLeafPolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        MeasureResult::new(Size::new(100.0, self.height.get()), Vec::new())
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        100.0
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        100.0
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.height.get()
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.height.get()
    }
}
