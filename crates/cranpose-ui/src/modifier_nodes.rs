//! Concrete implementations of modifier nodes for common modifiers.
//!
//! This module provides node-backed implementations of layout and draw modifiers
//! following Jetpack Compose's Modifier.Node architecture. All modifiers are now
//! node-based, achieving complete parity with Kotlin's modifier system.
//!
//! # Overview
//!
//! The Modifier.Node system provides excellent performance through:
//! - **Node reuse** — Node instances are reused across recompositions (zero allocations when stable)
//! - **Targeted invalidation** — Only affected phases (layout/draw/pointer/focus) are invalidated
//! - **Lifecycle hooks** — `on_attach`, `on_detach`, `update` for efficient state management
//! - **Capability-driven dispatch** — Nodes declare capabilities via `NodeCapabilities` bits
//!
//! # Example Usage
//!
//! ```text
//! use cranpose_foundation::{modifier_element, ModifierNodeChain, BasicModifierNodeContext};
//! use cranpose_ui::{PaddingElement, EdgeInsets};
//!
//! let mut chain = ModifierNodeChain::new();
//! let mut context = BasicModifierNodeContext::new();
//!
//! // Create a padding modifier element
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(16.0)))];
//!
//! // Reconcile the chain (attaches new nodes, reuses existing)
//! chain.update_from_slice(&elements, &mut context);
//!
//! // Update with different padding - reuses the same node instance
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(24.0)))];
//! chain.update_from_slice(&elements, &mut context);
//! // Zero allocations on this update!
//! ```
//!
//! # Available Nodes
//!
//! ## Layout Modifiers
//! - [`PaddingNode`] / [`PaddingElement`]: Adds padding around content
//! - [`SizeNode`] / [`SizeElement`]: Enforces specific dimensions
//! - [`FillNode`] / [`FillElement`]: Fills available space with optional fractions
//! - [`OffsetNode`] / [`OffsetElement`]: Translates content by offset
//! - [`WeightNode`] / [`WeightElement`]: Proportional sizing in flex containers
//! - [`AlignmentNode`] / [`AlignmentElement`]: Alignment within parent
//! - [`IntrinsicSizeNode`] / [`IntrinsicSizeElement`]: Intrinsic measurement
//!
//! ## Draw Modifiers
//! - [`BackgroundNode`] / [`BackgroundElement`]: Draws a background color
//! - [`AlphaNode`] / [`AlphaElement`]: Applies alpha transparency
//! - [`CornerShapeNode`] / [`CornerShapeElement`]: Rounded corner clipping
//! - [`GraphicsLayerNode`] / [`GraphicsLayerElement`]: Advanced transformations
//!
//! ## Input Modifiers
//! - [`ClickableNode`] / [`ClickableElement`]: Handles click/tap interactions (pointer input)
//!
//! # Architecture Notes
//!
//! This is the **only** modifier implementation — there is no alternate "value-based" system.
//! All modifier factories in `Modifier` return `ModifierNodeElement` instances that create
//! these nodes. The system achieves complete 1:1 parity with Jetpack Compose's modifier
//! architecture.

use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, LayoutModifierNode, Measurable,
    MeasurementProxy, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities,
    NodeState, PointerEvent, PointerEventKind, PointerInputNode, Size,
};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};

use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::draw::DrawCommand;
use crate::modifier::{
    BlendMode, Color, ColorFilter, CompositingStrategy, EdgeInsets, GraphicsLayer, LayoutWeight,
    Point, RoundedCornerShape,
};

fn hash_f32_value<H: Hasher>(state: &mut H, value: f32) {
    state.write_u32(value.to_bits());
}

fn hash_option_f32<H: Hasher>(state: &mut H, value: Option<f32>) {
    match value {
        Some(v) => {
            state.write_u8(1);
            hash_f32_value(state, v);
        }
        None => state.write_u8(0),
    }
}

fn hash_graphics_layer<H: Hasher>(state: &mut H, layer: &GraphicsLayer) {
    hash_f32_value(state, layer.alpha);
    hash_f32_value(state, layer.scale);
    hash_f32_value(state, layer.scale_x);
    hash_f32_value(state, layer.scale_y);
    hash_f32_value(state, layer.rotation_x);
    hash_f32_value(state, layer.rotation_y);
    hash_f32_value(state, layer.rotation_z);
    hash_f32_value(state, layer.camera_distance);
    hash_f32_value(state, layer.transform_origin.pivot_fraction_x);
    hash_f32_value(state, layer.transform_origin.pivot_fraction_y);
    hash_f32_value(state, layer.translation_x);
    hash_f32_value(state, layer.translation_y);
    hash_f32_value(state, layer.shadow_elevation);
    hash_f32_value(state, layer.ambient_shadow_color.r());
    hash_f32_value(state, layer.ambient_shadow_color.g());
    hash_f32_value(state, layer.ambient_shadow_color.b());
    hash_f32_value(state, layer.ambient_shadow_color.a());
    hash_f32_value(state, layer.spot_shadow_color.r());
    hash_f32_value(state, layer.spot_shadow_color.g());
    hash_f32_value(state, layer.spot_shadow_color.b());
    hash_f32_value(state, layer.spot_shadow_color.a());
    match layer.shape {
        crate::modifier::LayerShape::Rectangle => {
            state.write_u8(0);
        }
        crate::modifier::LayerShape::Rounded(shape) => {
            state.write_u8(1);
            let radii = shape.radii();
            hash_f32_value(state, radii.top_left);
            hash_f32_value(state, radii.top_right);
            hash_f32_value(state, radii.bottom_right);
            hash_f32_value(state, radii.bottom_left);
        }
    }
    state.write_u8(layer.clip as u8);
    match layer.color_filter {
        Some(ColorFilter::Tint(color)) => {
            state.write_u8(1);
            hash_f32_value(state, color.r());
            hash_f32_value(state, color.g());
            hash_f32_value(state, color.b());
            hash_f32_value(state, color.a());
        }
        Some(ColorFilter::Modulate(color)) => {
            state.write_u8(2);
            hash_f32_value(state, color.r());
            hash_f32_value(state, color.g());
            hash_f32_value(state, color.b());
            hash_f32_value(state, color.a());
        }
        Some(ColorFilter::Matrix(matrix)) => {
            state.write_u8(3);
            for value in matrix {
                hash_f32_value(state, value);
            }
        }
        None => state.write_u8(0),
    }
    state.write_u8(layer.render_effect.is_some() as u8);
    state.write_u8(layer.backdrop_effect.is_some() as u8);
    let compositing_tag = match layer.compositing_strategy {
        CompositingStrategy::Auto => 0,
        CompositingStrategy::Offscreen => 1,
        CompositingStrategy::ModulateAlpha => 2,
    };
    state.write_u8(compositing_tag);
    let blend_tag = match layer.blend_mode {
        BlendMode::Clear => 0,
        BlendMode::Src => 1,
        BlendMode::Dst => 2,
        BlendMode::SrcOver => 3,
        BlendMode::DstOver => 4,
        BlendMode::SrcIn => 5,
        BlendMode::DstIn => 6,
        BlendMode::SrcOut => 7,
        BlendMode::DstOut => 8,
        BlendMode::SrcAtop => 9,
        BlendMode::DstAtop => 10,
        BlendMode::Xor => 11,
        BlendMode::Plus => 12,
        BlendMode::Modulate => 13,
        BlendMode::Screen => 14,
        BlendMode::Overlay => 15,
        BlendMode::Darken => 16,
        BlendMode::Lighten => 17,
        BlendMode::ColorDodge => 18,
        BlendMode::ColorBurn => 19,
        BlendMode::HardLight => 20,
        BlendMode::SoftLight => 21,
        BlendMode::Difference => 22,
        BlendMode::Exclusion => 23,
        BlendMode::Multiply => 24,
        BlendMode::Hue => 25,
        BlendMode::Saturation => 26,
        BlendMode::Color => 27,
        BlendMode::Luminosity => 28,
    };
    state.write_u8(blend_tag);
}

fn hash_horizontal_alignment<H: Hasher>(state: &mut H, alignment: HorizontalAlignment) {
    let tag = match alignment {
        HorizontalAlignment::Start => 0,
        HorizontalAlignment::CenterHorizontally => 1,
        HorizontalAlignment::End => 2,
    };
    state.write_u8(tag);
}

fn hash_vertical_alignment<H: Hasher>(state: &mut H, alignment: VerticalAlignment) {
    let tag = match alignment {
        VerticalAlignment::Top => 0,
        VerticalAlignment::CenterVertically => 1,
        VerticalAlignment::Bottom => 2,
    };
    state.write_u8(tag);
}

fn hash_alignment<H: Hasher>(state: &mut H, alignment: Alignment) {
    hash_horizontal_alignment(state, alignment.horizontal);
    hash_vertical_alignment(state, alignment.vertical);
}

static NEXT_LAZY_GRAPHICS_LAYER_SCOPE_ID: AtomicUsize = AtomicUsize::new(1);

fn next_lazy_graphics_layer_scope_id() -> usize {
    NEXT_LAZY_GRAPHICS_LAYER_SCOPE_ID.fetch_add(1, Ordering::Relaxed)
}

// ============================================================================
// Padding Modifier Node
// ============================================================================

/// Node that adds padding around its content.
#[derive(Debug)]
pub struct PaddingNode {
    padding: EdgeInsets,
    state: NodeState,
}

impl PaddingNode {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            state: NodeState::new(),
        }
    }

    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
}

impl DelegatableNode for PaddingNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for PaddingNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for PaddingNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Convert padding to floating point values
        let horizontal_padding = self.padding.horizontal_sum();
        let vertical_padding = self.padding.vertical_sum();

        // Subtract padding from available space
        let inner_constraints = Constraints {
            min_width: (constraints.min_width - horizontal_padding).max(0.0),
            max_width: (constraints.max_width - horizontal_padding).max(0.0),
            min_height: (constraints.min_height - vertical_padding).max(0.0),
            max_height: (constraints.max_height - vertical_padding).max(0.0),
        };

        // Measure the wrapped content
        let inner_placeable = measurable.measure(inner_constraints);
        let inner_width = inner_placeable.width();
        let inner_height = inner_placeable.height();

        let (width, height) = constraints.constrain(
            inner_width + horizontal_padding,
            inner_height + vertical_padding,
        );

        // Return size with padding added, and placement offset to position child inside padding
        cranpose_ui_layout::LayoutModifierMeasureResult::new(
            Size { width, height },
            self.padding.left, // Place child offset by left padding
            self.padding.top,  // Place child offset by top padding
        )
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.min_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.max_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.min_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.max_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }

    fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
        Some(Box::new(PaddingMeasurementProxy {
            padding: self.padding,
        }))
    }
}

/// Measurement proxy for PaddingNode that snapshots live state.
///
/// Phase 2: Instead of reconstructing nodes via `PaddingNode::new()`, this proxy
/// directly implements measurement logic using the snapshotted padding state.
/// This avoids temporary allocations and matches Jetpack Compose's pattern more closely.
struct PaddingMeasurementProxy {
    padding: EdgeInsets,
}

impl MeasurementProxy for PaddingMeasurementProxy {
    fn measure_proxy(
        &self,
        _context: &mut dyn ModifierNodeContext,
        wrapped: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Directly implement padding measurement logic (no node reconstruction)
        let horizontal_padding = self.padding.horizontal_sum();
        let vertical_padding = self.padding.vertical_sum();

        // Subtract padding from available space
        let inner_constraints = Constraints {
            min_width: (constraints.min_width - horizontal_padding).max(0.0),
            max_width: (constraints.max_width - horizontal_padding).max(0.0),
            min_height: (constraints.min_height - vertical_padding).max(0.0),
            max_height: (constraints.max_height - vertical_padding).max(0.0),
        };

        // Measure the wrapped content
        let inner_placeable = wrapped.measure(inner_constraints);
        let inner_width = inner_placeable.width();
        let inner_height = inner_placeable.height();

        let (width, height) = constraints.constrain(
            inner_width + horizontal_padding,
            inner_height + vertical_padding,
        );

        // Return size with padding added, and placement offset to position child inside padding
        cranpose_ui_layout::LayoutModifierMeasureResult::new(
            Size { width, height },
            self.padding.left, // Place child offset by left padding
            self.padding.top,  // Place child offset by top padding
        )
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = wrapped.min_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = wrapped.max_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = wrapped.min_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = wrapped.max_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }
}

/// Element that creates and updates padding nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl Hash for PaddingElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.padding.left);
        hash_f32_value(state, self.padding.top);
        hash_f32_value(state, self.padding.right);
        hash_f32_value(state, self.padding.bottom);
    }
}

impl ModifierNodeElement for PaddingElement {
    type Node = PaddingNode;

    fn create(&self) -> Self::Node {
        PaddingNode::new(self.padding)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.padding != self.padding {
            node.padding = self.padding;
            // Note: In a full implementation, we would invalidate layout here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Background Modifier Node
// ============================================================================

/// Node that draws a background behind its content.
#[derive(Debug)]
pub struct BackgroundNode {
    color: Color,
    shape: Option<RoundedCornerShape>,
    state: NodeState,
}

impl BackgroundNode {
    pub fn new(color: Color) -> Self {
        Self {
            color,
            shape: None,
            state: NodeState::new(),
        }
    }

    pub fn color(&self) -> Color {
        self.color
    }

    pub fn shape(&self) -> Option<RoundedCornerShape> {
        self.shape
    }
}

impl DelegatableNode for BackgroundNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for BackgroundNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for BackgroundNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // Background rendering is now handled via draw commands collected in modifier slices.
        // This node exists primarily for capability tracking and future draw scope integration.
    }
}

/// Element that creates and updates background nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl Hash for BackgroundElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.color.0);
        hash_f32_value(state, self.color.1);
        hash_f32_value(state, self.color.2);
        hash_f32_value(state, self.color.3);
    }
}

impl ModifierNodeElement for BackgroundElement {
    type Node = BackgroundNode;

    fn create(&self) -> Self::Node {
        BackgroundNode::new(self.color)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.color != self.color {
            node.color = self.color;
            // Note: In a full implementation, we would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Size Modifier Node
// ============================================================================

/// Node that tracks the latest rounded corner shape.
#[derive(Debug)]
pub struct CornerShapeNode {
    shape: RoundedCornerShape,
    state: NodeState,
}

impl CornerShapeNode {
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self {
            shape,
            state: NodeState::new(),
        }
    }

    pub fn shape(&self) -> RoundedCornerShape {
        self.shape
    }
}

impl DelegatableNode for CornerShapeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for CornerShapeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for CornerShapeNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}
}

/// Element that creates and updates corner shape nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerShapeElement {
    shape: RoundedCornerShape,
}

impl CornerShapeElement {
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self { shape }
    }
}

impl Hash for CornerShapeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let radii = self.shape.radii();
        hash_f32_value(state, radii.top_left);
        hash_f32_value(state, radii.top_right);
        hash_f32_value(state, radii.bottom_right);
        hash_f32_value(state, radii.bottom_left);
    }
}

impl ModifierNodeElement for CornerShapeElement {
    type Node = CornerShapeNode;

    fn create(&self) -> Self::Node {
        CornerShapeNode::new(self.shape)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.shape != self.shape {
            node.shape = self.shape;
            // Invalidations are handled lazily for now.
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// GraphicsLayer Modifier Node
// ============================================================================

/// Node that stores graphics layer state for resolved modifiers.
pub struct GraphicsLayerNode {
    layer: GraphicsLayer,
    layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    lazy_scope_id: Option<usize>,
    lazy_observer: Option<cranpose_core::SnapshotStateObserver>,
    state: NodeState,
}

impl GraphicsLayerNode {
    pub fn new(layer: GraphicsLayer) -> Self {
        Self {
            layer,
            layer_resolver: None,
            lazy_scope_id: None,
            lazy_observer: None,
            state: NodeState::new(),
        }
    }

    pub fn new_lazy(layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) -> Self {
        let mut node = Self {
            layer: GraphicsLayer::default(),
            layer_resolver: Some(layer_resolver),
            lazy_scope_id: None,
            lazy_observer: None,
            state: NodeState::new(),
        };
        node.ensure_lazy_observation();
        node.layer = node.layer();
        node
    }

    pub fn layer(&self) -> GraphicsLayer {
        if let Some(resolve) = self.layer_resolver() {
            resolve()
        } else {
            self.layer.clone()
        }
    }

    pub fn layer_snapshot(&self) -> GraphicsLayer {
        self.layer.clone()
    }

    pub fn layer_resolver(&self) -> Option<Rc<dyn Fn() -> GraphicsLayer>> {
        self.layer_resolver.as_ref().map(|resolve| {
            let resolve = resolve.clone();
            match (&self.lazy_observer, self.lazy_scope_id) {
                (Some(observer), Some(scope_id)) => {
                    let observer = observer.clone();
                    Rc::new(move || {
                        observer.observe_reads(
                            scope_id,
                            |_| crate::request_render_invalidation(),
                            || resolve(),
                        )
                    }) as Rc<dyn Fn() -> GraphicsLayer>
                }
                _ => resolve,
            }
        })
    }

    fn set_static(&mut self, layer: GraphicsLayer) {
        self.layer = layer;
        self.layer_resolver = None;
        self.clear_lazy_observation();
    }

    fn set_lazy(&mut self, layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) {
        self.layer_resolver = Some(layer_resolver);
        self.ensure_lazy_observation();
        self.layer = self.layer();
    }

    fn ensure_lazy_observation(&mut self) {
        if self.layer_resolver.is_none() {
            self.clear_lazy_observation();
            return;
        }
        if self.lazy_observer.is_some() {
            return;
        }
        let observer = cranpose_core::SnapshotStateObserver::new(|callback| callback());
        observer.start();
        self.lazy_scope_id = Some(next_lazy_graphics_layer_scope_id());
        self.lazy_observer = Some(observer);
    }

    fn clear_lazy_observation(&mut self) {
        if let Some(observer) = self.lazy_observer.take() {
            observer.stop();
        }
        self.lazy_scope_id = None;
    }
}

impl DelegatableNode for GraphicsLayerNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for GraphicsLayerNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn on_detach(&mut self) {
        self.clear_lazy_observation();
    }
}

impl std::fmt::Debug for GraphicsLayerNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphicsLayerNode")
            .field("layer", &self.layer)
            .field("lazy", &self.layer_resolver.is_some())
            .finish()
    }
}

/// Element that creates and updates graphics layer nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphicsLayerElement {
    layer: GraphicsLayer,
}

impl GraphicsLayerElement {
    pub fn new(layer: GraphicsLayer) -> Self {
        Self { layer }
    }
}

impl Hash for GraphicsLayerElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_graphics_layer(state, &self.layer);
    }
}

impl ModifierNodeElement for GraphicsLayerElement {
    type Node = GraphicsLayerNode;

    fn create(&self) -> Self::Node {
        GraphicsLayerNode::new(self.layer.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.set_static(self.layer.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

/// Element that evaluates a graphics layer lazily during render data collection.
#[derive(Clone)]
pub struct LazyGraphicsLayerElement {
    layer_resolver: Rc<dyn Fn() -> GraphicsLayer>,
}

impl LazyGraphicsLayerElement {
    pub fn new(layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) -> Self {
        Self { layer_resolver }
    }
}

impl std::fmt::Debug for LazyGraphicsLayerElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyGraphicsLayerElement")
            .field("resolver", &"<closure>")
            .finish()
    }
}

impl PartialEq for LazyGraphicsLayerElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.layer_resolver, &other.layer_resolver)
    }
}

impl Eq for LazyGraphicsLayerElement {}

impl Hash for LazyGraphicsLayerElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let ptr = Rc::as_ptr(&self.layer_resolver) as *const ();
        ptr.hash(state);
    }
}

impl ModifierNodeElement for LazyGraphicsLayerElement {
    type Node = GraphicsLayerNode;

    fn create(&self) -> Self::Node {
        GraphicsLayerNode::new_lazy(self.layer_resolver.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.set_lazy(self.layer_resolver.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }

    fn always_update(&self) -> bool {
        true
    }
}

// ============================================================================
// Size Modifier Node
// ============================================================================

/// Node that enforces size constraints on its content.
///
/// Matches Kotlin: `SizeNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug)]
pub struct SizeNode {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce_incoming: bool,
    state: NodeState,
}

impl SizeNode {
    pub fn new(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        enforce_incoming: bool,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            enforce_incoming,
            state: NodeState::new(),
        }
    }

    /// Helper to build target constraints from element parameters
    fn target_constraints(&self) -> Constraints {
        let max_width = self.max_width.map(|v| v.max(0.0)).unwrap_or(f32::INFINITY);
        let max_height = self.max_height.map(|v| v.max(0.0)).unwrap_or(f32::INFINITY);

        let min_width = self
            .min_width
            .map(|v| {
                let clamped = v.clamp(0.0, max_width);
                if clamped == f32::INFINITY {
                    0.0
                } else {
                    clamped
                }
            })
            .unwrap_or(0.0);

        let min_height = self
            .min_height
            .map(|v| {
                let clamped = v.clamp(0.0, max_height);
                if clamped == f32::INFINITY {
                    0.0
                } else {
                    clamped
                }
            })
            .unwrap_or(0.0);

        Constraints {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    pub fn min_width(&self) -> Option<f32> {
        self.min_width
    }

    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    pub fn min_height(&self) -> Option<f32> {
        self.min_height
    }

    pub fn max_height(&self) -> Option<f32> {
        self.max_height
    }

    pub fn enforce_incoming(&self) -> bool {
        self.enforce_incoming
    }
}

impl DelegatableNode for SizeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SizeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for SizeNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let target = self.target_constraints();

        let wrapped_constraints = if self.enforce_incoming {
            // Constrain target constraints by incoming constraints
            Constraints {
                min_width: target
                    .min_width
                    .max(constraints.min_width)
                    .min(constraints.max_width),
                max_width: target
                    .max_width
                    .min(constraints.max_width)
                    .max(constraints.min_width),
                min_height: target
                    .min_height
                    .max(constraints.min_height)
                    .min(constraints.max_height),
                max_height: target
                    .max_height
                    .min(constraints.max_height)
                    .max(constraints.min_height),
            }
        } else {
            // Required size: use target, but preserve incoming if target is unspecified
            let resolved_min_width = if self.min_width.is_some() {
                target.min_width
            } else {
                constraints.min_width.min(target.max_width)
            };
            let resolved_max_width = if self.max_width.is_some() {
                target.max_width
            } else {
                constraints.max_width.max(target.min_width)
            };
            let resolved_min_height = if self.min_height.is_some() {
                target.min_height
            } else {
                constraints.min_height.min(target.max_height)
            };
            let resolved_max_height = if self.max_height.is_some() {
                target.max_height
            } else {
                constraints.max_height.max(target.min_height)
            };

            Constraints {
                min_width: resolved_min_width,
                max_width: resolved_max_width,
                min_height: resolved_min_height,
                max_height: resolved_max_height,
            }
        };

        let placeable = measurable.measure(wrapped_constraints);
        let measured_width = placeable.width();
        let measured_height = placeable.height();

        // Return the target size when both min==max (fixed size), but only if it satisfies
        // the wrapped constraints we passed down. Otherwise return measured size.
        // This handles the case where enforce_incoming=true and incoming constraints are tighter.
        let result_width = if self.min_width.is_some()
            && self.max_width.is_some()
            && self.min_width == self.max_width
            && target.min_width >= wrapped_constraints.min_width
            && target.min_width <= wrapped_constraints.max_width
        {
            target.min_width
        } else {
            measured_width
        };

        let result_height = if self.min_height.is_some()
            && self.max_height.is_some()
            && self.min_height == self.max_height
            && target.min_height >= wrapped_constraints.min_height
            && target.min_height <= wrapped_constraints.max_height
        {
            target.min_height
        } else {
            measured_height
        };

        // SizeNode doesn't offset placement - child is placed at (0, 0) relative to this node
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_width == target.max_width && target.max_width != f32::INFINITY {
            target.max_width
        } else {
            let child_height = if self.enforce_incoming {
                height
            } else {
                height.clamp(target.min_height, target.max_height)
            };
            measurable
                .min_intrinsic_width(child_height)
                .clamp(target.min_width, target.max_width)
        }
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_width == target.max_width && target.max_width != f32::INFINITY {
            target.max_width
        } else {
            let child_height = if self.enforce_incoming {
                height
            } else {
                height.clamp(target.min_height, target.max_height)
            };
            measurable
                .max_intrinsic_width(child_height)
                .clamp(target.min_width, target.max_width)
        }
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_height == target.max_height && target.max_height != f32::INFINITY {
            target.max_height
        } else {
            let child_width = if self.enforce_incoming {
                width
            } else {
                width.clamp(target.min_width, target.max_width)
            };
            measurable
                .min_intrinsic_height(child_width)
                .clamp(target.min_height, target.max_height)
        }
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_height == target.max_height && target.max_height != f32::INFINITY {
            target.max_height
        } else {
            let child_width = if self.enforce_incoming {
                width
            } else {
                width.clamp(target.min_width, target.max_width)
            };
            measurable
                .max_intrinsic_height(child_width)
                .clamp(target.min_height, target.max_height)
        }
    }

    fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
        Some(Box::new(SizeMeasurementProxy {
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: self.min_height,
            max_height: self.max_height,
            enforce_incoming: self.enforce_incoming,
        }))
    }
}

/// Measurement proxy for SizeNode that snapshots live state.
///
/// Phase 2: Instead of reconstructing nodes via `SizeNode::new()`, this proxy
/// directly implements measurement logic using the snapshotted size configuration.
struct SizeMeasurementProxy {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce_incoming: bool,
}

impl SizeMeasurementProxy {
    /// Compute target constraints from the size parameters.
    /// Matches SizeNode::target_constraints() logic.
    fn target_constraints(&self) -> Constraints {
        let max_width = self.max_width.map(|v| v.max(0.0)).unwrap_or(f32::INFINITY);
        let max_height = self.max_height.map(|v| v.max(0.0)).unwrap_or(f32::INFINITY);

        let min_width = self
            .min_width
            .map(|v| {
                let clamped = v.clamp(0.0, max_width);
                if clamped == f32::INFINITY {
                    0.0
                } else {
                    clamped
                }
            })
            .unwrap_or(0.0);

        let min_height = self
            .min_height
            .map(|v| {
                let clamped = v.clamp(0.0, max_height);
                if clamped == f32::INFINITY {
                    0.0
                } else {
                    clamped
                }
            })
            .unwrap_or(0.0);

        Constraints {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }
}

impl MeasurementProxy for SizeMeasurementProxy {
    fn measure_proxy(
        &self,
        _context: &mut dyn ModifierNodeContext,
        wrapped: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Directly implement size measurement logic (no node reconstruction)
        let target = self.target_constraints();

        let wrapped_constraints = if self.enforce_incoming {
            // Constrain target constraints by incoming constraints
            Constraints {
                min_width: target
                    .min_width
                    .max(constraints.min_width)
                    .min(constraints.max_width),
                max_width: target
                    .max_width
                    .min(constraints.max_width)
                    .max(constraints.min_width),
                min_height: target
                    .min_height
                    .max(constraints.min_height)
                    .min(constraints.max_height),
                max_height: target
                    .max_height
                    .min(constraints.max_height)
                    .max(constraints.min_height),
            }
        } else {
            // Required size: use target, but preserve incoming if target is unspecified
            let resolved_min_width = if self.min_width.is_some() {
                target.min_width
            } else {
                constraints.min_width.min(target.max_width)
            };
            let resolved_max_width = if self.max_width.is_some() {
                target.max_width
            } else {
                constraints.max_width.max(target.min_width)
            };
            let resolved_min_height = if self.min_height.is_some() {
                target.min_height
            } else {
                constraints.min_height.min(target.max_height)
            };
            let resolved_max_height = if self.max_height.is_some() {
                target.max_height
            } else {
                constraints.max_height.max(target.min_height)
            };

            Constraints {
                min_width: resolved_min_width,
                max_width: resolved_max_width,
                min_height: resolved_min_height,
                max_height: resolved_max_height,
            }
        };

        let placeable = wrapped.measure(wrapped_constraints);
        let measured_width = placeable.width();
        let measured_height = placeable.height();

        // Return the target size when both min==max (fixed size), but only if it satisfies
        // the wrapped constraints we passed down. Otherwise return measured size.
        let result_width = if self.min_width.is_some()
            && self.max_width.is_some()
            && self.min_width == self.max_width
            && target.min_width >= wrapped_constraints.min_width
            && target.min_width <= wrapped_constraints.max_width
        {
            target.min_width
        } else {
            measured_width
        };

        let result_height = if self.min_height.is_some()
            && self.max_height.is_some()
            && self.min_height == self.max_height
            && target.min_height >= wrapped_constraints.min_height
            && target.min_height <= wrapped_constraints.max_height
        {
            target.min_height
        } else {
            measured_height
        };

        // SizeNode doesn't offset placement - child is placed at (0, 0) relative to this node
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_width == target.max_width && target.max_width != f32::INFINITY {
            target.max_width
        } else {
            let child_height = if self.enforce_incoming {
                height
            } else {
                height.clamp(target.min_height, target.max_height)
            };
            wrapped
                .min_intrinsic_width(child_height)
                .clamp(target.min_width, target.max_width)
        }
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_width == target.max_width && target.max_width != f32::INFINITY {
            target.max_width
        } else {
            let child_height = if self.enforce_incoming {
                height
            } else {
                height.clamp(target.min_height, target.max_height)
            };
            wrapped
                .max_intrinsic_width(child_height)
                .clamp(target.min_width, target.max_width)
        }
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_height == target.max_height && target.max_height != f32::INFINITY {
            target.max_height
        } else {
            let child_width = if self.enforce_incoming {
                width
            } else {
                width.clamp(target.min_width, target.max_width)
            };
            wrapped
                .min_intrinsic_height(child_width)
                .clamp(target.min_height, target.max_height)
        }
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let target = self.target_constraints();
        if target.min_height == target.max_height && target.max_height != f32::INFINITY {
            target.max_height
        } else {
            let child_width = if self.enforce_incoming {
                width
            } else {
                width.clamp(target.min_width, target.max_width)
            };
            wrapped
                .max_intrinsic_height(child_width)
                .clamp(target.min_height, target.max_height)
        }
    }
}

/// Element that creates and updates size nodes.
///
/// Matches Kotlin: `SizeElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug, Clone, PartialEq)]
pub struct SizeElement {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce_incoming: bool,
}

impl SizeElement {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
            enforce_incoming: true,
        }
    }

    pub fn with_constraints(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        enforce_incoming: bool,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            enforce_incoming,
        }
    }
}

impl Hash for SizeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_option_f32(state, self.min_width);
        hash_option_f32(state, self.max_width);
        hash_option_f32(state, self.min_height);
        hash_option_f32(state, self.max_height);
        self.enforce_incoming.hash(state);
    }
}

impl ModifierNodeElement for SizeElement {
    type Node = SizeNode;

    fn create(&self) -> Self::Node {
        SizeNode::new(
            self.min_width,
            self.max_width,
            self.min_height,
            self.max_height,
            self.enforce_incoming,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        if node.min_width != self.min_width
            || node.max_width != self.max_width
            || node.min_height != self.min_height
            || node.max_height != self.max_height
            || node.enforce_incoming != self.enforce_incoming
        {
            node.min_width = self.min_width;
            node.max_width = self.max_width;
            node.min_height = self.min_height;
            node.max_height = self.max_height;
            node.enforce_incoming = self.enforce_incoming;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Clickable Modifier Node
// ============================================================================

/// Node that handles click/tap interactions.
// Drag threshold is now shared via cranpose_foundation::DRAG_THRESHOLD
use cranpose_foundation::DRAG_THRESHOLD;

use std::cell::RefCell;

// Press position is stored per-node via Rc<RefCell> for sharing with handler closure
// Node reuse is ensured by ClickableElement implementing key() to return a stable key
// The handler closure is cached to ensure the same closure (and press_position state) is returned

pub struct ClickableNode {
    on_click: Rc<dyn Fn(Point)>,
    state: NodeState,
    /// Shared press position for drag detection (per-node state, accessible by handler closure)
    press_position: Rc<RefCell<Option<Point>>>,
    /// Cached handler closure - created once, returned on every pointer_input_handler() call
    cached_handler: Rc<dyn Fn(PointerEvent)>,
}

impl std::fmt::Debug for ClickableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableNode").finish()
    }
}

impl ClickableNode {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self::with_handler(Rc::new(on_click))
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        let press_position = Rc::new(RefCell::new(None));
        let cached_handler = Self::create_handler(on_click.clone(), press_position.clone());
        Self {
            on_click,
            state: NodeState::new(),
            press_position,
            cached_handler,
        }
    }

    fn create_handler(
        handler: Rc<dyn Fn(Point)>,
        press_position: Rc<RefCell<Option<Point>>>,
    ) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            // Check if event was consumed by scroll or other gesture handlers
            if event.is_consumed() {
                // Clear press state if event was consumed
                *press_position.borrow_mut() = None;
                return;
            }

            match event.kind {
                PointerEventKind::Down => {
                    // Store global press position for drag detection on Up
                    *press_position.borrow_mut() = Some(Point {
                        x: event.global_position.x,
                        y: event.global_position.y,
                    });
                }
                PointerEventKind::Move => {
                    // Move events are tracked via press_position for drag detection
                }
                PointerEventKind::Up => {
                    // Check if this is a click (Up near Down) or a drag (Up far from Down)
                    let press_pos_value = *press_position.borrow();

                    let should_click = if let Some(press_pos) = press_pos_value {
                        let dx = event.global_position.x - press_pos.x;
                        let dy = event.global_position.y - press_pos.y;
                        let distance = (dx * dx + dy * dy).sqrt();
                        distance <= DRAG_THRESHOLD
                    } else {
                        // No Down was tracked - fire click anyway
                        // This preserves the original behavior for cases where Down
                        // was handled by a different mechanism
                        true
                    };

                    // Reset press position
                    *press_position.borrow_mut() = None;

                    if should_click {
                        handler(Point {
                            x: event.position.x,
                            y: event.position.y,
                        });
                        event.consume();
                    }
                }
                PointerEventKind::Cancel => {
                    // Clear press state on cancel
                    *press_position.borrow_mut() = None;
                }
                PointerEventKind::Scroll | PointerEventKind::Enter | PointerEventKind::Exit => {
                    // These events don't affect click press state.
                }
            }
        })
    }

    pub fn handler(&self) -> Rc<dyn Fn(Point)> {
        self.on_click.clone()
    }
}

impl DelegatableNode for ClickableNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ClickableNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::PointerInput);
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl PointerInputNode for ClickableNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
    ) -> bool {
        // Delegate to the cached handler - single source of truth for click logic
        // This avoids duplicating the press position tracking and threshold checking
        (self.cached_handler)(event.clone());
        event.is_consumed()
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        // Always participate in hit testing
        true
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        // Return the cached handler - this ensures the same closure (with its press_position state)
        // is used across multiple calls to pointer_input_handler()
        Some(self.cached_handler.clone())
    }
}

/// Element that creates and updates clickable nodes.
#[derive(Clone)]
pub struct ClickableElement {
    on_click: Rc<dyn Fn(Point)>,
}

impl ClickableElement {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_click: Rc::new(on_click),
        }
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self { on_click }
    }
}

impl std::fmt::Debug for ClickableElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableElement").finish()
    }
}

impl PartialEq for ClickableElement {
    fn eq(&self, _other: &Self) -> bool {
        // Type matching is sufficient - node will be updated via update() method
        // This matches JC behavior where nodes are reused for same-type elements,
        // preserving press_position state for proper drag detection
        true
    }
}

impl Eq for ClickableElement {}

impl Hash for ClickableElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Consistent hash for type-based matching
        "clickable".hash(state);
    }
}

impl ModifierNodeElement for ClickableElement {
    type Node = ClickableNode;

    fn create(&self) -> Self::Node {
        ClickableNode::with_handler(self.on_click.clone())
    }

    // Note: key() is deliberately NOT implemented (returns None by default)
    // This enables type-based node reuse: the same ClickableNode instance is
    // reused across recompositions, preserving the cached_handler and its
    // captured press_position state for proper drag detection.

    fn update(&self, node: &mut Self::Node) {
        // Update the handler - the cached_handler needs to be recreated
        // with the new on_click while preserving press_position
        node.on_click = self.on_click.clone();
        // Recreate the cached handler with the same press_position but new click handler
        node.cached_handler =
            ClickableNode::create_handler(node.on_click.clone(), node.press_position.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }

    fn always_update(&self) -> bool {
        // Always update to capture new closure while preserving node state
        true
    }
}

// ============================================================================
// Alpha Modifier Node
// ============================================================================

/// Node that applies alpha transparency to its content.
#[derive(Debug)]
pub struct AlphaNode {
    alpha: f32,
    state: NodeState,
}

impl AlphaNode {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for AlphaNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for AlphaNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for AlphaNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Save the current alpha/layer state
        // 2. Apply the alpha value to the graphics context
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous state
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates alpha nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct AlphaElement {
    alpha: f32,
}

impl AlphaElement {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl Hash for AlphaElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.alpha);
    }
}

impl ModifierNodeElement for AlphaElement {
    type Node = AlphaNode;

    fn create(&self) -> Self::Node {
        AlphaNode::new(self.alpha)
    }

    fn update(&self, node: &mut Self::Node) {
        let new_alpha = self.alpha.clamp(0.0, 1.0);
        if (node.alpha - new_alpha).abs() > f32::EPSILON {
            node.alpha = new_alpha;
            // In a full implementation, would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Clip-To-Bounds Modifier Node
// ============================================================================

/// Node that marks the subtree for clipping during rendering.
#[derive(Debug)]
pub struct ClipToBoundsNode {
    state: NodeState,
}

impl ClipToBoundsNode {
    pub fn new() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for ClipToBoundsNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ClipToBoundsNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for ClipToBoundsNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}
}

/// Element that creates clip-to-bounds nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipToBoundsElement;

impl ClipToBoundsElement {
    pub fn new() -> Self {
        Self
    }
}

impl ModifierNodeElement for ClipToBoundsElement {
    type Node = ClipToBoundsNode;

    fn create(&self) -> Self::Node {
        ClipToBoundsNode::new()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Draw Command Modifier Node
// ============================================================================

/// Node that stores draw commands emitted by draw modifiers.
pub struct DrawCommandNode {
    commands: Vec<DrawCommand>,
    state: NodeState,
}

impl DrawCommandNode {
    pub fn new(commands: Vec<DrawCommand>) -> Self {
        Self {
            commands,
            state: NodeState::new(),
        }
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }
}

impl DelegatableNode for DrawCommandNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DrawCommandNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Draw);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for DrawCommandNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}
}

fn draw_command_tag(cmd: &DrawCommand) -> u8 {
    match cmd {
        DrawCommand::Behind(_) => 0,
        DrawCommand::WithContent(_) => 1,
        DrawCommand::Overlay(_) => 2,
    }
}

fn draw_command_closure_identity(cmd: &DrawCommand) -> *const () {
    match cmd {
        DrawCommand::Behind(f) | DrawCommand::WithContent(f) | DrawCommand::Overlay(f) => {
            Rc::as_ptr(f) as *const ()
        }
    }
}

/// Element that wires draw commands into the modifier node chain.
#[derive(Clone)]
pub struct DrawCommandElement {
    commands: Vec<DrawCommand>,
}

impl DrawCommandElement {
    pub fn new(command: DrawCommand) -> Self {
        Self {
            commands: vec![command],
        }
    }

    pub fn from_commands(commands: Vec<DrawCommand>) -> Self {
        Self { commands }
    }
}

impl std::fmt::Debug for DrawCommandElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrawCommandElement")
            .field("commands", &self.commands.len())
            .finish()
    }
}

impl PartialEq for DrawCommandElement {
    fn eq(&self, other: &Self) -> bool {
        if self.commands.len() != other.commands.len() {
            return false;
        }
        self.commands
            .iter()
            .zip(other.commands.iter())
            .all(|(a, b)| {
                draw_command_tag(a) == draw_command_tag(b)
                    && draw_command_closure_identity(a) == draw_command_closure_identity(b)
            })
    }
}

impl Eq for DrawCommandElement {}

impl std::hash::Hash for DrawCommandElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "draw_commands".hash(state);
        self.commands.len().hash(state);
        for command in &self.commands {
            draw_command_tag(command).hash(state);
            (draw_command_closure_identity(command) as usize).hash(state);
        }
    }
}

impl ModifierNodeElement for DrawCommandElement {
    type Node = DrawCommandNode;

    fn create(&self) -> Self::Node {
        DrawCommandNode::new(self.commands.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.commands = self.commands.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Offset Modifier Node
// ============================================================================

/// Node that offsets its content by a fixed (x, y) amount.
///
/// Matches Kotlin: `OffsetNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt
#[derive(Debug)]
pub struct OffsetNode {
    x: f32,
    y: f32,
    rtl_aware: bool,
    state: NodeState,
}

impl OffsetNode {
    pub fn new(x: f32, y: f32, rtl_aware: bool) -> Self {
        Self {
            x,
            y,
            rtl_aware,
            state: NodeState::new(),
        }
    }

    pub fn offset(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    pub fn rtl_aware(&self) -> bool {
        self.rtl_aware
    }
}

impl DelegatableNode for OffsetNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for OffsetNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for OffsetNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Offset doesn't affect measurement, just placement
        let placeable = measurable.measure(constraints);

        // Return child size unchanged, but specify the offset for placement
        cranpose_ui_layout::LayoutModifierMeasureResult::new(
            Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            self.x, // Place child offset by x
            self.y, // Place child offset by y
        )
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.max_intrinsic_height(width)
    }

    fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
        Some(Box::new(OffsetMeasurementProxy {
            x: self.x,
            y: self.y,
            rtl_aware: self.rtl_aware,
        }))
    }
}

/// Measurement proxy for OffsetNode that snapshots live state.
///
/// Phase 2: Instead of reconstructing nodes via `OffsetNode::new()`, this proxy
/// directly implements measurement logic. Since offset doesn't affect measurement
/// (only placement), this is a simple passthrough.
struct OffsetMeasurementProxy {
    x: f32,
    y: f32,
    #[allow(dead_code)]
    rtl_aware: bool,
}

impl MeasurementProxy for OffsetMeasurementProxy {
    fn measure_proxy(
        &self,
        _context: &mut dyn ModifierNodeContext,
        wrapped: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Offset doesn't affect measurement, just placement - simple passthrough
        let placeable = wrapped.measure(constraints);

        // Return child size unchanged, but specify the offset for placement
        cranpose_ui_layout::LayoutModifierMeasureResult::new(
            Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            self.x, // Place child offset by x
            self.y, // Place child offset by y
        )
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        wrapped.min_intrinsic_width(height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        wrapped.max_intrinsic_width(height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        wrapped.min_intrinsic_height(width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        wrapped.max_intrinsic_height(width)
    }
}

/// Element that creates and updates offset nodes.
///
/// Matches Kotlin: `OffsetElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetElement {
    x: f32,
    y: f32,
    rtl_aware: bool,
}

impl OffsetElement {
    pub fn new(x: f32, y: f32, rtl_aware: bool) -> Self {
        Self { x, y, rtl_aware }
    }
}

impl Hash for OffsetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.x);
        hash_f32_value(state, self.y);
        self.rtl_aware.hash(state);
    }
}

impl ModifierNodeElement for OffsetElement {
    type Node = OffsetNode;

    fn create(&self) -> Self::Node {
        OffsetNode::new(self.x, self.y, self.rtl_aware)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.x != self.x || node.y != self.y || node.rtl_aware != self.rtl_aware {
            node.x = self.x;
            node.y = self.y;
            node.rtl_aware = self.rtl_aware;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Fill Modifier Node
// ============================================================================

/// Direction for fill modifiers (horizontal, vertical, or both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FillDirection {
    Horizontal,
    Vertical,
    Both,
}

/// Node that fills the maximum available space in one or both dimensions.
///
/// Matches Kotlin: `FillNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug)]
pub struct FillNode {
    direction: FillDirection,
    fraction: f32,
    state: NodeState,
}

impl FillNode {
    pub fn new(direction: FillDirection, fraction: f32) -> Self {
        Self {
            direction,
            fraction,
            state: NodeState::new(),
        }
    }

    pub fn direction(&self) -> FillDirection {
        self.direction
    }

    pub fn fraction(&self) -> f32 {
        self.fraction
    }
}

impl DelegatableNode for FillNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for FillNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for FillNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Calculate the fill size based on constraints
        let (fill_width, child_min_width, child_max_width) = if self.direction
            != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            let width = (constraints.max_width * self.fraction)
                .round()
                .clamp(constraints.min_width, constraints.max_width);
            // Tight constraint for child on this axis
            (width, width, width)
        } else {
            (
                constraints.max_width,
                constraints.min_width,
                constraints.max_width,
            )
        };

        let (fill_height, child_min_height, child_max_height) = if self.direction
            != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            let height = (constraints.max_height * self.fraction)
                .round()
                .clamp(constraints.min_height, constraints.max_height);
            // Tight constraint for child on this axis
            (height, height, height)
        } else {
            (
                constraints.max_height,
                constraints.min_height,
                constraints.max_height,
            )
        };

        let fill_constraints = Constraints {
            min_width: child_min_width,
            max_width: child_max_width,
            min_height: child_min_height,
            max_height: child_max_height,
        };

        let placeable = measurable.measure(fill_constraints);

        // Return the FILL size, not the child size.
        // The child is measured within tight constraints on the fill axis,
        // but we report the fill size to our parent.
        let result_width = if self.direction != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            fill_width
        } else {
            placeable.width()
        };

        let result_height = if self.direction != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            fill_height
        } else {
            placeable.height()
        };

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.max_intrinsic_height(width)
    }

    fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
        Some(Box::new(FillMeasurementProxy {
            direction: self.direction,
            fraction: self.fraction,
        }))
    }
}

/// Measurement proxy for FillNode that snapshots live state.
///
/// Phase 2: Instead of reconstructing nodes via `FillNode::new()`, this proxy
/// directly implements measurement logic using the snapshotted fill configuration.
struct FillMeasurementProxy {
    direction: FillDirection,
    fraction: f32,
}

impl MeasurementProxy for FillMeasurementProxy {
    fn measure_proxy(
        &self,
        _context: &mut dyn ModifierNodeContext,
        wrapped: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Calculate the fill size based on constraints
        let (fill_width, child_min_width, child_max_width) = if self.direction
            != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            let width = (constraints.max_width * self.fraction)
                .round()
                .clamp(constraints.min_width, constraints.max_width);
            (width, width, width)
        } else {
            (
                constraints.max_width,
                constraints.min_width,
                constraints.max_width,
            )
        };

        let (fill_height, child_min_height, child_max_height) = if self.direction
            != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            let height = (constraints.max_height * self.fraction)
                .round()
                .clamp(constraints.min_height, constraints.max_height);
            (height, height, height)
        } else {
            (
                constraints.max_height,
                constraints.min_height,
                constraints.max_height,
            )
        };

        let fill_constraints = Constraints {
            min_width: child_min_width,
            max_width: child_max_width,
            min_height: child_min_height,
            max_height: child_max_height,
        };

        let placeable = wrapped.measure(fill_constraints);

        // Return the FILL size, not the child size
        let result_width = if self.direction != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            fill_width
        } else {
            placeable.width()
        };

        let result_height = if self.direction != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            fill_height
        } else {
            placeable.height()
        };

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        wrapped.min_intrinsic_width(height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        wrapped.max_intrinsic_width(height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        wrapped.min_intrinsic_height(width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        wrapped.max_intrinsic_height(width)
    }
}

/// Element that creates and updates fill nodes.
///
/// Matches Kotlin: `FillElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug, Clone, PartialEq)]
pub struct FillElement {
    direction: FillDirection,
    fraction: f32,
}

impl FillElement {
    pub fn width(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Horizontal,
            fraction,
        }
    }

    pub fn height(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Vertical,
            fraction,
        }
    }

    pub fn size(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Both,
            fraction,
        }
    }
}

impl Hash for FillElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.direction.hash(state);
        hash_f32_value(state, self.fraction);
    }
}

impl ModifierNodeElement for FillElement {
    type Node = FillNode;

    fn create(&self) -> Self::Node {
        FillNode::new(self.direction, self.fraction)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.direction != self.direction || node.fraction != self.fraction {
            node.direction = self.direction;
            node.fraction = self.fraction;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Weight Modifier Node
// ============================================================================

/// Node that records flex weight data for Row/Column parents.
#[derive(Debug)]
pub struct WeightNode {
    weight: f32,
    fill: bool,
    state: NodeState,
}

impl WeightNode {
    pub fn new(weight: f32, fill: bool) -> Self {
        Self {
            weight,
            fill,
            state: NodeState::new(),
        }
    }

    pub fn layout_weight(&self) -> LayoutWeight {
        LayoutWeight {
            weight: self.weight,
            fill: self.fill,
        }
    }
}

impl DelegatableNode for WeightNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for WeightNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }
}

/// Element that creates and updates weight nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightElement {
    weight: f32,
    fill: bool,
}

impl WeightElement {
    pub fn new(weight: f32, fill: bool) -> Self {
        Self { weight, fill }
    }
}

impl Hash for WeightElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.weight);
        self.fill.hash(state);
    }
}

impl ModifierNodeElement for WeightElement {
    type Node = WeightNode;

    fn create(&self) -> Self::Node {
        WeightNode::new(self.weight, self.fill)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.weight != self.weight || node.fill != self.fill {
            node.weight = self.weight;
            node.fill = self.fill;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Alignment Modifier Node
// ============================================================================

/// Node that records alignment preferences for Box/Row/Column scopes.
#[derive(Debug)]
pub struct AlignmentNode {
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
    state: NodeState,
}

impl AlignmentNode {
    pub fn new(
        box_alignment: Option<Alignment>,
        column_alignment: Option<HorizontalAlignment>,
        row_alignment: Option<VerticalAlignment>,
    ) -> Self {
        Self {
            box_alignment,
            column_alignment,
            row_alignment,
            state: NodeState::new(),
        }
    }

    pub fn box_alignment(&self) -> Option<Alignment> {
        self.box_alignment
    }

    pub fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    pub fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }
}

impl DelegatableNode for AlignmentNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for AlignmentNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }
}

/// Element that creates and updates alignment nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentElement {
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
}

impl AlignmentElement {
    pub fn box_alignment(alignment: Alignment) -> Self {
        Self {
            box_alignment: Some(alignment),
            column_alignment: None,
            row_alignment: None,
        }
    }

    pub fn column_alignment(alignment: HorizontalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: Some(alignment),
            row_alignment: None,
        }
    }

    pub fn row_alignment(alignment: VerticalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: None,
            row_alignment: Some(alignment),
        }
    }
}

impl Hash for AlignmentElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(alignment) = self.box_alignment {
            state.write_u8(1);
            hash_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
        if let Some(alignment) = self.column_alignment {
            state.write_u8(1);
            hash_horizontal_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
        if let Some(alignment) = self.row_alignment {
            state.write_u8(1);
            hash_vertical_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
    }
}

impl ModifierNodeElement for AlignmentElement {
    type Node = AlignmentNode;

    fn create(&self) -> Self::Node {
        AlignmentNode::new(
            self.box_alignment,
            self.column_alignment,
            self.row_alignment,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        if node.box_alignment != self.box_alignment {
            node.box_alignment = self.box_alignment;
        }
        if node.column_alignment != self.column_alignment {
            node.column_alignment = self.column_alignment;
        }
        if node.row_alignment != self.row_alignment {
            node.row_alignment = self.row_alignment;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Intrinsic Size Modifier Node
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntrinsicAxis {
    Width,
    Height,
}

/// Node that records intrinsic sizing requests.
#[derive(Debug)]
pub struct IntrinsicSizeNode {
    axis: IntrinsicAxis,
    size: IntrinsicSize,
    state: NodeState,
}

impl IntrinsicSizeNode {
    pub fn new(axis: IntrinsicAxis, size: IntrinsicSize) -> Self {
        Self {
            axis,
            size,
            state: NodeState::new(),
        }
    }

    pub fn axis(&self) -> IntrinsicAxis {
        self.axis
    }

    pub fn intrinsic_size(&self) -> IntrinsicSize {
        self.size
    }
}

impl DelegatableNode for IntrinsicSizeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for IntrinsicSizeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }
}

/// Element that creates and updates intrinsic size nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct IntrinsicSizeElement {
    axis: IntrinsicAxis,
    size: IntrinsicSize,
}

impl IntrinsicSizeElement {
    pub fn width(size: IntrinsicSize) -> Self {
        Self {
            axis: IntrinsicAxis::Width,
            size,
        }
    }

    pub fn height(size: IntrinsicSize) -> Self {
        Self {
            axis: IntrinsicAxis::Height,
            size,
        }
    }
}

impl Hash for IntrinsicSizeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(match self.axis {
            IntrinsicAxis::Width => 0,
            IntrinsicAxis::Height => 1,
        });
        state.write_u8(match self.size {
            IntrinsicSize::Min => 0,
            IntrinsicSize::Max => 1,
        });
    }
}

impl ModifierNodeElement for IntrinsicSizeElement {
    type Node = IntrinsicSizeNode;

    fn create(&self) -> Self::Node {
        IntrinsicSizeNode::new(self.axis, self.size)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.axis != self.axis {
            node.axis = self.axis;
        }
        if node.size != self.size {
            node.size = self.size;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[cfg(test)]
#[path = "tests/modifier_nodes_tests.rs"]
mod tests;
