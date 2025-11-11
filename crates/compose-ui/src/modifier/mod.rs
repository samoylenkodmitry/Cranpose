//! Modifier system for Compose-RS
//!
//! This module now acts as a thin builder around modifier elements. Each
//! [`Modifier`] stores the element chain required by the modifier node system
//! together with inspector metadata while resolved state is computed directly
//! from the modifier nodes.

#![allow(non_snake_case)]

use std::fmt;
use std::rc::Rc;

mod alignment;
mod background;
mod chain;
mod clickable;
mod draw_cache;
mod fill;
mod focus;
mod graphics_layer;
mod local;
mod offset;
mod padding;
mod pointer_input;
mod semantics;
mod size;
mod slices;
mod weight;

pub use crate::draw::{DrawCacheBuilder, DrawCommand};
#[allow(unused_imports)]
pub use chain::{ModifierChainHandle, ModifierLocalsHandle};
use compose_foundation::ModifierNodeElement;
pub use compose_foundation::{
    modifier_element, AnyModifierElement, DynModifierElement, FocusState, PointerEvent,
    PointerEventKind, SemanticsConfiguration,
};
pub use compose_ui_graphics::{
    Brush, Color, CornerRadii, EdgeInsets, GraphicsLayer, Point, Rect, RoundedCornerShape, Size,
};
use compose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};
pub use focus::{FocusDirection, FocusRequester};
#[allow(unused_imports)]
pub use local::{modifier_local_of, ModifierLocalKey, ModifierLocalReadScope};
pub(crate) use local::{
    ModifierLocalAncestorResolver, ModifierLocalSource, ModifierLocalToken, ResolvedModifierLocal,
};
#[allow(unused_imports)]
pub use pointer_input::{AwaitPointerEventScope, PointerInputScope};
pub use semantics::{collect_semantics_from_chain, collect_semantics_from_modifier};
pub use slices::{collect_modifier_slices, collect_slices_from_modifier, ModifierNodeSlices};

use crate::modifier_nodes::ClipToBoundsElement;
use focus::{FocusRequesterElement, FocusTargetElement};
use local::{ModifierLocalConsumerElement, ModifierLocalProviderElement};
use semantics::SemanticsElement;

/// Trait mirroring Jetpack Compose's `Modifier` interface.
///
/// Implementors expose helper operations that fold over modifier elements or
/// evaluate predicates against the chain without materializing intermediate
/// allocations. This matches Kotlin's `foldIn`, `foldOut`, `any`, and `all`
/// helpers and allows downstream code to treat modifiers abstractly instead of
/// poking at cached resolved state.
pub trait ComposeModifier {
    /// Accumulates a value by visiting modifier elements in insertion order.
    fn fold_in<R, F>(&self, initial: R, operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R;

    /// Accumulates a value by visiting modifier elements in reverse order.
    fn fold_out<R, F>(&self, initial: R, operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R;

    /// Returns true when any element satisfies the predicate.
    fn any<F>(&self, predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool;

    /// Returns true only if all elements satisfy the predicate.
    fn all<F>(&self, predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool;
}

/// Minimal inspector metadata storage.
#[derive(Clone, Debug, Default)]
pub struct InspectorInfo {
    properties: Vec<InspectorProperty>,
}

impl InspectorInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_property<V: Into<String>>(&mut self, name: &'static str, value: V) {
        self.properties.push(InspectorProperty {
            name,
            value: value.into(),
        });
    }

    pub fn properties(&self) -> &[InspectorProperty] {
        &self.properties
    }

    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    pub fn add_dimension(&mut self, name: &'static str, constraint: DimensionConstraint) {
        self.add_property(name, describe_dimension(constraint));
    }

    pub fn add_offset_components(
        &mut self,
        x_name: &'static str,
        y_name: &'static str,
        offset: Point,
    ) {
        self.add_property(x_name, offset.x.to_string());
        self.add_property(y_name, offset.y.to_string());
    }

    pub fn add_alignment<A>(&mut self, name: &'static str, alignment: A)
    where
        A: fmt::Debug,
    {
        self.add_property(name, format!("{alignment:?}"));
    }

    pub fn debug_properties(&self) -> Vec<(&'static str, String)> {
        self.properties
            .iter()
            .map(|property| (property.name, property.value.clone()))
            .collect()
    }

    pub fn describe(&self) -> String {
        self.properties
            .iter()
            .map(|property| format!("{}={}", property.name, property.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Single inspector entry recording a property exposed by a modifier.
#[derive(Clone, Debug)]
pub struct InspectorProperty {
    pub name: &'static str,
    pub value: String,
}

/// Helper describing the metadata contributed by a modifier factory.
#[derive(Clone, Debug)]
pub(crate) struct InspectorMetadata {
    name: &'static str,
    info: InspectorInfo,
}

impl InspectorMetadata {
    pub(crate) fn new<F>(name: &'static str, recorder: F) -> Self
    where
        F: FnOnce(&mut InspectorInfo),
    {
        let mut info = InspectorInfo::new();
        recorder(&mut info);
        Self { name, info }
    }

    fn append_to(&self, target: &mut InspectorInfo) {
        if self.info.is_empty() {
            target.add_property(self.name, "applied");
        } else {
            for property in self.info.properties() {
                target.add_property(property.name, property.value.clone());
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.info.is_empty()
    }
}

fn describe_dimension(constraint: DimensionConstraint) -> String {
    match constraint {
        DimensionConstraint::Unspecified => "unspecified".to_string(),
        DimensionConstraint::Points(value) => value.to_string(),
        DimensionConstraint::Fraction(value) => format!("fraction({value})"),
        DimensionConstraint::Intrinsic(size) => format!("intrinsic({size:?})"),
    }
}

pub(crate) fn inspector_metadata<F>(name: &'static str, recorder: F) -> InspectorMetadata
where
    F: FnOnce(&mut InspectorInfo),
{
    InspectorMetadata::new(name, recorder)
}

/// Trait implemented by modifiers that can describe themselves for tooling.
pub trait InspectableModifier {
    /// Human-readable name exposed to inspector tooling.
    fn inspector_name(&self) -> &'static str {
        "Modifier"
    }

    /// Records inspector metadata for the modifier chain.
    fn inspect(&self, _info: &mut InspectorInfo) {}
}

#[derive(Clone, Default)]
pub struct Modifier {
    elements: Rc<Vec<DynModifierElement>>,
    inspector: Rc<Vec<InspectorMetadata>>,
}

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn clip_to_bounds() -> Self {
        Self::with_element(ClipToBoundsElement::new()).with_inspector_metadata(inspector_metadata(
            "clipToBounds",
            |info| {
                info.add_property("clipToBounds", "true");
            },
        ))
    }

    pub fn modifier_local_provider<T, F>(self, key: ModifierLocalKey<T>, value: F) -> Self
    where
        T: 'static,
        F: Fn() -> T + 'static,
    {
        let element = ModifierLocalProviderElement::new(key, value);
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    pub fn modifier_local_consumer<F>(self, consumer: F) -> Self
    where
        F: for<'scope> Fn(&mut ModifierLocalReadScope<'scope>) + 'static,
    {
        let element = ModifierLocalConsumerElement::new(consumer);
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    pub fn semantics<F>(self, recorder: F) -> Self
    where
        F: Fn(&mut SemanticsConfiguration) + 'static,
    {
        let element = SemanticsElement::new(recorder);
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    /// Makes this component focusable.
    ///
    /// This adds a focus target node that can receive focus and participate
    /// in focus traversal. The component will be included in tab order and
    /// can be focused programmatically.
    pub fn focus_target(self) -> Self {
        let element = FocusTargetElement::new();
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    /// Makes this component focusable with a callback for focus changes.
    ///
    /// The callback is invoked whenever the focus state changes, allowing
    /// components to react to gaining or losing focus.
    pub fn on_focus_changed<F>(self, callback: F) -> Self
    where
        F: Fn(FocusState) + 'static,
    {
        let element = FocusTargetElement::with_callback(callback);
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    /// Attaches a focus requester to this component.
    ///
    /// The requester can be used to programmatically request focus for
    /// this component from application code.
    pub fn focus_requester(self, requester: &FocusRequester) -> Self {
        let element = FocusRequesterElement::new(requester.id());
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    pub fn then(&self, next: Modifier) -> Modifier {
        if self.is_trivially_empty() {
            return next;
        }
        if next.is_trivially_empty() {
            return self.clone();
        }
        let mut elements = Vec::with_capacity(self.elements.len() + next.elements.len());
        elements.extend(self.elements.iter().cloned());
        elements.extend(next.elements.iter().cloned());
        let mut inspector = Vec::with_capacity(self.inspector.len() + next.inspector.len());
        inspector.extend(self.inspector.iter().cloned());
        inspector.extend(next.inspector.iter().cloned());
        Modifier {
            elements: Rc::new(elements),
            inspector: Rc::new(inspector),
        }
    }

    pub(crate) fn elements(&self) -> &[DynModifierElement] {
        &self.elements
    }

    pub fn total_padding(&self) -> f32 {
        let padding = self.padding_values();
        padding
            .left
            .max(padding.right)
            .max(padding.top)
            .max(padding.bottom)
    }

    pub fn explicit_size(&self) -> Option<Size> {
        let props = self.layout_properties();
        match (props.width, props.height) {
            (DimensionConstraint::Points(width), DimensionConstraint::Points(height)) => {
                Some(Size { width, height })
            }
            _ => None,
        }
    }

    pub fn padding_values(&self) -> EdgeInsets {
        self.resolved_modifiers().padding()
    }

    pub(crate) fn total_offset(&self) -> Point {
        self.resolved_modifiers().offset()
    }

    pub(crate) fn layout_properties(&self) -> LayoutProperties {
        self.resolved_modifiers().layout_properties()
    }

    pub fn box_alignment(&self) -> Option<Alignment> {
        self.layout_properties().box_alignment()
    }

    pub fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.layout_properties().column_alignment()
    }

    pub fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.layout_properties().row_alignment()
    }

    pub fn background_color(&self) -> Option<Color> {
        self.resolved_modifiers()
            .background()
            .map(|background| background.color())
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.resolved_modifiers().corner_shape()
    }

    pub fn draw_commands(&self) -> Vec<DrawCommand> {
        collect_slices_from_modifier(self).draw_commands().to_vec()
    }

    pub fn graphics_layer_values(&self) -> Option<GraphicsLayer> {
        self.resolved_modifiers().graphics_layer()
    }

    pub fn clips_to_bounds(&self) -> bool {
        collect_slices_from_modifier(self).clip_to_bounds()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(self);
        handle.resolved_modifiers()
    }

    fn with_element<E>(element: E) -> Self
    where
        E: ModifierNodeElement,
    {
        let dyn_element = modifier_element(element);
        Self::from_parts(vec![dyn_element])
    }

    fn from_parts(elements: Vec<DynModifierElement>) -> Self {
        Self {
            elements: Rc::new(elements),
            inspector: Rc::new(Vec::new()),
        }
    }

    fn is_trivially_empty(&self) -> bool {
        self.elements.is_empty() && self.inspector.is_empty()
    }

    pub(crate) fn with_inspector_metadata(mut self, metadata: InspectorMetadata) -> Self {
        if metadata.is_empty() {
            return self;
        }
        Rc::make_mut(&mut self.inspector).push(metadata);
        self
    }
}

impl ComposeModifier for Modifier {
    fn fold_in<R, F>(&self, mut initial: R, mut operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R,
    {
        for element in self.elements.iter() {
            let erased: &dyn AnyModifierElement = element.as_ref();
            initial = operation(initial, erased);
        }
        initial
    }

    fn fold_out<R, F>(&self, mut initial: R, mut operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R,
    {
        for element in self.elements.iter().rev() {
            let erased: &dyn AnyModifierElement = element.as_ref();
            initial = operation(initial, erased);
        }
        initial
    }

    fn any<F>(&self, mut predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool,
    {
        for element in self.elements.iter() {
            if predicate(element.as_ref()) {
                return true;
            }
        }
        false
    }

    fn all<F>(&self, mut predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool,
    {
        for element in self.elements.iter() {
            if !predicate(element.as_ref()) {
                return false;
            }
        }
        true
    }
}

impl InspectableModifier for Modifier {
    fn inspect(&self, info: &mut InspectorInfo) {
        for metadata in self.inspector.iter() {
            metadata.append_to(info);
        }
    }
}

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.elements, &other.elements) && Rc::ptr_eq(&self.inspector, &other.inspector)
    }
}

impl Eq for Modifier {}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.elements.is_empty() {
            return write!(f, "Modifier.empty");
        }
        write!(f, "Modifier[")?;
        for (index, element) in self.elements.iter().enumerate() {
            if index > 0 {
                write!(f, ", ")?;
            }
            let name = element.inspector_name();
            let mut properties = Vec::new();
            element.record_inspector_properties(&mut |prop, value| {
                properties.push(format!("{prop}={value}"));
            });
            if properties.is_empty() {
                write!(f, "{name}")?;
            } else {
                write!(f, "{name}({})", properties.join(", "))?;
            }
        }
        write!(f, "]")
    }
}

impl fmt::Debug for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedBackground {
    color: Color,
    shape: Option<RoundedCornerShape>,
}

impl ResolvedBackground {
    pub fn new(color: Color, shape: Option<RoundedCornerShape>) -> Self {
        Self { color, shape }
    }

    pub fn color(&self) -> Color {
        self.color
    }

    pub fn shape(&self) -> Option<RoundedCornerShape> {
        self.shape
    }

    pub fn set_shape(&mut self, shape: Option<RoundedCornerShape>) {
        self.shape = shape;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedModifiers {
    padding: EdgeInsets,
    background: Option<ResolvedBackground>,
    corner_shape: Option<RoundedCornerShape>,
    layout: LayoutProperties,
    offset: Point,
    graphics_layer: Option<GraphicsLayer>,
}

impl Default for ResolvedModifiers {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::default(),
            background: None,
            corner_shape: None,
            layout: LayoutProperties::default(),
            offset: Point::default(),
            graphics_layer: None,
        }
    }
}

impl ResolvedModifiers {
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    pub fn background(&self) -> Option<ResolvedBackground> {
        self.background
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.corner_shape
    }

    pub fn layout_properties(&self) -> LayoutProperties {
        self.layout
    }

    pub fn offset(&self) -> Point {
        self.offset
    }

    pub fn graphics_layer(&self) -> Option<GraphicsLayer> {
        self.graphics_layer
    }

    pub(crate) fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    pub(crate) fn add_padding(&mut self, padding: EdgeInsets) {
        self.padding += padding;
    }

    pub(crate) fn set_layout_properties(&mut self, layout: LayoutProperties) {
        self.layout = layout;
    }

    pub(crate) fn set_offset(&mut self, offset: Point) {
        self.offset = offset;
    }

    pub(crate) fn set_graphics_layer(&mut self, layer: Option<GraphicsLayer>) {
        self.graphics_layer = layer;
    }

    pub(crate) fn set_background_color(&mut self, color: Color) {
        self.background = Some(ResolvedBackground::new(color, self.corner_shape));
    }

    pub(crate) fn clear_background(&mut self) {
        self.background = None;
    }

    pub(crate) fn set_corner_shape(&mut self, shape: Option<RoundedCornerShape>) {
        self.corner_shape = shape;
        if let Some(background) = &mut self.background {
            background.set_shape(shape);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DimensionConstraint {
    #[default]
    Unspecified,
    Points(f32),
    Fraction(f32),
    Intrinsic(IntrinsicSize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutWeight {
    pub weight: f32,
    pub fill: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutProperties {
    padding: EdgeInsets,
    width: DimensionConstraint,
    height: DimensionConstraint,
    min_width: Option<f32>,
    min_height: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
    weight: Option<LayoutWeight>,
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
}

impl LayoutProperties {
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    pub fn width(&self) -> DimensionConstraint {
        self.width
    }

    pub fn height(&self) -> DimensionConstraint {
        self.height
    }

    pub fn min_width(&self) -> Option<f32> {
        self.min_width
    }

    pub fn min_height(&self) -> Option<f32> {
        self.min_height
    }

    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    pub fn max_height(&self) -> Option<f32> {
        self.max_height
    }

    pub fn weight(&self) -> Option<LayoutWeight> {
        self.weight
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

    fn merged(self, other: LayoutProperties) -> LayoutProperties {
        let mut result = self;
        result.padding += other.padding;
        if other.width != DimensionConstraint::Unspecified {
            result.width = other.width;
        }
        if other.height != DimensionConstraint::Unspecified {
            result.height = other.height;
        }
        if other.min_width.is_some() {
            result.min_width = other.min_width;
        }
        if other.min_height.is_some() {
            result.min_height = other.min_height;
        }
        if other.max_width.is_some() {
            result.max_width = other.max_width;
        }
        if other.max_height.is_some() {
            result.max_height = other.max_height;
        }
        if other.weight.is_some() {
            result.weight = other.weight;
        }
        if other.box_alignment.is_some() {
            result.box_alignment = other.box_alignment;
        }
        if other.column_alignment.is_some() {
            result.column_alignment = other.column_alignment;
        }
        if other.row_alignment.is_some() {
            result.row_alignment = other.row_alignment;
        }
        result
    }
}

#[cfg(test)]
#[path = "tests/modifier_tests.rs"]
mod tests;
