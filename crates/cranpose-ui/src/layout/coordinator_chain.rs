// A layout node's chain of coordinators: its layout modifiers, each
// measuring the next and the last its measure policy, with the sizes and
// offsets each measure leaves on them.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{NodeId, PackedU64};
use cranpose_foundation::{
    NodeCapabilities, PLAIN_WORDS_MAX, PlainElement, PlainIntrinsic, PlainLayoutFns, PrepareAt,
};
use cranpose_ui_layout::{AlignmentLines, Constraints, Measurables, MeasurePolicy, Placement};
use smallvec::SmallVec;

use super::{
    LayoutNodeContext, ModifierChainInputs,
    core::{Measurable, Placeable},
    inherited_alignment_lines,
    layout_child_measurable::LayoutChildMeasurable,
    policy_intrinsic,
};
use crate::{
    modifier::{Point, Rect as GeometryRect, Size},
    modifier_nodes::{
        FillElement, FractionalOffsetElement, OffsetElement, PaddingElement, SizeElement,
    },
    widgets::nodes::{IntrinsicKind, LayoutNode},
};

/// The intrinsic size `kind` names, of a measurable.
fn measurable_intrinsic(measurable: &dyn Measurable, kind: IntrinsicKind) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => measurable.min_intrinsic_width(height),
        IntrinsicKind::MaxWidth(height) => measurable.max_intrinsic_width(height),
        IntrinsicKind::MinHeight(width) => measurable.min_intrinsic_height(width),
        IntrinsicKind::MaxHeight(width) => measurable.max_intrinsic_height(width),
    }
}

/// The intrinsic size `kind` names, of a layout modifier around `wrapped`.
fn layout_modifier_intrinsic(
    node: &dyn cranpose_foundation::LayoutModifierNode,
    wrapped: &dyn Measurable,
    kind: IntrinsicKind,
    density: f32,
) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => node.min_intrinsic_width(wrapped, height, density),
        IntrinsicKind::MaxWidth(height) => node.max_intrinsic_width(wrapped, height, density),
        IntrinsicKind::MinHeight(width) => node.min_intrinsic_height(wrapped, width, density),
        IntrinsicKind::MaxHeight(width) => node.max_intrinsic_height(wrapped, width, density),
    }
}

fn plain_intrinsic(kind: IntrinsicKind) -> PlainIntrinsic {
    match kind {
        IntrinsicKind::MinWidth(height) => PlainIntrinsic::MinWidth(height),
        IntrinsicKind::MaxWidth(height) => PlainIntrinsic::MaxWidth(height),
        IntrinsicKind::MinHeight(width) => PlainIntrinsic::MinHeight(width),
        IntrinsicKind::MaxHeight(width) => PlainIntrinsic::MaxHeight(width),
    }
}

pub(super) trait SubcomposeCoordinatorLeaf {
    fn measure_leaf(&self, chain: &CoordinatorChain, constraints: Constraints) -> Placeable;
}

enum CoordinatorLeaf<'a> {
    Policy {
        measure_policy: &'a Rc<dyn MeasurePolicy>,
        scope: &'a dyn cranpose_ui_layout::MeasureScope,
        measurables: Measurables<'a>,
        child_states: &'a [LayoutChildMeasurable],
        placements: RefCell<&'a mut Vec<Placement>>,
        placement_indices: RefCell<&'a mut Vec<usize>>,
    },
    Subcompose(&'a dyn SubcomposeCoordinatorLeaf),
}

pub(super) struct CoordinatorFrame<'a> {
    density: f32,
    invalidations: Cell<u8>,
    leaf: CoordinatorLeaf<'a>,
}

impl<'a> CoordinatorFrame<'a> {
    pub(super) fn new(
        measure_policy: &'a Rc<dyn MeasurePolicy>,
        scope: &'a dyn cranpose_ui_layout::MeasureScope,
        measurables: Measurables<'a>,
        child_states: &'a [LayoutChildMeasurable],
        placements: &'a mut Vec<Placement>,
        placement_indices: &'a mut Vec<usize>,
    ) -> Self {
        let density = scope.density();
        Self {
            density,
            invalidations: Cell::new(0),
            leaf: CoordinatorLeaf::Policy {
                measure_policy,
                scope,
                measurables,
                child_states,
                placements: RefCell::new(placements),
                placement_indices: RefCell::new(placement_indices),
            },
        }
    }

    pub(super) fn for_subcompose(density: f32, leaf: &'a dyn SubcomposeCoordinatorLeaf) -> Self {
        Self {
            density,
            invalidations: Cell::new(0),
            leaf: CoordinatorLeaf::Subcompose(leaf),
        }
    }

    pub(super) fn take_invalidations(&self) -> u8 {
        self.invalidations.replace(0)
    }
}

struct CoordinatorLink<'chain, 'frame_ref, 'frame_data> {
    chain: &'chain CoordinatorChain,
    frame: &'frame_ref CoordinatorFrame<'frame_data>,
    index: usize,
    alignment_lines: Cell<AlignmentLines>,
}

impl<'chain, 'frame_ref, 'frame_data> CoordinatorLink<'chain, 'frame_ref, 'frame_data> {
    fn new(
        chain: &'chain CoordinatorChain,
        frame: &'frame_ref CoordinatorFrame<'frame_data>,
        index: usize,
    ) -> Self {
        Self {
            chain,
            frame,
            index,
            alignment_lines: Cell::default(),
        }
    }
}

impl Measurable for CoordinatorLink<'_, '_, '_> {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let placeable = self.chain.measure_from(self.index, self.frame, constraints);
        self.alignment_lines.set(placeable.alignment_lines());
        placeable
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MinWidth(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MaxWidth(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MinHeight(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MaxHeight(width))
    }
}

struct CoordinatorNode {
    modifier_index: usize,
    source: Source,
    measured_size: Cell<Size>,
    /// The constraints this node last measured under.
    measured_constraints: Cell<Constraints>,
    accumulated_offset: Cell<Point>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DirectLayoutKind {
    Unsupported,
    PassThrough,
    Padding,
    Fill,
    Size,
    Offset,
    FractionalOffset,
}

// What a coordinator measures with. A measure runs while the applier is
// free for the children's measures, so the coordinator keeps what it needs
// of its modifier: the node, or a plain element's table and its words as
// the last sync read them, prepared for the sync's density and direction.
enum Source {
    Node(Rc<RefCell<dyn cranpose_foundation::ModifierNode>>),
    Plain {
        layout: &'static PlainLayoutFns,
        kind: DirectLayoutKind,
        words: [u32; PLAIN_WORDS_MAX],
    },
    // A layout-capable modifier that measures nothing itself (e.g. clip,
    // scroll motion/translation context, weight).
    PassThrough,
}

// The layout modifier at an entry of a chain, as a walk reads it.
enum Found<'a> {
    Node(&'a Rc<RefCell<dyn cranpose_foundation::ModifierNode>>),
    Plain(
        &'static PlainLayoutFns,
        DirectLayoutKind,
        &'a [u32; PLAIN_WORDS_MAX],
    ),
    PassThrough,
}

impl<'a> Found<'a> {
    fn at(
        chain: &'a cranpose_foundation::ModifierNodeChain,
        index: usize,
        node_ref: &cranpose_foundation::modifier::ModifierChainNodeRef<'_>,
    ) -> Option<Self> {
        match chain.get_node_rc(index) {
            Some(node) => {
                if node.borrow().as_layout_node().is_some() {
                    Some(Self::Node(node))
                } else {
                    Some(Self::PassThrough)
                }
            }
            None => chain
                .plain_layout(index)
                .map(|(layout, words)| match layout {
                    None => Self::PassThrough,
                    Some(layout) => {
                        let kind = if node_ref.plain::<PaddingElement>().is_some() {
                            DirectLayoutKind::Padding
                        } else if node_ref.plain::<FillElement>().is_some() {
                            DirectLayoutKind::Fill
                        } else if node_ref.plain::<SizeElement>().is_some() {
                            DirectLayoutKind::Size
                        } else if node_ref.plain::<OffsetElement>().is_some() {
                            DirectLayoutKind::Offset
                        } else if node_ref.plain::<FractionalOffsetElement>().is_some() {
                            DirectLayoutKind::FractionalOffset
                        } else {
                            DirectLayoutKind::Unsupported
                        };
                        Self::Plain(layout, kind, words)
                    }
                }),
        }
    }

    fn source(&self, at: PrepareAt) -> Source {
        match *self {
            Self::Node(node) => Source::Node(Rc::clone(node)),
            Self::Plain(layout, kind, words) => Source::Plain {
                layout,
                kind,
                words: layout.prepare(words, at),
            },
            Self::PassThrough => Source::PassThrough,
        }
    }
}

impl CoordinatorNode {
    fn new(modifier_index: usize, source: Source) -> Self {
        Self {
            modifier_index,
            source,
            measured_size: Cell::new(Size::default()),
            measured_constraints: Cell::new(Constraints::tight(0.0, 0.0)),
            accumulated_offset: Cell::new(Point::default()),
        }
    }

    fn direct_kind(&self) -> DirectLayoutKind {
        match self.source {
            Source::Node(_) => DirectLayoutKind::Unsupported,
            Source::Plain { kind, .. } => kind,
            Source::PassThrough => DirectLayoutKind::PassThrough,
        }
    }

    #[inline]
    fn direct_inner_constraints(&self, constraints: Constraints, density: f32) -> Constraints {
        let Source::Plain { kind, words, .. } = &self.source else {
            return constraints;
        };
        match kind {
            DirectLayoutKind::Padding => {
                PaddingElement::from_words(words).inner_constraints_prepared(constraints)
            }
            DirectLayoutKind::Fill => {
                FillElement::from_words(words).inner_constraints(constraints, density)
            }
            DirectLayoutKind::Size => {
                SizeElement::from_words(words).inner_constraints_prepared(constraints)
            }
            DirectLayoutKind::Offset
            | DirectLayoutKind::FractionalOffset
            | DirectLayoutKind::PassThrough
            | DirectLayoutKind::Unsupported => constraints,
        }
    }

    // Takes the words of the coordinator's plain element again from `chain`,
    // whose entries stand where the last walk found them.
    fn read_plain(&mut self, chain: &cranpose_foundation::ModifierNodeChain, at: PrepareAt) {
        if let Source::Plain { layout, words, .. } = &mut self.source
            && let Some((_, found)) = chain.plain_layout(self.modifier_index)
        {
            *words = layout.prepare(found, at);
        }
    }

    // Whether the coordinator stands for `found` at `modifier_index`: the
    // same node, or a plain element of the same measure, whose words it
    // then takes.
    fn takes(&mut self, modifier_index: usize, found: &Found<'_>, at: PrepareAt) -> bool {
        if self.modifier_index != modifier_index {
            return false;
        }
        match (&mut self.source, found) {
            (Source::Node(held), Found::Node(node)) => Rc::ptr_eq(held, node),
            (
                Source::Plain {
                    layout,
                    kind,
                    words,
                },
                Found::Plain(found_layout, found_kind, found_words),
            ) => {
                let same = *kind == *found_kind && std::ptr::eq(*layout, *found_layout);
                if same {
                    *words = layout.prepare(found_words, at);
                }
                same
            }
            (Source::PassThrough, Found::PassThrough) => true,
            _ => false,
        }
    }
}

// What a walk of a chain gave, with the chain revision it read: the inputs
// but the density, which the node holds, and the density's scale, which
// put the offsets on the device grid.
#[derive(Clone, Copy)]
struct SyncedInputs {
    revision: PackedU64,
    scale: f32,
    offset: Point,
    window_root: bool,
    uses_chain: bool,
}

impl SyncedInputs {
    fn of(revision: u64, inputs: ModifierChainInputs) -> Self {
        Self {
            revision: PackedU64::new(revision),
            scale: inputs.density.density(),
            offset: inputs.offset,
            window_root: inputs.window_root,
            uses_chain: inputs.uses_chain,
        }
    }

    fn inputs(self, density: crate::density::Density) -> ModifierChainInputs {
        ModifierChainInputs {
            density,
            window_root: self.window_root,
            offset: self.offset,
            uses_chain: self.uses_chain,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RootKind {
    Node,
    Window,
}

#[derive(Default)]
pub(super) struct CoordinatorChain {
    nodes: Vec<CoordinatorNode>,
    // Trailing pass-through layout-capable entries share the inner content
    // coordinator rect, so they need no per-frame coordinator node.
    trailing_pass_through: usize,
    all_direct: bool,
    /// The size the node's own measure policy measured its content at.
    inner_size: Cell<Size>,
    /// The constraints the node's own measure policy last measured under.
    inner_constraints: Cell<Option<Constraints>>,
    /// The chain revision the nodes were last synced at and what the walk
    /// gave: a node whose modifiers did not change since measures again
    /// without walking its chain.
    synced: Option<SyncedInputs>,
    // Whether a measure placed the coordinators, whose rects the nodes'
    // sizes and offsets give, and whether one of them moved since the scene
    // last heard of the node.
    placed: Cell<bool>,
    moved: Cell<bool>,
}

impl CoordinatorChain {
    pub(super) fn sync(&mut self, node: &LayoutNode) -> ModifierChainInputs {
        let density = node.density();
        let chain = node.modifier_chain();
        let at = PrepareAt {
            density: density.density(),
            direction: chain.layout_direction(),
        };
        let revision = chain.revision();
        if let Some(inputs) = self.try_sync_cached(density, chain, at, revision) {
            return inputs;
        }
        let root = if node.is_window_root() {
            RootKind::Window
        } else {
            RootKind::Node
        };
        self.walk_and_cache(chain, root, density, at, revision)
    }

    pub(super) fn sync_subcompose(
        &mut self,
        density: crate::density::Density,
        chain: &crate::modifier::ModifierChainHandle,
    ) -> ModifierChainInputs {
        let at = PrepareAt {
            density: density.density(),
            direction: chain.layout_direction(),
        };
        let revision = chain.revision();
        if let Some(inputs) = self.try_sync_cached(density, chain, at, revision) {
            return inputs;
        }
        self.walk_and_cache(chain, RootKind::Node, density, at, revision)
    }

    #[inline]
    fn try_sync_cached(
        &mut self,
        density: crate::density::Density,
        chain: &crate::modifier::ModifierChainHandle,
        at: PrepareAt,
        revision: u64,
    ) -> Option<ModifierChainInputs> {
        // A walk reads every entry, so it takes the changes too.
        let changed = chain.take_changed_in_place();
        let synced = self.synced?;
        if synced.revision.get() != revision || synced.scale.to_bits() != at.density.to_bits() {
            return None;
        }
        // Values changed in place: the entries stand where they were,
        // and the coordinators of those entries take their words again.
        if changed != 0 {
            self.read_changed(chain.chain(), changed, at);
        }
        Some(synced.inputs(density))
    }

    // Out of line: steady-state syncs hit the cached revision and never walk.
    #[inline(never)]
    fn walk_and_cache(
        &mut self,
        chain: &crate::modifier::ModifierChainHandle,
        root: RootKind,
        density: crate::density::Density,
        at: PrepareAt,
        revision: u64,
    ) -> ModifierChainInputs {
        let inputs = self.walk(chain, root, density, at);
        self.synced = Some(SyncedInputs::of(revision, inputs));
        // Other modifiers may place other coordinators.
        self.moved.set(true);
        inputs
    }

    // Out of line: most syncs find nothing changed.
    #[inline(never)]
    fn read_changed(
        &mut self,
        chain: &cranpose_foundation::ModifierNodeChain,
        changed: u64,
        at: PrepareAt,
    ) {
        for coordinator in &mut self.nodes {
            let index = u32::try_from(coordinator.modifier_index).unwrap_or(u32::MAX);
            if changed.checked_shr(index).is_some_and(|rest| rest & 1 == 1) {
                coordinator.read_plain(chain, at);
            }
        }
    }

    // Out of line: steady-state syncs hit the cached revision and never walk.
    #[inline(never)]
    fn walk(
        &mut self,
        chain_handle: &crate::modifier::ModifierChainHandle,
        root: RootKind,
        density: crate::density::Density,
        at: PrepareAt,
    ) -> ModifierChainInputs {
        let mut inputs = ModifierChainInputs {
            density,
            window_root: root == RootKind::Window,
            offset: Point::default(),
            uses_chain: false,
        };
        if !chain_handle.has_layout_nodes() {
            self.nodes.clear();
            self.trailing_pass_through = 0;
            self.all_direct = false;
            return inputs;
        }

        let chain = chain_handle.chain();
        // The layout modifiers of the chain in order, with their entries,
        // which a rebuild reads again: a chain holds a few.
        let mut layouts: SmallVec<[(usize, Found<'_>); 8]> = SmallVec::new();
        chain.for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
            let Some(index) = node_ref.entry_index() else {
                return;
            };
            if let Some(found) = Found::at(chain, index, &node_ref) {
                layouts.push((index, found));
            }
            if let Some(offset) = node_ref.plain::<OffsetElement>() {
                let delta = offset.placed(at);
                inputs.offset.x += delta.x;
                inputs.offset.y += delta.y;
            }
        });

        let mut trailing = 0_usize;
        while matches!(layouts.last(), Some((_, Found::PassThrough))) {
            layouts.pop();
            trailing += 1;
        }
        self.trailing_pass_through = trailing;
        inputs.uses_chain = !layouts.is_empty();
        if !inputs.uses_chain {
            self.nodes.clear();
            self.all_direct = false;
            return inputs;
        }
        let matches = layouts.len() == self.nodes.len()
            && self
                .nodes
                .iter_mut()
                .zip(&layouts)
                .all(|(candidate, (index, found))| candidate.takes(*index, found, at));
        if !matches {
            self.rebuild(&layouts, at);
        }
        self.all_direct = self
            .nodes
            .iter()
            .all(|node| node.direct_kind() != DirectLayoutKind::Unsupported);
        inputs
    }

    fn rebuild(&mut self, layouts: &[(usize, Found<'_>)], at: PrepareAt) {
        let mut previous_nodes = std::mem::take(&mut self.nodes);
        // Exactly: most chains hold one layout modifier, where growth would
        // make room for four.
        self.nodes.reserve_exact(layouts.len());
        for (index, found) in layouts {
            match previous_nodes
                .iter_mut()
                .position(|candidate| candidate.takes(*index, found, at))
            {
                Some(position) => self.nodes.push(previous_nodes.swap_remove(position)),
                None => self
                    .nodes
                    .push(CoordinatorNode::new(*index, found.source(at))),
            }
        }
    }

    /// What the last measure through this chain holds for: the policy's own
    /// range, then each layout modifier from the innermost out turns its
    /// content's range into its own; any other node passes its constraints
    /// through.
    pub(super) fn measured_hold(
        &self,
        density: f32,
        policy: &dyn MeasurePolicy,
        measurables: Measurables<'_>,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        let inner_size = self.inner_size.get();
        let mut wrapped = cranpose_ui_layout::WrappedHold {
            size: inner_size,
            hold: self
                .inner_constraints
                .get()
                .and_then(|constraints| policy.measure_hold(measurables, constraints, inner_size)),
        };
        for node in self.nodes.iter().rev() {
            let size = node.measured_size.get();
            let constraints = node.measured_constraints.get();
            let hold = match &node.source {
                Source::Node(held) => match held.borrow().as_layout_node() {
                    Some(layout) => layout.measure_hold(density, constraints, size, wrapped),
                    None => wrapped.hold,
                },
                Source::Plain { layout, words, .. } => {
                    layout.measure_hold(words, density, constraints, size, wrapped)
                }
                Source::PassThrough => wrapped.hold,
            };
            wrapped = cranpose_ui_layout::WrappedHold { size, hold };
        }
        wrapped.hold
    }

    /// Whether the policy's last measure through this chain ignored its
    /// children's sizes. The layout modifiers around it see only its size,
    /// which then stays as it is.
    pub(super) fn ignores_child_sizes(
        &self,
        policy: &dyn MeasurePolicy,
        measurables: Measurables<'_>,
    ) -> bool {
        self.inner_constraints
            .get()
            .is_some_and(|constraints| policy.ignores_child_sizes(measurables, constraints))
    }

    #[inline]
    pub(super) fn is_direct(&self) -> bool {
        self.all_direct
    }

    pub(super) fn enter_direct(&self, mut constraints: Constraints, density: f32) -> Constraints {
        for node in &self.nodes {
            if matches!(node.source, Source::PassThrough) {
                continue;
            }
            node.measured_constraints.set(constraints);
            constraints = node.direct_inner_constraints(constraints, density);
        }
        constraints
    }

    pub(super) fn exit_direct(
        &self,
        mut inner_constraints: Constraints,
        mut size: Size,
        mut alignment_lines: AlignmentLines,
    ) -> (Size, Point, AlignmentLines) {
        let mut moved = self.inner_size.replace(size) != size;
        self.inner_constraints.set(Some(inner_constraints));
        let mut accumulated = Point::default();
        for node in self.nodes.iter().rev() {
            let Source::Plain { kind, words, .. } = &node.source else {
                moved |= node.measured_size.replace(size) != size;
                moved |= node.accumulated_offset.replace(accumulated) != accumulated;
                continue;
            };
            let outer_constraints = node.measured_constraints.get();
            match kind {
                DirectLayoutKind::Padding => {
                    let (outer_size, offset) = PaddingElement::from_words(words)
                        .place_size_prepared(outer_constraints, size);
                    size = outer_size;
                    accumulated.x += offset.x;
                    accumulated.y += offset.y;
                    if offset.y != 0.0 {
                        alignment_lines = alignment_lines.translated(offset.y);
                    }
                }
                DirectLayoutKind::Fill => {
                    size = FillElement::from_words(words).place_size(
                        outer_constraints,
                        inner_constraints,
                        size,
                    );
                }
                DirectLayoutKind::Size => {
                    size =
                        SizeElement::from_words(words).place_size_prepared(inner_constraints, size);
                }
                DirectLayoutKind::Offset => {
                    let offset = OffsetElement::from_words(words).placed_prepared();
                    accumulated.x += offset.x;
                    accumulated.y += offset.y;
                    if offset.y != 0.0 {
                        alignment_lines = alignment_lines.translated(offset.y);
                    }
                }
                DirectLayoutKind::FractionalOffset => {
                    let offset = FractionalOffsetElement::from_words(words).offset_for(size);
                    accumulated.x += offset.x;
                    accumulated.y += offset.y;
                    if offset.y != 0.0 {
                        alignment_lines = alignment_lines.translated(offset.y);
                    }
                }
                DirectLayoutKind::PassThrough | DirectLayoutKind::Unsupported => {}
            }
            moved |= node.measured_size.replace(size) != size;
            moved |= node.accumulated_offset.replace(accumulated) != accumulated;
            inner_constraints = outer_constraints;
        }
        if moved {
            self.moved.set(true);
        }
        (size, accumulated, alignment_lines)
    }

    pub(super) fn record_leaf(&self, size: Size, constraints: Constraints) {
        self.place(&self.inner_size, size);
        self.inner_constraints.set(Some(constraints));
    }

    #[inline]
    fn measure_leaf(&self, frame: &CoordinatorFrame<'_>, constraints: Constraints) -> Placeable {
        match &frame.leaf {
            CoordinatorLeaf::Policy {
                measure_policy,
                scope,
                measurables,
                child_states,
                placements,
                placement_indices,
            } => {
                let mut placements = placements.borrow_mut();
                let measurement =
                    measure_policy.measure_into(*scope, *measurables, constraints, &mut placements);
                self.record_leaf(measurement.size, constraints);
                Placeable::value(
                    measurement.size.width,
                    measurement.size.height,
                    NodeId::default(),
                )
                .with_alignment_lines(
                    inherited_alignment_lines(
                        child_states,
                        &placements,
                        &mut placement_indices.borrow_mut(),
                    )
                    .with_overrides(measurement.alignment_lines),
                )
            }
            CoordinatorLeaf::Subcompose(leaf) => leaf.measure_leaf(self, constraints),
        }
    }

    pub(super) fn measure_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        constraints: Constraints,
    ) -> Placeable {
        let Some(node) = self.nodes.get(index) else {
            return self.measure_leaf(frame, constraints);
        };

        let wrapped = CoordinatorLink::new(self, frame, index + 1);
        node.measured_constraints.set(constraints);
        let mut context = LayoutNodeContext::new(frame.density);
        let measured = match &node.source {
            Source::Node(held) => held
                .borrow()
                .as_layout_node()
                .map(|layout| layout.measure(&mut context, &wrapped, constraints)),
            Source::Plain { layout, words, .. } => {
                Some(layout.measure(words, &mut context, &wrapped, constraints))
            }
            Source::PassThrough => None,
        };
        let bits = context.invalidations();
        if bits != 0 {
            frame.invalidations.set(frame.invalidations.get() | bits);
        }
        // A modifier that measures nothing itself passes its content's
        // measure through.
        let Some(result) = measured else {
            let placeable = wrapped.measure(constraints);
            self.place(
                &node.measured_size,
                Size {
                    width: placeable.width(),
                    height: placeable.height(),
                },
            );
            let child_accumulated = self.total_content_offset_from(index + 1);
            self.place(&node.accumulated_offset, child_accumulated);
            return placeable;
        };

        self.place(&node.measured_size, result.size);
        let local_offset = Point {
            x: result.placement_offset_x,
            y: result.placement_offset_y,
        };
        let child_accumulated = self.total_content_offset_from(index + 1);
        let accumulated = Point {
            x: local_offset.x + child_accumulated.x,
            y: local_offset.y + child_accumulated.y,
        };
        self.place(&node.accumulated_offset, accumulated);

        Placeable::value_with_offset(
            result.size.width,
            result.size.height,
            NodeId::default(),
            (accumulated.x, accumulated.y),
        )
        .with_alignment_lines(
            wrapped
                .alignment_lines
                .get()
                .translated(local_offset.y)
                .with_overrides(result.alignment_lines),
        )
    }

    pub(super) fn intrinsic_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        kind: IntrinsicKind,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return match &frame.leaf {
                CoordinatorLeaf::Policy {
                    measure_policy,
                    measurables,
                    ..
                } => policy_intrinsic(measure_policy.as_ref(), *measurables, kind),
                CoordinatorLeaf::Subcompose(_) => 0.0,
            };
        };
        let wrapped = CoordinatorLink::new(self, frame, index + 1);
        let density = frame.density;
        match &node.source {
            Source::Node(held) => held.borrow().as_layout_node().map_or_else(
                || measurable_intrinsic(&wrapped, kind),
                |layout_node| layout_modifier_intrinsic(layout_node, &wrapped, kind, density),
            ),
            Source::Plain { layout, words, .. } => {
                layout.intrinsic(words, &wrapped, plain_intrinsic(kind), density)
            }
            Source::PassThrough => measurable_intrinsic(&wrapped, kind),
        }
    }

    // Sets a size or an offset the coordinators' rects come from, noting a
    // coordinator that moved: a layer bounded by an inner coordinator
    // changes when that coordinator does, though the node's size may not.
    fn place<T: Copy + PartialEq>(&self, cell: &Cell<T>, value: T) {
        if cell.replace(value) != value {
            self.moved.set(true);
        }
    }

    // Notes that a measure placed the coordinators, and says whether one of
    // them moved since the last note.
    pub(super) fn take_moved(&self) -> bool {
        self.placed.set(true);
        self.moved.take()
    }

    // Where the last measure put the coordinator `ordinal`, relative to the
    // node drawn at its offset: for each layout modifier, from the outermost
    // in, where it was placed and its size, then the node's content. A node
    // of `node_size` without layout modifiers has its content alone, which
    // fills it. `None` before a measure placed them.
    pub(super) fn placed_rect(&self, ordinal: usize, node_size: Size) -> Option<GeometryRect> {
        let inputs = self.synced.filter(|_| self.placed.get())?;
        if !inputs.uses_chain {
            return (ordinal <= self.trailing_pass_through)
                .then(|| GeometryRect::from_size(node_size));
        }
        let content = self.total_content_offset_from(0);
        let placed = |inner_offset: Point, size: Size| GeometryRect {
            x: content.x - inner_offset.x - inputs.offset.x,
            y: content.y - inner_offset.y - inputs.offset.y,
            width: size.width,
            height: size.height,
        };
        match self.nodes.get(ordinal) {
            Some(node) => Some(placed(
                node.accumulated_offset.get(),
                node.measured_size.get(),
            )),
            None => (ordinal <= self.nodes.len() + self.trailing_pass_through)
                .then(|| placed(Point::default(), self.inner_size.get())),
        }
    }

    fn total_content_offset_from(&self, index: usize) -> Point {
        self.nodes
            .get(index)
            .map_or_default(|node| node.accumulated_offset.get())
    }
}
