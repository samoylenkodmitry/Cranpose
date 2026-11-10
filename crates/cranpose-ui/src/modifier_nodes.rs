use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, InvalidationKind, LayoutModifierNode,
    Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities,
    NodeState, PointerEvent, PointerInputNode, Size,
};
use cranpose_ui_graphics::{ColorFilter, DrawScope, PointerIcon};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};

mod alignment;
mod background;
mod clickable;
mod clip;
mod fill;
mod intrinsic_size;
mod offset;
mod padding;
mod size;
mod weight;

pub(crate) use alignment::{AlignmentElement, AlignmentNode};
pub use background::{BackgroundElement, CornerShapeElement};
pub use clickable::{ClickableElement, ClickableNode};
pub(crate) use clip::{ClipToBoundsElement, ClipToBoundsNode};
pub use fill::{FillDirection, FillElement};
pub(crate) use intrinsic_size::IntrinsicSizeElement;
pub use offset::{FractionalOffsetElement, FractionalOffsetNode, OffsetElement, OffsetNode};
pub use padding::PaddingElement;
pub(crate) use padding::device_padding;
pub use size::SizeElement;
pub(crate) use weight::{WeightElement, WeightNode};

use crate::{
    draw::DrawCommand,
    modifier::{BlendMode, Brush, CompositingStrategy, GraphicsLayer, RoundedCornerShape},
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

/// Hashes which 3D role a layer has and which mesh it is drawn on: equal
/// layers hash alike, and the camera and placement are left to equality.
fn hash_space<H: Hasher>(state: &mut H, space: Option<&cranpose_ui_graphics::space::LayerSpace>) {
    use cranpose_ui_graphics::space::LayerSpace;
    match space {
        None => state.write_u8(0),
        Some(LayerSpace::Scene(_)) => state.write_u8(1),
        Some(LayerSpace::Surface(surface)) => {
            state.write_u8(2);
            state.write_u64(surface.mesh.id());
        }
        Some(LayerSpace::Transform(_)) => state.write_u8(3),
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
    hash_space(state, layer.space.as_deref());
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
    match &layer.color_filter {
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
            for &value in matrix.iter() {
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

macro_rules! impl_layout_modifier_node {
    ($ty:ty) => {
        impl ModifierNode for $ty {
            fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
                Some(self)
            }

            fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
                Some(self)
            }
        }
    };
    ($ty:ty, invalidate = $invalidation:expr) => {
        impl ModifierNode for $ty {
            fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
                context.invalidate($invalidation);
            }

            fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
                Some(self)
            }

            fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
                Some(self)
            }
        }
    };
}
pub(crate) use impl_layout_modifier_node;

macro_rules! impl_draw_modifier_node {
    ($ty:ty) => {
        impl ModifierNode for $ty {
            fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
                context.invalidate(InvalidationKind::Draw);
            }

            fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
                Some(self)
            }

            fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
                Some(self)
            }
        }

        impl DrawModifierNode for $ty {}
    };
}
pub(crate) use impl_draw_modifier_node;

macro_rules! impl_sink_reporter_element {
    ($element:ty, $node:ty) => {
        impl ModifierNodeElement for $element {
            type Node = $node;

            fn create(&self) -> Self::Node {
                <$node>::new(self.sink.clone())
            }

            fn update(&self, node: &mut Self::Node) {
                node.sink = self.sink.clone();
            }

            fn capabilities(&self) -> NodeCapabilities {
                NodeCapabilities::LAYOUT
            }
        }
    };
}

fn measure_pass_through(
    measurable: &dyn Measurable,
    constraints: Constraints,
    offset: impl FnOnce(Size) -> (f32, f32),
) -> cranpose_ui_layout::LayoutModifierMeasureResult {
    let placeable = measurable.measure(constraints);
    let size = Size {
        width: placeable.width(),
        height: placeable.height(),
    };
    let (x, y) = offset(size);
    cranpose_ui_layout::LayoutModifierMeasureResult::new(size, x, y)
}

fn attach_draw_observer(
    node_id_cell: &Cell<Option<NodeId>>,
    context: &mut dyn ModifierNodeContext,
) {
    node_id_cell.set(context.node_id());
    context.invalidate(InvalidationKind::Draw);
}

fn detach_draw_observer(node_id_cell: &Cell<Option<NodeId>>) {
    if let Some(node_id) = node_id_cell.replace(None) {
        crate::render_state::clear_draw_observations_for_node(node_id);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderElement {
    width: f32,
    brush: Brush,
    shape: RoundedCornerShape,
}

impl BorderElement {
    pub(crate) fn new(width: f32, brush: Brush, shape: RoundedCornerShape) -> Self {
        Self {
            width,
            brush,
            shape,
        }
    }

    /// The draw command stroking the border over the content.
    pub(crate) fn command(&self) -> DrawCommand {
        let Self {
            width,
            brush,
            shape,
        } = self.clone();
        DrawCommand::Overlay(Rc::new(move |scope| {
            let size = scope.size();
            if width <= 0.0 || size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let radii = shape.resolve(size.width, size.height);
            let half = width / 2.0;
            if width * 2.0 >= size.width.min(size.height) {
                // A stroke as thick as half the node covers all of it.
                scope.draw_round_rect(brush.clone(), radii);
                return;
            }
            let inset = |radius: f32| (radius - half).max(0.0);
            scope.draw_round_rect_at_stroked(
                cranpose_ui_graphics::Rect {
                    x: half,
                    y: half,
                    width: size.width - width,
                    height: size.height - width,
                },
                brush.clone(),
                cranpose_ui_graphics::CornerRadii {
                    top_left: inset(radii.top_left),
                    top_right: inset(radii.top_right),
                    bottom_right: inset(radii.bottom_right),
                    bottom_left: inset(radii.bottom_left),
                },
                cranpose_ui_graphics::Stroke::new(width),
            );
        }))
    }
}

impl Hash for BorderElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.width);
        std::mem::discriminant(&self.brush).hash(state);
        if let Brush::Solid(color) = self.brush {
            hash_f32_value(state, color.0);
            hash_f32_value(state, color.1);
            hash_f32_value(state, color.2);
            hash_f32_value(state, color.3);
        }
        let radii = self.shape.radii();
        hash_f32_value(state, radii.top_left);
        hash_f32_value(state, radii.top_right);
        hash_f32_value(state, radii.bottom_right);
        hash_f32_value(state, radii.bottom_left);
    }
}

impl ModifierNodeElement for BorderElement {
    type Node = DrawCommandNode;

    fn create(&self) -> Self::Node {
        DrawCommandNode::new(vec![self.command()])
    }

    fn update(&self, node: &mut Self::Node) {
        node.commands = vec![self.command()];
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

pub(crate) struct GraphicsLayerNode {
    layer: GraphicsLayer,
    layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    node_id: Rc<Cell<Option<NodeId>>>,
    state: NodeState,
}

impl GraphicsLayerNode {
    pub(crate) fn new(layer: GraphicsLayer) -> Self {
        Self {
            layer,
            layer_resolver: None,
            node_id: Rc::new(Cell::new(None)),
            state: NodeState::new(),
        }
    }

    pub(crate) fn new_lazy(layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) -> Self {
        Self {
            layer: GraphicsLayer::default(),
            layer_resolver: Some(layer_resolver),
            node_id: Rc::new(Cell::new(None)),
            state: NodeState::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn layer(&self) -> GraphicsLayer {
        if let Some(resolve) = self.layer_resolver(0) {
            resolve()
        } else {
            self.layer.clone()
        }
    }

    pub(crate) fn layer_snapshot(&self) -> GraphicsLayer {
        self.layer.clone()
    }

    pub(crate) fn layer_resolver(
        &self,
        modifier_index: usize,
    ) -> Option<Rc<dyn Fn() -> GraphicsLayer>> {
        self.layer_resolver.as_ref().map(|resolve| {
            let resolve = Rc::clone(resolve);
            let node_id = Rc::clone(&self.node_id);
            Rc::new(move || {
                if let Some(node_id) = node_id.get() {
                    let scope = crate::render_state::DrawObservationScope::layer_properties(
                        node_id,
                        modifier_index,
                    );
                    crate::render_state::observe_draw_reads(scope, || resolve())
                } else {
                    resolve()
                }
            }) as Rc<dyn Fn() -> GraphicsLayer>
        })
    }

    fn set_static(&mut self, layer: GraphicsLayer) {
        let changed = self.layer != layer || self.layer_resolver.is_some();
        self.layer = layer;
        self.layer_resolver = None;
        if changed && let Some(node_id) = self.node_id.get() {
            crate::render_state::schedule_draw_repass(node_id);
        }
    }

    pub(crate) fn set_lazy(&mut self, layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) {
        let changed = self
            .layer_resolver
            .as_ref()
            .is_none_or(|current| !Rc::ptr_eq(current, &layer_resolver));
        self.layer_resolver = Some(layer_resolver);
        if changed && let Some(node_id) = self.node_id.get() {
            crate::render_state::schedule_draw_repass(node_id);
        }
    }
}

impl DelegatableNode for GraphicsLayerNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for GraphicsLayerNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        attach_draw_observer(&self.node_id, context);
    }

    fn on_detach(&mut self) {
        detach_draw_observer(&self.node_id);
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GraphicsLayerElement {
    layer: GraphicsLayer,
}

impl GraphicsLayerElement {
    pub(crate) fn new(layer: GraphicsLayer) -> Self {
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

#[derive(Clone)]
pub(crate) struct LazyGraphicsLayerElement {
    layer_resolver: Rc<dyn Fn() -> GraphicsLayer>,
}

impl LazyGraphicsLayerElement {
    pub(crate) fn new(layer_resolver: Rc<dyn Fn() -> GraphicsLayer>) -> Self {
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
        GraphicsLayerNode::new_lazy(Rc::clone(&self.layer_resolver))
    }

    fn update(&self, node: &mut Self::Node) {
        node.set_lazy(Rc::clone(&self.layer_resolver));
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }

    fn always_update(&self) -> bool {
        true
    }

    fn auto_invalidate_on_update(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub(crate) struct PointerIconNode {
    icon: PointerIcon,
    state: NodeState,
}

impl PointerIconNode {
    pub(crate) fn new(icon: PointerIcon) -> Self {
        Self {
            icon,
            state: NodeState::new(),
        }
    }

    pub(crate) fn icon(&self) -> &PointerIcon {
        &self.icon
    }
}

impl DelegatableNode for PointerIconNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for PointerIconNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::PointerInput);
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl PointerInputNode for PointerIconNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _event: &PointerEvent,
    ) -> bool {
        false
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        true
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PointerIconElement {
    icon: PointerIcon,
}

impl PointerIconElement {
    pub(crate) fn new(icon: PointerIcon) -> Self {
        Self { icon }
    }
}

impl ModifierNodeElement for PointerIconElement {
    type Node = PointerIconNode;

    fn create(&self) -> Self::Node {
        PointerIconNode::new(self.icon.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.icon = self.icon.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

/// Node that applies alpha transparency to its content.
#[derive(Debug)]
pub struct AlphaNode {
    alpha: f32,
    state: NodeState,
}

impl AlphaNode {
    /// Creates a node that draws its content at opacity `alpha`, clamped to the range 0.0 to 1.0.
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

impl_draw_modifier_node!(AlphaNode);

/// Element that creates and updates alpha nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct AlphaElement {
    alpha: f32,
}

impl AlphaElement {
    /// Creates an element for a node that draws its content at opacity `alpha`, clamped to the
    /// range 0.0 to 1.0.
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
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

pub trait WindowRectSink {
    fn set(&self, coordinates: cranpose_ui_graphics::WindowCoordinates);
}

impl WindowRectSink for Cell<cranpose_ui_graphics::Rect> {
    fn set(&self, coordinates: cranpose_ui_graphics::WindowCoordinates) {
        Self::set(self, coordinates.bounds());
    }
}

impl WindowRectSink for Cell<cranpose_ui_graphics::WindowCoordinates> {
    fn set(&self, coordinates: cranpose_ui_graphics::WindowCoordinates) {
        Self::set(self, coordinates);
    }
}

struct StateWindowRectSink(cranpose_core::MutableState<cranpose_ui_graphics::Rect>);

impl WindowRectSink for StateWindowRectSink {
    fn set(&self, coordinates: cranpose_ui_graphics::WindowCoordinates) {
        self.0.set(coordinates.bounds());
    }
}

pub(crate) struct WindowRectReporterNode {
    sink: Rc<dyn WindowRectSink>,
    state: NodeState,
}

impl WindowRectReporterNode {
    pub(crate) fn new(sink: Rc<dyn WindowRectSink>) -> Self {
        Self {
            sink,
            state: NodeState::new(),
        }
    }

    pub(crate) fn window_rect_sink(&self) -> Rc<dyn WindowRectSink> {
        Rc::clone(&self.sink)
    }
}

impl DelegatableNode for WindowRectReporterNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(WindowRectReporterNode);

impl LayoutModifierNode for WindowRectReporterNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        measure_pass_through(measurable, constraints, |_| (0.0, 0.0))
    }
}

#[derive(Clone)]
pub(crate) struct WindowRectReporterElement {
    sink: Rc<dyn WindowRectSink>,
}

impl WindowRectReporterElement {
    pub(crate) fn new(sink: Rc<Cell<cranpose_ui_graphics::Rect>>) -> Self {
        Self { sink }
    }

    pub(crate) fn from_coordinates(
        sink: Rc<Cell<cranpose_ui_graphics::WindowCoordinates>>,
    ) -> Self {
        Self { sink }
    }

    pub(crate) fn from_state(
        sink: cranpose_core::MutableState<cranpose_ui_graphics::Rect>,
    ) -> Self {
        Self {
            sink: Rc::new(StateWindowRectSink(sink)),
        }
    }
}

impl std::fmt::Debug for WindowRectReporterElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowRectReporterElement").finish()
    }
}

impl PartialEq for WindowRectReporterElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.sink, &other.sink)
    }
}

impl Eq for WindowRectReporterElement {}

impl Hash for WindowRectReporterElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Rc::as_ptr(&self.sink).cast::<()>(), state);
    }
}

impl_sink_reporter_element!(WindowRectReporterElement, WindowRectReporterNode);

pub(crate) struct SelectableTextNode {
    sink: Rc<crate::selection_container::SelectableGeometry>,
    state: NodeState,
}

impl SelectableTextNode {
    pub(crate) fn new(sink: Rc<crate::selection_container::SelectableGeometry>) -> Self {
        Self {
            sink,
            state: NodeState::new(),
        }
    }

    pub(crate) fn geometry(&self) -> &crate::selection_container::SelectableGeometry {
        &self.sink
    }
}

impl DelegatableNode for SelectableTextNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(SelectableTextNode);

impl LayoutModifierNode for SelectableTextNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        measure_pass_through(measurable, constraints, |_| (0.0, 0.0))
    }
}

#[derive(Clone)]
pub(crate) struct SelectableTextElement {
    sink: Rc<crate::selection_container::SelectableGeometry>,
}

impl SelectableTextElement {
    pub(crate) fn new(sink: Rc<crate::selection_container::SelectableGeometry>) -> Self {
        Self { sink }
    }
}

impl std::fmt::Debug for SelectableTextElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectableTextElement").finish()
    }
}

impl PartialEq for SelectableTextElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.sink, &other.sink)
    }
}

impl Eq for SelectableTextElement {}

impl Hash for SelectableTextElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Rc::as_ptr(&self.sink), state);
    }
}

impl_sink_reporter_element!(SelectableTextElement, SelectableTextNode);

pub(crate) trait SizeSink {
    fn set(&self, size: Size);
}

impl SizeSink for Cell<Size> {
    fn set(&self, size: Size) {
        Self::set(self, size);
    }
}

struct StateSizeSink(cranpose_core::MutableState<Size>);

impl SizeSink for StateSizeSink {
    fn set(&self, size: Size) {
        self.0.set(size);
    }
}

pub(crate) struct SizeReporterNode {
    sink: Rc<dyn SizeSink>,
    state: NodeState,
    #[cfg(debug_assertions)]
    oscillation: Cell<(Size, Size, u32)>,
}

impl SizeReporterNode {
    pub(crate) fn new(sink: Rc<dyn SizeSink>) -> Self {
        Self {
            sink,
            state: NodeState::new(),
            #[cfg(debug_assertions)]
            oscillation: Cell::new((Size::default(), Size::default(), 0)),
        }
    }

    #[cfg(debug_assertions)]
    fn check_oscillation(&self, size: Size) {
        const ALTERNATION_CEILING: u32 = 64;
        let (last, second_last, count) = self.oscillation.get();
        let count = if size == second_last && size != last {
            count + 1
        } else if size == last {
            count
        } else {
            0
        };
        assert!(
            count <= ALTERNATION_CEILING,
            "size-reactive feedback loop: this node's measured size has \
             alternated between {last:?} and {size:?} for {count} passes — \
             its content's size depends on the size it reports (the \
             onSizeChanged self-reference hazard). Break the cycle by making \
             the reported size feed only content that does not change this \
             node's own measured size."
        );
        self.oscillation.set((size, last, count));
    }
}

impl DelegatableNode for SizeReporterNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(SizeReporterNode);

impl LayoutModifierNode for SizeReporterNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        measure_pass_through(measurable, constraints, |size| {
            #[cfg(debug_assertions)]
            self.check_oscillation(size);
            self.sink.set(size);
            (0.0, 0.0)
        })
    }
}

#[derive(Clone)]
pub(crate) struct SizeReporterElement {
    sink: Rc<dyn SizeSink>,
}

impl SizeReporterElement {
    pub(crate) fn new(sink: Rc<Cell<Size>>) -> Self {
        Self { sink }
    }

    pub(crate) fn from_state(sink: cranpose_core::MutableState<Size>) -> Self {
        Self {
            sink: Rc::new(StateSizeSink(sink)),
        }
    }
}

impl std::fmt::Debug for SizeReporterElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeReporterElement").finish()
    }
}

impl PartialEq for SizeReporterElement {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Rc::as_ptr(&self.sink), Rc::as_ptr(&other.sink))
    }
}

impl Hash for SizeReporterElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.sink) as *const () as usize).hash(state);
    }
}

impl_sink_reporter_element!(SizeReporterElement, SizeReporterNode);

pub(crate) struct DrawCommandNode {
    commands: Vec<DrawCommand>,
    node_id: Cell<Option<NodeId>>,
    state: NodeState,
}

impl DrawCommandNode {
    pub(crate) fn new(commands: Vec<DrawCommand>) -> Self {
        Self {
            commands,
            node_id: Cell::new(None),
            state: NodeState::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    pub(crate) fn observed_commands(
        &self,
        modifier_index: usize,
    ) -> impl Iterator<Item = DrawCommand> + '_ {
        let node_id = self.node_id.get();
        self.commands
            .iter()
            .cloned()
            .enumerate()
            .map(move |(index, command)| {
                observe_draw_command(command, node_id, modifier_index, index)
            })
    }
}

impl DelegatableNode for DrawCommandNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DrawCommandNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        attach_draw_observer(&self.node_id, context);
    }

    fn on_detach(&mut self) {
        detach_draw_observer(&self.node_id);
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }
}

impl DrawModifierNode for DrawCommandNode {}

fn observe_draw_command(
    command: DrawCommand,
    node_id: Option<NodeId>,
    modifier_index: usize,
    command_index: usize,
) -> DrawCommand {
    let Some(node_id) = node_id else {
        return command;
    };
    let observation =
        crate::render_state::DrawObservationScope::new(node_id, modifier_index, command_index);
    match command {
        DrawCommand::Behind(draw) => DrawCommand::Behind(Rc::new(move |scope| {
            crate::render_state::observe_draw_reads(observation, || draw(scope));
        })),
        DrawCommand::WithContent(draw) => DrawCommand::WithContent(Rc::new(move |scope| {
            crate::render_state::observe_draw_reads(observation, || draw(scope));
        })),
        DrawCommand::Overlay(draw) => DrawCommand::Overlay(Rc::new(move |scope| {
            crate::render_state::observe_draw_reads(observation, || draw(scope));
        })),
        // A background reads no state.
        background @ DrawCommand::Background(_) => background,
    }
}

fn draw_command_closure_identity(cmd: &DrawCommand) -> Option<*const ()> {
    match cmd {
        DrawCommand::Behind(f) | DrawCommand::WithContent(f) | DrawCommand::Overlay(f) => {
            Some(Rc::as_ptr(f) as *const ())
        }
        DrawCommand::Background(_) => None,
    }
}

#[derive(Clone)]
pub(crate) struct DrawCommandElement {
    commands: Vec<DrawCommand>,
}

impl DrawCommandElement {
    pub(crate) fn new(command: DrawCommand) -> Self {
        Self {
            commands: vec![command],
        }
    }

    pub(crate) fn from_commands(commands: Vec<DrawCommand>) -> Self {
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
            .all(|(a, b)| a.same_as(b))
    }
}

impl Eq for DrawCommandElement {}

impl Hash for DrawCommandElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "draw_commands".hash(state);
        self.commands.len().hash(state);
        for command in &self.commands {
            std::mem::discriminant(command).hash(state);
            draw_command_closure_identity(command).hash(state);
        }
    }
}

impl ModifierNodeElement for DrawCommandElement {
    type Node = DrawCommandNode;

    fn create(&self) -> Self::Node {
        DrawCommandNode::new(self.commands.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.commands.clone_from(&self.commands);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

#[cfg(test)]
#[path = "tests/modifier_nodes_tests.rs"]
mod tests;
