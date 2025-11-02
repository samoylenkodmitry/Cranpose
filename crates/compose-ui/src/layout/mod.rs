pub mod core;
pub mod policies;

use compose_core::collections::map::HashMap;
use compose_core::collections::map::Entry;
use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use compose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost, MemoryApplier, Node, NodeError, NodeId,
    Phase, RuntimeHandle, SlotTable, SlotsHost, SnapshotStateObserver,
};

#[cfg(test)]
use self::core::VerticalAlignment;
use self::core::{HorizontalAlignment, LinearArrangement, Measurable, Placeable};
use crate::modifier::{
    DimensionConstraint, EdgeInsets, Modifier, Point, Rect as GeometryRect, Size,
};
use crate::subcompose_layout::SubcomposeLayoutNode;
use crate::widgets::nodes::{ButtonNode, LayoutNode, LayoutNodeCacheHandles, SpacerNode, TextNode};
use compose_ui_layout::Constraints;

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
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    slots: SlotTable,
    cache_epoch: u64,
}

impl LayoutBuilder {
    fn new(applier: Rc<ConcreteApplierHost<MemoryApplier>>) -> Self {
        let epoch = NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed);
        let runtime_handle = applier.borrow_typed().runtime_handle();
        Self {
            applier,
            runtime_handle,
            slots: SlotTable::new(),
            cache_epoch: epoch,
        }
    }

    fn with_applier<R>(&self, f: impl FnOnce(&mut MemoryApplier) -> R) -> R {
        let mut applier = self.applier.borrow_typed();
        f(&mut *applier)
    }

    fn measure_node(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let constraints = normalize_constraints(constraints);
        if let Some(subcompose) = self.try_measure_subcompose(node_id, constraints)? {
            return Ok(subcompose);
        }
        if let Some(layout) =
            self.with_applier(|applier| try_clone::<LayoutNode>(applier, node_id))?
        {
            return self.measure_layout_node(node_id, layout, constraints);
        }
        if let Some(text) = self.with_applier(|applier| try_clone::<TextNode>(applier, node_id))? {
            return Ok(measure_text(node_id, &text, constraints));
        }
        if let Some(spacer) =
            self.with_applier(|applier| try_clone::<SpacerNode>(applier, node_id))?
        {
            return Ok(measure_spacer(node_id, &spacer, constraints));
        }
        if let Some(button) =
            self.with_applier(|applier| try_clone::<ButtonNode>(applier, node_id))?
        {
            return self.measure_button(node_id, button, constraints);
        }
        Ok(Rc::new(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
            Vec::new(),
        )))
    }

    fn try_measure_subcompose(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        // Extract modifier data while we have safe mutable access
        let (node_handle, props, offset) = {
            let mut applier = self.applier.borrow_typed();
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

        let runtime_handle = self
            .runtime_handle
            .clone()
            .or_else(|| self.with_applier(|applier| applier.runtime_handle()))
            .ok_or(NodeError::MissingContext {
                id: node_id,
                reason: "runtime handle required for subcomposition",
            })?;
        self.runtime_handle = Some(runtime_handle.clone());

        // Props and offset were extracted during the safe borrow above
        let padding = props.padding();
        let mut inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        // Apply explicit width/height constraints to inner_constraints BEFORE measuring children.
        // This ensures that children respect the parent's explicit size constraints.
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
        self.slots.reset();
        let slots_host = Rc::new(SlotsHost::new(std::mem::take(&mut self.slots)));
        let applier_host: Rc<dyn ApplierHost> = self.applier.clone();
        let observer = SnapshotStateObserver::new(|callback| callback());
        let composer = Composer::new(
            Rc::clone(&slots_host),
            applier_host,
            runtime_handle.clone(),
            observer,
            Some(node_id),
        );
        composer.enter_phase(Phase::Measure);

        // The handle keeps interior mutability within the node while releasing the
        // applier borrow, allowing the composer to re-enter safely during measure.
        let measure_result = node_handle.measure(&composer, node_id, inner_constraints)?;

        self.slots = slots_host.take();

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
            let child = self.measure_node(placement.node_id, inner_constraints)?;
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
        &mut self,
        node_id: NodeId,
        node: LayoutNode,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let node = node;
        let cache = node.cache_handles();
        cache.activate(self.cache_epoch);
        if let Some(cached) = cache.get_measurement(constraints) {
            return Ok(cached);
        }
        let modifier = node.modifier.clone();
        let props = modifier.layout_properties();
        let padding = props.padding();
        let offset = modifier.total_offset();
        let mut inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        // Apply explicit width/height constraints to inner_constraints BEFORE measuring children.
        // This ensures that when a parent has an explicit size (e.g., Modifier::width(360.0)),
        // its children receive constraints that respect that size.
        // Without this, a child with fill_max_width() would incorrectly use the grandparent's constraints.
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

        let error = Rc::new(RefCell::new(None));
        let mut records: HashMap<NodeId, ChildRecord> = HashMap::default();
        let mut measurables: Vec<Box<dyn Measurable>> = Vec::new();

        for &child_id in node.children.iter() {
            let measured = Rc::new(RefCell::new(None));
            let position = Rc::new(RefCell::new(None));
            let cache_handles = {
                let mut applier = self.applier.borrow_typed();
                match applier
                    .with_node::<LayoutNode, _>(child_id, |layout_node| layout_node.cache_handles())
                {
                    Ok(handles) => Some(handles),
                    Err(NodeError::TypeMismatch { .. }) => Some(LayoutNodeCacheHandles::default()),
                    Err(NodeError::Missing { .. }) => None,
                    Err(err) => return Err(err),
                }
            };
            let Some(cache_handles) = cache_handles else {
                continue;
            };
            cache_handles.activate(self.cache_epoch);
            records.insert(
                child_id,
                ChildRecord {
                    measured: Rc::clone(&measured),
                    last_position: Rc::clone(&position),
                },
            );
            measurables.push(Box::new(LayoutChildMeasurable::new(
                Rc::clone(&self.applier),
                child_id,
                measured,
                position,
                Rc::clone(&error),
                self.runtime_handle.clone(),
                cache_handles,
                self.cache_epoch,
            )));
        }

        // For wrap-content behavior: when width/height is Unspecified, use intrinsic measurements
        // to determine the parent's natural size BEFORE measuring children.
        // This prevents children with fill_max_width() from taking the entire available space
        // when the parent should wrap to content.
        if props.width() == DimensionConstraint::Unspecified {
            // Query the policy's intrinsic width based on current constraints
            let intrinsic_width = node
                .measure_policy
                .min_intrinsic_width(&measurables, inner_constraints.max_height);
            // Constrain max_width to the intrinsic size, but respect min_width from constraints
            let constrained_width = intrinsic_width.max(inner_constraints.min_width);
            if constrained_width.is_finite() && constrained_width < inner_constraints.max_width {
                inner_constraints.max_width = constrained_width;
            }
        }
        if props.height() == DimensionConstraint::Unspecified {
            // Query the policy's intrinsic height based on current constraints
            let intrinsic_height = node
                .measure_policy
                .min_intrinsic_height(&measurables, inner_constraints.max_width);
            // Constrain max_height to the intrinsic size, but respect min_height from constraints
            let constrained_height = intrinsic_height.max(inner_constraints.min_height);
            if constrained_height.is_finite() && constrained_height < inner_constraints.max_height {
                inner_constraints.max_height = constrained_height;
            }
        }

        let policy_result = node.measure_policy.measure(&measurables, inner_constraints);

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

        let mut placement_map: HashMap<NodeId, Point> = policy_result
            .placements
            .into_iter()
            .map(|placement| {
                (
                    placement.node_id,
                    Point {
                        x: placement.x,
                        y: placement.y,
                    },
                )
            })
            .collect();

        let mut children = Vec::new();
        for &child_id in node.children.iter() {
            if let Some(record) = records.remove(&child_id) {
                if let Some(measured) = record.measured.borrow_mut().take() {
                    let base_position = placement_map
                        .remove(&child_id)
                        .or_else(|| record.last_position.borrow().as_ref().copied())
                        .unwrap_or(Point { x: 0.0, y: 0.0 });
                    let position = Point {
                        x: padding.left + base_position.x,
                        y: padding.top + base_position.y,
                    };
                    children.push(MeasuredChild {
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
            children,
        ));

        cache.store_measurement(constraints, Rc::clone(&measured));

        Ok(measured)
    }

    fn measure_button(
        &mut self,
        node_id: NodeId,
        node: ButtonNode,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        // Button is just a layout with column-like behavior
        use crate::layout::policies::FlexMeasurePolicy;
        let mut layout = LayoutNode::new(
            node.modifier.clone(),
            Rc::new(FlexMeasurePolicy::column(
                LinearArrangement::Start,
                HorizontalAlignment::Start,
            )),
        );
        layout.children = node.children.clone();
        let layout_measurement = self.measure_layout_node(node_id, layout, constraints)?;
        let measured = Rc::new(MeasuredNode::new(
            node_id,
            layout_measurement.size,
            layout_measurement.offset,
            layout_measurement.children.clone(),
        ));
        Ok(measured)
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
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        self.cache.activate(self.cache_epoch);
        if let Some(cached) = self.cache.get_measurement(constraints) {
            return Some(cached);
        }

        match measure_node_with_host(
            Rc::clone(&self.applier),
            self.runtime_handle.clone(),
            self.node_id,
            constraints,
            self.cache_epoch,
        ) {
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
            match measure_node_with_host(
                Rc::clone(&self.applier),
                self.runtime_handle.clone(),
                self.node_id,
                constraints,
                self.cache_epoch,
            ) {
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
        match self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }) {
            Some(node) => node.size.width,
            None => 0.0,
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        match self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: height,
        }) {
            Some(node) => node.size.width,
            None => 0.0,
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        match self.intrinsic_measure(Constraints {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }) {
            Some(node) => node.size.height,
            None => 0.0,
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        match self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }) {
            Some(node) => node.size.height,
            None => 0.0,
        }
    }

    fn flex_parent_data(&self) -> Option<compose_ui_layout::FlexParentData> {
        // Access the node's modifier to extract weight information
        // We use with_node which is safe, but we need to convert the raw pointer
        // to a mutable reference temporarily for the API
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
    builder.runtime_handle = runtime_handle;
    builder.cache_epoch = epoch;
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
