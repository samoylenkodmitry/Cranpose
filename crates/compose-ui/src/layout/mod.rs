pub mod core;
pub mod policies;

use compose_core::collections::map::Entry;
use compose_core::collections::map::HashMap;
use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use compose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost,
    MemoryApplier, Node, NodeError, NodeId, Phase, RuntimeHandle, SlotBackend, SlotsHost,
    SnapshotStateObserver,
};

#[cfg(test)]
use self::core::VerticalAlignment;
use self::core::{HorizontalAlignment, LinearArrangement, Measurable, Placeable};
use crate::modifier::{
    DimensionConstraint, EdgeInsets, Modifier, Point, Rect as GeometryRect, Size,
};
use crate::subcompose_layout::SubcomposeLayoutNode;
use crate::widgets::nodes::{
    ButtonNode, IntrinsicKind, LayoutNode, LayoutNodeCacheHandles, SpacerNode, TextNode,
};
use compose_ui_layout::{Constraints, MeasurePolicy};

static NEXT_CACHE_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Discrete event callback reference produced during semantics extraction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticsCallback {
    node_id: NodeId,
}

impl SemanticsCallback {
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Semantics action exposed to the input system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsAction {
    Click { handler: SemanticsCallback },
}

/// Semantic role describing how a node should participate in accessibility and hit testing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsRole {
    Layout,
    Subcompose,
    Text { value: String },
    Spacer,
    Button,
    Unknown,
}

/// A single node within the semantics tree.
#[derive(Clone, Debug)]
pub struct SemanticsNode {
    pub node_id: NodeId,
    pub role: SemanticsRole,
    pub actions: Vec<SemanticsAction>,
    pub children: Vec<SemanticsNode>,
}

impl SemanticsNode {
    fn new(
        node_id: NodeId,
        role: SemanticsRole,
        actions: Vec<SemanticsAction>,
        children: Vec<SemanticsNode>,
    ) -> Self {
        Self {
            node_id,
            role,
            actions,
            children,
        }
    }
}

/// Rooted semantics tree extracted after layout.
#[derive(Clone, Debug)]
pub struct SemanticsTree {
    root: SemanticsNode,
}

impl SemanticsTree {
    fn new(root: SemanticsNode) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &SemanticsNode {
        &self.root
    }
}

/// Result of running layout for a Compose tree.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub fn into_root(self) -> LayoutBox {
        self.root
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: NodeId,
    pub rect: GeometryRect,
    pub node_data: LayoutNodeData,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(
        node_id: NodeId,
        rect: GeometryRect,
        node_data: LayoutNodeData,
        children: Vec<LayoutBox>,
    ) -> Self {
        Self {
            node_id,
            rect,
            node_data,
            children,
        }
    }
}

/// Snapshot of the data required to render a layout node.
#[derive(Debug, Clone)]
pub struct LayoutNodeData {
    pub modifier: Modifier,
    pub kind: LayoutNodeKind,
}

impl LayoutNodeData {
    pub fn new(modifier: Modifier, kind: LayoutNodeKind) -> Self {
        Self { modifier, kind }
    }
}

/// Classification of the node captured inside a [`LayoutBox`].
#[derive(Clone)]
pub enum LayoutNodeKind {
    Layout,
    Subcompose,
    Text { value: String },
    Spacer,
    Button { on_click: Rc<RefCell<dyn FnMut()>> },
    Unknown,
}

impl fmt::Debug for LayoutNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutNodeKind::Layout => f.write_str("Layout"),
            LayoutNodeKind::Subcompose => f.write_str("Subcompose"),
            LayoutNodeKind::Text { value } => f.debug_struct("Text").field("value", value).finish(),
            LayoutNodeKind::Spacer => f.write_str("Spacer"),
            LayoutNodeKind::Button { .. } => f.write_str("Button"),
            LayoutNodeKind::Unknown => f.write_str("Unknown"),
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError>;
}

impl LayoutEngine for MemoryApplier {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError> {
        let measurements = measure_layout(self, root, max_size)?;
        Ok(measurements.into_layout_tree())
    }
}

/// Result of running the measure pass for a Compose layout tree.
#[derive(Debug, Clone)]
pub struct LayoutMeasurements {
    root: Rc<MeasuredNode>,
    semantics: SemanticsTree,
    layout_tree: LayoutTree,
}

impl LayoutMeasurements {
    fn new(root: Rc<MeasuredNode>, semantics: SemanticsTree, layout_tree: LayoutTree) -> Self {
        Self {
            root,
            semantics,
            layout_tree,
        }
    }

    /// Returns the measured size of the root node.
    pub fn root_size(&self) -> Size {
        self.root.size
    }

    pub fn semantics_tree(&self) -> &SemanticsTree {
        &self.semantics
    }

    /// Consumes the measurements and produces a [`LayoutTree`].
    pub fn into_layout_tree(self) -> LayoutTree {
        self.layout_tree
    }

    /// Returns a borrowed [`LayoutTree`] for rendering.
    pub fn layout_tree(&self) -> LayoutTree {
        self.layout_tree.clone()
    }
}

/// Runs the measure phase for the subtree rooted at `root`.
pub fn measure_layout(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
) -> Result<LayoutMeasurements, NodeError> {
    let constraints = Constraints {
        min_width: 0.0,
        max_width: max_size.width,
        min_height: 0.0,
        max_height: max_size.height,
    };
    let original_applier = std::mem::replace(applier, MemoryApplier::new());
    let applier_host = Rc::new(ConcreteApplierHost::new(original_applier));
    let mut builder = LayoutBuilder::new(Rc::clone(&applier_host));
    let measured = builder.measure_node(root, normalize_constraints(constraints))?;
    let metadata = {
        let mut applier_ref = applier_host.borrow_typed();
        collect_runtime_metadata(&mut *applier_ref, &measured)?
    };
    drop(builder);
    let applier_inner = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("layout builder should be sole owner of applier host"))
        .into_inner();
    *applier = applier_inner;
    let semantics_root = build_semantics_node(&measured, &metadata);
    let semantics = SemanticsTree::new(semantics_root);
    let layout_tree = build_layout_tree_from_metadata(&measured, &metadata);
    Ok(LayoutMeasurements::new(measured, semantics, layout_tree))
}

struct LayoutBuilder {
    state: Rc<RefCell<LayoutBuilderState>>,
}

impl LayoutBuilder {
    fn new(applier: Rc<ConcreteApplierHost<MemoryApplier>>) -> Self {
        Self {
            state: Rc::new(RefCell::new(LayoutBuilderState::new(applier))),
        }
    }

    fn measure_node(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        LayoutBuilderState::measure_node(Rc::clone(&self.state), node_id, constraints)
    }

    fn measure_node_reuse(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        LayoutBuilderState::measure_node(Rc::clone(&self.state), node_id, constraints)
    }

    fn set_runtime_handle(&mut self, handle: Option<RuntimeHandle>) {
        self.state.borrow_mut().runtime_handle = handle;
    }

    fn set_cache_epoch(&mut self, epoch: u64) {
        self.state.borrow_mut().cache_epoch = epoch;
    }

    fn cache_epoch(&self) -> u64 {
        self.state.borrow().cache_epoch
    }
}

struct LayoutBuilderState {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    slots: SlotBackend,
    cache_epoch: u64,
    tmp_measurables: Vec<Box<dyn Measurable>>,
    tmp_records: Vec<(NodeId, ChildRecord)>,
}

impl LayoutBuilderState {
    fn new(applier: Rc<ConcreteApplierHost<MemoryApplier>>) -> Self {
        let runtime_handle = applier.borrow_typed().runtime_handle();
        Self {
            applier,
            runtime_handle,
            slots: SlotBackend::default(),
            cache_epoch: NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed),
            tmp_measurables: Vec::new(),
            tmp_records: Vec::new(),
        }
    }

    fn with_applier_result<R>(
        state_rc: &Rc<RefCell<Self>>,
        f: impl FnOnce(&mut MemoryApplier) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let host = {
            let state = state_rc.borrow();
            Rc::clone(&state.applier)
        };
        let mut applier = host.borrow_typed();
        f(&mut *applier)
    }

    fn measure_node(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let constraints = normalize_constraints(constraints);

        if let Some(subcompose) =
            Self::try_measure_subcompose(Rc::clone(&state_rc), node_id, constraints)?
        {
            return Ok(subcompose);
        }

        if let Some(snapshot) = Self::with_applier_result(&state_rc, |applier| {
            match applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                LayoutNodeSnapshot::from_layout_node(layout_node)
            }) {
                Ok(snapshot) => Ok(Some(snapshot)),
                Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => Ok(None),
                Err(err) => Err(err),
            }
        })? {
            return Self::measure_layout_node(Rc::clone(&state_rc), node_id, snapshot, constraints);
        }

        if let Some(text) =
            Self::with_applier_result(&state_rc, |applier| try_clone::<TextNode>(applier, node_id))?
        {
            return Ok(measure_text(node_id, &text, constraints));
        }

        if let Some(spacer) = Self::with_applier_result(&state_rc, |applier| {
            try_clone::<SpacerNode>(applier, node_id)
        })? {
            return Ok(measure_spacer(node_id, &spacer, constraints));
        }

        if let Some(button) = Self::with_applier_result(&state_rc, |applier| {
            try_clone::<ButtonNode>(applier, node_id)
        })? {
            return Self::measure_button(Rc::clone(&state_rc), node_id, button, constraints);
        }

        Ok(Rc::new(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
            Vec::new(),
        )))
    }

    fn try_measure_subcompose(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let applier_host = {
            let state = state_rc.borrow();
            Rc::clone(&state.applier)
        };

        let (node_handle, props, offset) = {
            let mut applier = applier_host.borrow_typed();
            let node = match applier.get_mut(node_id) {
                Ok(node) => node,
                Err(NodeError::Missing { .. }) => return Ok(None),
                Err(err) => return Err(err),
            };
            let any = node.as_any_mut();
            if let Some(subcompose) = any.downcast_mut::<SubcomposeLayoutNode>() {
                let handle = subcompose.handle();
                let props = handle.layout_properties();
                let offset = handle.total_offset();
                (handle, props, offset)
            } else {
                return Ok(None);
            }
        };

        let runtime_handle = {
            let mut state = state_rc.borrow_mut();
            if state.runtime_handle.is_none() {
                state.runtime_handle = applier_host.borrow_typed().runtime_handle();
            }
            state
                .runtime_handle
                .clone()
                .ok_or(NodeError::MissingContext {
                    id: node_id,
                    reason: "runtime handle required for subcomposition",
                })?
        };

        let padding = props.padding();
        let mut inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        if let DimensionConstraint::Points(width) = props.width() {
            let constrained_width = width - padding.horizontal_sum();
            inner_constraints.max_width = inner_constraints.max_width.min(constrained_width);
            inner_constraints.min_width = inner_constraints.min_width.min(constrained_width);
        }
        if let DimensionConstraint::Points(height) = props.height() {
            let constrained_height = height - padding.vertical_sum();
            inner_constraints.max_height = inner_constraints.max_height.min(constrained_height);
            inner_constraints.min_height = inner_constraints.min_height.min(constrained_height);
        }

        let mut slots_guard = SlotsGuard::take(Rc::clone(&state_rc));
        let slots_host = slots_guard.host();
        let applier_host_dyn: Rc<dyn ApplierHost> = applier_host.clone();
        let observer = SnapshotStateObserver::new(|callback| callback());
        let composer = Composer::new(
            Rc::clone(&slots_host),
            applier_host_dyn,
            runtime_handle.clone(),
            observer,
            Some(node_id),
        );
        composer.enter_phase(Phase::Measure);

        let measure_result = node_handle.measure(&composer, node_id, inner_constraints)?;

        slots_guard.restore(slots_host.take());

        let node_ids: Vec<NodeId> = measure_result
            .placements
            .iter()
            .map(|placement| placement.node_id)
            .collect();

        node_handle.set_active_children(node_ids.iter().copied());

        let mut width = measure_result.size.width + padding.horizontal_sum();
        let mut height = measure_result.size.height + padding.vertical_sum();

        width = resolve_dimension(
            width,
            props.width(),
            props.min_width(),
            props.max_width(),
            constraints.min_width,
            constraints.max_width,
        );
        height = resolve_dimension(
            height,
            props.height(),
            props.min_height(),
            props.max_height(),
            constraints.min_height,
            constraints.max_height,
        );

        let mut children = Vec::new();
        for placement in measure_result.placements {
            let child =
                Self::measure_node(Rc::clone(&state_rc), placement.node_id, inner_constraints)?;
            let position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };
            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        Ok(Some(Rc::new(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            children,
        ))))
    }

    fn measure_layout_node(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        snapshot: LayoutNodeSnapshot,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let cache_epoch = {
            let state = state_rc.borrow();
            state.cache_epoch
        };
        let LayoutNodeSnapshot {
            modifier,
            measure_policy,
            children,
            cache,
        } = snapshot;
        cache.activate(cache_epoch);
        if let Some(cached) = cache.get_measurement(constraints) {
            return Ok(cached);
        }

        let props = modifier.layout_properties();
        let padding = props.padding();
        let offset = modifier.total_offset();
        let mut inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        if let DimensionConstraint::Points(width) = props.width() {
            let constrained_width = width - padding.horizontal_sum();
            inner_constraints.max_width = inner_constraints.max_width.min(constrained_width);
            inner_constraints.min_width = inner_constraints.min_width.min(constrained_width);
        }
        if let DimensionConstraint::Points(height) = props.height() {
            let constrained_height = height - padding.vertical_sum();
            inner_constraints.max_height = inner_constraints.max_height.min(constrained_height);
            inner_constraints.min_height = inner_constraints.min_height.min(constrained_height);
        }

        let (runtime_handle, applier_host) = {
            let state = state_rc.borrow();
            (state.runtime_handle.clone(), Rc::clone(&state.applier))
        };

        let measure_handle = LayoutMeasureHandle::new(Rc::clone(&state_rc));
        let error = Rc::new(RefCell::new(None));
        let mut pools = VecPools::acquire(Rc::clone(&state_rc));
        let (measurables, records) = pools.parts();

        for &child_id in children.iter() {
            let measured = Rc::new(RefCell::new(None));
            let position = Rc::new(RefCell::new(None));
            let child_info = {
                let mut applier = applier_host.borrow_typed();
                match applier.with_node::<LayoutNode, _>(child_id, |layout_node| {
                    let props = layout_node.modifier.layout_properties();
                    (layout_node.cache_handles(), props.width(), props.height())
                }) {
                    Ok(value) => Some(value),
                    Err(NodeError::TypeMismatch { .. }) => Some((
                        LayoutNodeCacheHandles::default(),
                        DimensionConstraint::Unspecified,
                        DimensionConstraint::Unspecified,
                    )),
                    Err(NodeError::Missing { .. }) => None,
                    Err(err) => return Err(err),
                }
            };
            let Some((cache_handles, _child_width_constraint, _child_height_constraint)) =
                child_info
            else {
                continue;
            };
            cache_handles.activate(cache_epoch);

            records.push((
                child_id,
                ChildRecord {
                    measured: Rc::clone(&measured),
                    last_position: Rc::clone(&position),
                },
            ));
            measurables.push(Box::new(LayoutChildMeasurable::new(
                Rc::clone(&applier_host),
                child_id,
                measured,
                position,
                Rc::clone(&error),
                runtime_handle.clone(),
                cache_handles,
                cache_epoch,
                Some(measure_handle.clone()),
            )));
        }

        let needs_intrinsic_width = matches!(props.width(), DimensionConstraint::Intrinsic(_));
        if needs_intrinsic_width {
            let intrinsic_width = measure_policy
                .min_intrinsic_width(measurables.as_slice(), inner_constraints.max_height);
            let constrained_width = intrinsic_width.max(inner_constraints.min_width);
            if constrained_width.is_finite() && constrained_width < inner_constraints.max_width {
                inner_constraints.max_width = constrained_width;
            }
        }

        let needs_intrinsic_height = matches!(props.height(), DimensionConstraint::Intrinsic(_));
        if needs_intrinsic_height {
            let intrinsic_height = measure_policy
                .min_intrinsic_height(measurables.as_slice(), inner_constraints.max_width);
            let constrained_height = intrinsic_height.max(inner_constraints.min_height);
            if constrained_height.is_finite() && constrained_height < inner_constraints.max_height {
                inner_constraints.max_height = constrained_height;
            }
        }

        let mut measure_constraints = inner_constraints;
        let mut relaxed_width = None;
        if matches!(props.width(), DimensionConstraint::Unspecified)
            && inner_constraints.min_width < inner_constraints.max_width
            && constraints.min_width < constraints.max_width
            && inner_constraints.max_width.is_finite()
        {
            relaxed_width = Some(inner_constraints.max_width);
            measure_constraints.max_width = f32::INFINITY;
        }

        let mut relaxed_height = None;
        if matches!(props.height(), DimensionConstraint::Unspecified)
            && inner_constraints.min_height < inner_constraints.max_height
            && constraints.min_height < constraints.max_height
            && inner_constraints.max_height.is_finite()
        {
            relaxed_height = Some(inner_constraints.max_height);
            measure_constraints.max_height = f32::INFINITY;
        }

        let mut policy_result = measure_policy.measure(measurables.as_slice(), measure_constraints);

        if relaxed_width.is_some() || relaxed_height.is_some() {
            let width_exceeds = relaxed_width
                .map(|limit| policy_result.size.width > limit && limit.is_finite())
                .unwrap_or(false);
            let height_exceeds = relaxed_height
                .map(|limit| policy_result.size.height > limit && limit.is_finite())
                .unwrap_or(false);

            if width_exceeds || height_exceeds {
                let mut tightened_constraints = measure_constraints;
                if let Some(limit) = relaxed_width {
                    tightened_constraints.max_width = limit;
                }
                if let Some(limit) = relaxed_height {
                    tightened_constraints.max_height = limit;
                }
                policy_result =
                    measure_policy.measure(measurables.as_slice(), tightened_constraints);
            }
        }

        if let Some(err) = error.borrow_mut().take() {
            return Err(err);
        }

        let mut width = policy_result.size.width + padding.horizontal_sum();
        let mut height = policy_result.size.height + padding.vertical_sum();

        width = resolve_dimension(
            width,
            props.width(),
            props.min_width(),
            props.max_width(),
            constraints.min_width,
            constraints.max_width,
        );
        height = resolve_dimension(
            height,
            props.height(),
            props.min_height(),
            props.max_height(),
            constraints.min_height,
            constraints.max_height,
        );

        let mut measured_children = Vec::new();
        for &child_id in children.iter() {
            if let Some((_, record)) = records.iter().find(|(id, _)| *id == child_id) {
                if let Some(measured) = record.measured.borrow_mut().take() {
                    let base_position = policy_result
                        .placements
                        .iter()
                        .find(|placement| placement.node_id == child_id)
                        .map(|placement| Point {
                            x: placement.x,
                            y: placement.y,
                        })
                        .or_else(|| record.last_position.borrow().as_ref().copied())
                        .unwrap_or(Point { x: 0.0, y: 0.0 });
                    let position = Point {
                        x: padding.left + base_position.x,
                        y: padding.top + base_position.y,
                    };
                    measured_children.push(MeasuredChild {
                        node: measured,
                        offset: position,
                    });
                }
            }
        }

        let measured = Rc::new(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            measured_children,
        ));

        cache.store_measurement(constraints, Rc::clone(&measured));

        Ok(measured)
    }

    fn measure_button(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        node: ButtonNode,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        use crate::layout::policies::FlexMeasurePolicy;
        let mut layout = LayoutNode::new(
            node.modifier.clone(),
            Rc::new(FlexMeasurePolicy::column(
                LinearArrangement::Start,
                HorizontalAlignment::Start,
            )),
        );
        layout.children = node.children.clone();
        let snapshot = LayoutNodeSnapshot::from_layout_node(&layout);
        Self::measure_layout_node(state_rc, node_id, snapshot, constraints)
    }
}

struct LayoutNodeSnapshot {
    modifier: Modifier,
    measure_policy: Rc<dyn MeasurePolicy>,
    children: Vec<NodeId>,
    cache: LayoutNodeCacheHandles,
}

impl LayoutNodeSnapshot {
    fn from_layout_node(node: &LayoutNode) -> Self {
        Self {
            modifier: node.modifier.clone(),
            measure_policy: Rc::clone(&node.measure_policy),
            children: node.children.iter().copied().collect(),
            cache: node.cache_handles(),
        }
    }
}

struct VecPools {
    state: Rc<RefCell<LayoutBuilderState>>,
    measurables: Option<Vec<Box<dyn Measurable>>>,
    records: Option<Vec<(NodeId, ChildRecord)>>,
}

impl VecPools {
    fn acquire(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        let measurables = {
            let mut state_mut = state.borrow_mut();
            std::mem::take(&mut state_mut.tmp_measurables)
        };
        let records = {
            let mut state_mut = state.borrow_mut();
            std::mem::take(&mut state_mut.tmp_records)
        };
        Self {
            state,
            measurables: Some(measurables),
            records: Some(records),
        }
    }

    fn parts(
        &mut self,
    ) -> (
        &mut Vec<Box<dyn Measurable>>,
        &mut Vec<(NodeId, ChildRecord)>,
    ) {
        let measurables = self
            .measurables
            .as_mut()
            .expect("measurables already returned");
        let records = self.records.as_mut().expect("records already returned");
        (measurables, records)
    }
}

impl Drop for VecPools {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        if let Some(mut measurables) = self.measurables.take() {
            measurables.clear();
            state.tmp_measurables = measurables;
        }
        if let Some(mut records) = self.records.take() {
            records.clear();
            state.tmp_records = records;
        }
    }
}

struct SlotsGuard {
    state: Rc<RefCell<LayoutBuilderState>>,
    slots: Option<SlotBackend>,
}

impl SlotsGuard {
    fn take(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        let slots = {
            let mut state_mut = state.borrow_mut();
            std::mem::take(&mut state_mut.slots)
        };
        Self {
            state,
            slots: Some(slots),
        }
    }

    fn host(&mut self) -> Rc<SlotsHost> {
        let slots = self.slots.take().unwrap_or_else(SlotBackend::default);
        Rc::new(SlotsHost::new(slots))
    }

    fn restore(&mut self, slots: SlotBackend) {
        debug_assert!(self.slots.is_none());
        self.slots = Some(slots);
    }
}

impl Drop for SlotsGuard {
    fn drop(&mut self) {
        if let Some(slots) = self.slots.take() {
            let mut state = self.state.borrow_mut();
            state.slots = slots;
        }
    }
}

#[derive(Clone)]
struct LayoutMeasureHandle {
    state: Rc<RefCell<LayoutBuilderState>>,
}

impl LayoutMeasureHandle {
    fn new(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        Self { state }
    }

    fn measure(
        &self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        LayoutBuilderState::measure_node(Rc::clone(&self.state), node_id, constraints)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MeasuredNode {
    node_id: NodeId,
    size: Size,
    offset: Point,
    children: Vec<MeasuredChild>,
}

impl MeasuredNode {
    fn new(node_id: NodeId, size: Size, offset: Point, children: Vec<MeasuredChild>) -> Self {
        Self {
            node_id,
            size,
            offset,
            children,
        }
    }
}

#[derive(Debug, Clone)]
struct MeasuredChild {
    node: Rc<MeasuredNode>,
    offset: Point,
}

struct ChildRecord {
    measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
    last_position: Rc<RefCell<Option<Point>>>,
}

struct LayoutChildMeasurable {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    node_id: NodeId,
    measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
    last_position: Rc<RefCell<Option<Point>>>,
    error: Rc<RefCell<Option<NodeError>>>,
    runtime_handle: Option<RuntimeHandle>,
    cache: LayoutNodeCacheHandles,
    cache_epoch: u64,
    measure_handle: Option<LayoutMeasureHandle>,
}

impl LayoutChildMeasurable {
    fn new(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        node_id: NodeId,
        measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
        last_position: Rc<RefCell<Option<Point>>>,
        error: Rc<RefCell<Option<NodeError>>>,
        runtime_handle: Option<RuntimeHandle>,
        cache: LayoutNodeCacheHandles,
        cache_epoch: u64,
        measure_handle: Option<LayoutMeasureHandle>,
    ) -> Self {
        cache.activate(cache_epoch);
        Self {
            applier,
            node_id,
            measured,
            last_position,
            error,
            runtime_handle,
            cache,
            cache_epoch,
            measure_handle,
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn perform_measure(&self, constraints: Constraints) -> Result<Rc<MeasuredNode>, NodeError> {
        if let Some(handle) = &self.measure_handle {
            handle.measure(self.node_id, constraints)
        } else {
            measure_node_with_host(
                Rc::clone(&self.applier),
                self.runtime_handle.clone(),
                self.node_id,
                constraints,
                self.cache_epoch,
            )
        }
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        self.cache.activate(self.cache_epoch);
        if let Some(cached) = self.cache.get_measurement(constraints) {
            return Some(cached);
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                self.cache
                    .store_measurement(constraints, Rc::clone(&measured));
                Some(measured)
            }
            Err(err) => {
                self.record_error(err);
                None
            }
        }
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        self.cache.activate(self.cache_epoch);
        if let Some(cached) = self.cache.get_measurement(constraints) {
            *self.measured.borrow_mut() = Some(Rc::clone(&cached));
        } else {
            match self.perform_measure(constraints) {
                Ok(measured) => {
                    self.cache
                        .store_measurement(constraints, Rc::clone(&measured));
                    *self.measured.borrow_mut() = Some(measured);
                }
                Err(err) => {
                    self.record_error(err);
                    self.measured.borrow_mut().take();
                }
            }
        }
        Box::new(LayoutChildPlaceable::new(
            self.node_id,
            Rc::clone(&self.measured),
            Rc::clone(&self.last_position),
        ))
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MinWidth(height);
        self.cache.activate(self.cache_epoch);
        if let Some(value) = self.cache.get_intrinsic(&kind) {
            return value;
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.width;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MaxWidth(height);
        self.cache.activate(self.cache_epoch);
        if let Some(value) = self.cache.get_intrinsic(&kind) {
            return value;
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: height,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.width;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MinHeight(width);
        self.cache.activate(self.cache_epoch);
        if let Some(value) = self.cache.get_intrinsic(&kind) {
            return value;
        }
        let constraints = Constraints {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.height;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MaxHeight(width);
        self.cache.activate(self.cache_epoch);
        if let Some(value) = self.cache.get_intrinsic(&kind) {
            return value;
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.height;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn flex_parent_data(&self) -> Option<compose_ui_layout::FlexParentData> {
        let mut applier = self.applier.borrow_typed();
        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let props = layout_node.modifier.layout_properties();
                props.weight().map(|weight_data| {
                    compose_ui_layout::FlexParentData::new(weight_data.weight, weight_data.fill)
                })
            })
            .ok()
            .flatten()
    }
}

struct LayoutChildPlaceable {
    node_id: NodeId,
    measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
    last_position: Rc<RefCell<Option<Point>>>,
}

impl LayoutChildPlaceable {
    fn new(
        node_id: NodeId,
        measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
        last_position: Rc<RefCell<Option<Point>>>,
    ) -> Self {
        Self {
            node_id,
            measured,
            last_position,
        }
    }
}

impl Placeable for LayoutChildPlaceable {
    fn place(&self, x: f32, y: f32) {
        *self.last_position.borrow_mut() = Some(Point { x, y });
    }

    fn width(&self) -> f32 {
        self.measured
            .borrow()
            .as_ref()
            .map(|node| node.size.width)
            .unwrap_or(0.0)
    }

    fn height(&self) -> f32 {
        self.measured
            .borrow()
            .as_ref()
            .map(|node| node.size.height)
            .unwrap_or(0.0)
    }

    fn node_id(&self) -> NodeId {
        self.node_id
    }
}

fn measure_node_with_host(
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    node_id: NodeId,
    constraints: Constraints,
    epoch: u64,
) -> Result<Rc<MeasuredNode>, NodeError> {
    let runtime_handle = match runtime_handle {
        Some(handle) => Some(handle),
        None => applier.borrow_typed().runtime_handle(),
    };
    let mut builder = LayoutBuilder::new(applier);
    builder.set_runtime_handle(runtime_handle);
    builder.set_cache_epoch(epoch);
    builder.measure_node(node_id, constraints)
}

fn measure_text(node_id: NodeId, node: &TextNode, constraints: Constraints) -> Rc<MeasuredNode> {
    let base = measure_text_content(&node.text);
    measure_leaf(node_id, node.modifier.clone(), base, constraints)
}

fn measure_spacer(
    node_id: NodeId,
    node: &SpacerNode,
    constraints: Constraints,
) -> Rc<MeasuredNode> {
    measure_leaf(node_id, Modifier::empty(), node.size, constraints)
}

fn measure_leaf(
    node_id: NodeId,
    modifier: Modifier,
    base_size: Size,
    constraints: Constraints,
) -> Rc<MeasuredNode> {
    let props = modifier.layout_properties();
    let padding = props.padding();
    let offset = modifier.total_offset();

    let mut width = base_size.width + padding.horizontal_sum();
    let mut height = base_size.height + padding.vertical_sum();

    width = resolve_dimension(
        width,
        props.width(),
        props.min_width(),
        props.max_width(),
        constraints.min_width,
        constraints.max_width,
    );
    height = resolve_dimension(
        height,
        props.height(),
        props.min_height(),
        props.max_height(),
        constraints.min_height,
        constraints.max_height,
    );

    Rc::new(MeasuredNode::new(
        node_id,
        Size { width, height },
        offset,
        Vec::new(),
    ))
}

#[derive(Clone)]
struct RuntimeNodeMetadata {
    modifier: Modifier,
    role: SemanticsRole,
    actions: Vec<SemanticsAction>,
    button_handler: Option<Rc<RefCell<dyn FnMut()>>>,
}

impl Default for RuntimeNodeMetadata {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            role: SemanticsRole::Unknown,
            actions: Vec::new(),
            button_handler: None,
        }
    }
}

fn collect_runtime_metadata(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<HashMap<NodeId, RuntimeNodeMetadata>, NodeError> {
    let mut map = HashMap::default();
    collect_runtime_metadata_inner(applier, node, &mut map)?;
    Ok(map)
}

fn collect_runtime_metadata_inner(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
    map: &mut HashMap<NodeId, RuntimeNodeMetadata>,
) -> Result<(), NodeError> {
    if let Entry::Vacant(entry) = map.entry(node.node_id) {
        let meta = runtime_metadata_for(applier, node.node_id)?;
        entry.insert(meta);
    }
    for child in &node.children {
        collect_runtime_metadata_inner(applier, &child.node, map)?;
    }
    Ok(())
}

fn runtime_metadata_for(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<RuntimeNodeMetadata, NodeError> {
    if let Some(layout) = try_clone::<LayoutNode>(applier, node_id)? {
        return Ok(RuntimeNodeMetadata {
            modifier: layout.modifier.clone(),
            role: SemanticsRole::Layout,
            actions: Vec::new(),
            button_handler: None,
        });
    }
    if let Some(button) = try_clone::<ButtonNode>(applier, node_id)? {
        return Ok(RuntimeNodeMetadata {
            modifier: button.modifier.clone(),
            role: SemanticsRole::Button,
            actions: vec![SemanticsAction::Click {
                handler: SemanticsCallback::new(node_id),
            }],
            button_handler: Some(button.on_click.clone()),
        });
    }
    if let Some(text) = try_clone::<TextNode>(applier, node_id)? {
        return Ok(RuntimeNodeMetadata {
            modifier: text.modifier.clone(),
            role: SemanticsRole::Text {
                value: text.text.clone(),
            },
            actions: Vec::new(),
            button_handler: None,
        });
    }
    if try_clone::<SpacerNode>(applier, node_id)?.is_some() {
        return Ok(RuntimeNodeMetadata {
            modifier: Modifier::empty(),
            role: SemanticsRole::Spacer,
            actions: Vec::new(),
            button_handler: None,
        });
    }
    if let Ok(modifier) =
        applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| node.modifier())
    {
        return Ok(RuntimeNodeMetadata {
            modifier,
            role: SemanticsRole::Subcompose,
            actions: Vec::new(),
            button_handler: None,
        });
    }
    Ok(RuntimeNodeMetadata::default())
}

fn build_semantics_node(
    node: &MeasuredNode,
    metadata: &HashMap<NodeId, RuntimeNodeMetadata>,
) -> SemanticsNode {
    let info = metadata.get(&node.node_id).cloned().unwrap_or_default();
    let children = node
        .children
        .iter()
        .map(|child| build_semantics_node(&child.node, metadata))
        .collect();
    SemanticsNode::new(node.node_id, info.role, info.actions, children)
}

fn build_layout_tree_from_metadata(
    node: &MeasuredNode,
    metadata: &HashMap<NodeId, RuntimeNodeMetadata>,
) -> LayoutTree {
    fn place(
        node: &MeasuredNode,
        origin: Point,
        metadata: &HashMap<NodeId, RuntimeNodeMetadata>,
    ) -> LayoutBox {
        let top_left = Point {
            x: origin.x + node.offset.x,
            y: origin.y + node.offset.y,
        };
        let rect = GeometryRect {
            x: top_left.x,
            y: top_left.y,
            width: node.size.width,
            height: node.size.height,
        };
        let info = metadata.get(&node.node_id).cloned().unwrap_or_default();
        let kind = layout_kind_from_metadata(node.node_id, &info);
        let data = LayoutNodeData::new(info.modifier.clone(), kind);
        let children = node
            .children
            .iter()
            .map(|child| {
                let child_origin = Point {
                    x: top_left.x + child.offset.x,
                    y: top_left.y + child.offset.y,
                };
                place(&child.node, child_origin, metadata)
            })
            .collect();
        LayoutBox::new(node.node_id, rect, data, children)
    }

    LayoutTree::new(place(node, Point { x: 0.0, y: 0.0 }, metadata))
}

fn layout_kind_from_metadata(_node_id: NodeId, info: &RuntimeNodeMetadata) -> LayoutNodeKind {
    match &info.role {
        SemanticsRole::Layout => LayoutNodeKind::Layout,
        SemanticsRole::Subcompose => LayoutNodeKind::Subcompose,
        SemanticsRole::Text { value } => LayoutNodeKind::Text {
            value: value.clone(),
        },
        SemanticsRole::Spacer => LayoutNodeKind::Spacer,
        SemanticsRole::Button => {
            let handler = info
                .button_handler
                .as_ref()
                .cloned()
                .unwrap_or_else(|| Rc::new(RefCell::new(|| {})));
            LayoutNodeKind::Button { on_click: handler }
        }
        SemanticsRole::Unknown => LayoutNodeKind::Unknown,
    }
}

fn measure_text_content(text: &str) -> Size {
    let metrics = crate::text::measure_text(text);
    Size {
        width: metrics.width,
        height: metrics.height,
    }
}

fn subtract_padding(constraints: Constraints, padding: EdgeInsets) -> Constraints {
    let horizontal = padding.horizontal_sum();
    let vertical = padding.vertical_sum();
    let min_width = (constraints.min_width - horizontal).max(0.0);
    let mut max_width = constraints.max_width;
    if max_width.is_finite() {
        max_width = (max_width - horizontal).max(0.0);
    }
    let min_height = (constraints.min_height - vertical).max(0.0);
    let mut max_height = constraints.max_height;
    if max_height.is_finite() {
        max_height = (max_height - vertical).max(0.0);
    }
    normalize_constraints(Constraints {
        min_width,
        max_width,
        min_height,
        max_height,
    })
}

#[cfg(test)]
pub(crate) fn align_horizontal(alignment: HorizontalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        HorizontalAlignment::Start => 0.0,
        HorizontalAlignment::CenterHorizontally => ((available - child) / 2.0).max(0.0),
        HorizontalAlignment::End => (available - child).max(0.0),
    }
}

#[cfg(test)]
pub(crate) fn align_vertical(alignment: VerticalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        VerticalAlignment::Top => 0.0,
        VerticalAlignment::CenterVertically => ((available - child) / 2.0).max(0.0),
        VerticalAlignment::Bottom => (available - child).max(0.0),
    }
}

fn resolve_dimension(
    base: f32,
    explicit: DimensionConstraint,
    min_override: Option<f32>,
    max_override: Option<f32>,
    min_limit: f32,
    max_limit: f32,
) -> f32 {
    let mut min_bound = min_limit;
    if let Some(min_value) = min_override {
        min_bound = min_bound.max(min_value);
    }

    let mut max_bound = if max_limit.is_finite() {
        max_limit
    } else {
        max_override.unwrap_or(max_limit)
    };
    if let Some(max_value) = max_override {
        if max_bound.is_finite() {
            max_bound = max_bound.min(max_value);
        } else {
            max_bound = max_value;
        }
    }
    if max_bound < min_bound {
        max_bound = min_bound;
    }

    let mut size = match explicit {
        DimensionConstraint::Points(points) => points,
        DimensionConstraint::Fraction(fraction) => {
            if max_limit.is_finite() {
                max_limit * fraction.clamp(0.0, 1.0)
            } else {
                base
            }
        }
        DimensionConstraint::Unspecified => base,
        // Intrinsic sizing is resolved at a higher level where we have access to children.
        // At this point we just use the base size as a fallback.
        DimensionConstraint::Intrinsic(_) => base,
    };

    size = clamp_dimension(size, min_bound, max_bound);
    size = clamp_dimension(size, min_limit, max_limit);
    size.max(0.0)
}

fn clamp_dimension(value: f32, min: f32, max: f32) -> f32 {
    let mut result = value.max(min);
    if max.is_finite() {
        result = result.min(max);
    }
    result
}

fn normalize_constraints(mut constraints: Constraints) -> Constraints {
    if constraints.max_width < constraints.min_width {
        constraints.max_width = constraints.min_width;
    }
    if constraints.max_height < constraints.min_height {
        constraints.max_height = constraints.min_height;
    }
    constraints
}

fn try_clone<T: Node + Clone + 'static>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<Option<T>, NodeError> {
    match applier.with_node(node_id, |node: &mut T| node.clone()) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
#[path = "tests/layout_tests.rs"]
mod tests;
