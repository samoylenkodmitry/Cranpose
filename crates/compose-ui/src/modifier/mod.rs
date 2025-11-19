//! Modifier system for Compose-RS
//!
//! This module now acts as a thin builder around modifier elements. Each
//! [`Modifier`] stores the element chain required by the modifier node system
//! together with inspector metadata while resolved state is computed directly
//! from the modifier nodes.

#![allow(non_snake_case)]
#![allow(dead_code)]

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
pub use chain::{ModifierChainHandle, ModifierChainInspectorNode, ModifierLocalsHandle};
use compose_foundation::ModifierNodeElement;
pub use compose_foundation::{
    modifier_element, AnyModifierElement, DynModifierElement, FocusState, PointerEvent,
    PointerEventKind, SemanticsConfiguration,
};
pub use compose_ui_graphics::{
    Brush, Color, CornerRadii, EdgeInsets, GraphicsLayer, Point, Rect, RoundedCornerShape, Size,
};
use compose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};
#[allow(unused_imports)]
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
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorProperty {
    pub name: &'static str,
    pub value: String,
}

/// Structured inspector payload describing a modifier element.
#[derive(Clone, Debug, PartialEq)]
pub struct ModifierInspectorRecord {
    pub name: &'static str,
    pub properties: Vec<InspectorProperty>,
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

    fn to_record(&self) -> ModifierInspectorRecord {
        ModifierInspectorRecord {
            name: self.name,
            properties: self.info.properties().to_vec(),
        }
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

/// Internal representation of modifier composition structure.
/// This mirrors Jetpack Compose's CombinedModifier pattern where modifiers
/// form a persistent tree structure instead of eagerly flattening into vectors.
#[derive(Clone)]
enum ModifierKind {
    /// Empty modifier (like Modifier.companion in Kotlin)
    Empty,
    /// Single modifier with elements and inspector metadata
    Single {
        elements: Rc<Vec<DynModifierElement>>,
        inspector: Rc<Vec<InspectorMetadata>>,
    },
    /// Combined modifier tree node (like CombinedModifier in Kotlin)
    Combined {
        outer: Rc<Modifier>,
        inner: Rc<Modifier>,
    },
}

/// A modifier chain that can be applied to composable elements.
/// Modifiers form a persistent tree structure (via CombinedModifier pattern)
/// to enable O(1) composition and structural sharing during recomposition.
#[derive(Clone)]
pub struct Modifier {
    kind: ModifierKind,
}

impl Default for Modifier {
    fn default() -> Self {
        Self {
            kind: ModifierKind::Empty,
        }
    }
}

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Clip the content to the bounds of this modifier.
    ///
    /// Example: `Modifier::empty().clip_to_bounds()`
    pub fn clip_to_bounds(self) -> Self {
        let modifier = Self::with_element(ClipToBoundsElement::new()).with_inspector_metadata(
            inspector_metadata("clipToBounds", |info| {
                info.add_property("clipToBounds", "true");
            }),
        );
        self.then(modifier)
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
        let mut preview = SemanticsConfiguration::default();
        recorder(&mut preview);
        let description = preview.content_description.clone();
        let is_button = preview.is_button;
        let is_clickable = preview.is_clickable;
        let metadata = inspector_metadata("semantics", move |info| {
            if let Some(desc) = &description {
                info.add_property("contentDescription", desc.clone());
            }
            if is_button {
                info.add_property("isButton", "true");
            }
            if is_clickable {
                info.add_property("isClickable", "true");
            }
        });
        let element = SemanticsElement::new(recorder);
        let modifier =
            Modifier::from_parts(vec![modifier_element(element)]).with_inspector_metadata(metadata);
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

    /// Enables debug logging for this modifier chain.
    ///
    /// When enabled, logs the entire modifier chain structure including:
    /// - Element types and their properties
    /// - Inspector metadata
    /// - Capability flags
    ///
    /// This is useful for debugging modifier composition issues and understanding
    /// how the modifier chain is structured at runtime.
    ///
    /// Example:
    /// ```ignore
    /// Modifier::empty()
    ///     .padding(8.0)
    ///     .background(Color(1.0, 0.0, 0.0, 1.0))
    ///     .debug_chain("MyWidget")
    /// ```
    pub fn debug_chain(self, tag: &'static str) -> Self {
        use compose_foundation::{ModifierNode, ModifierNodeContext, NodeCapabilities, NodeState};

        #[derive(Clone)]
        struct DebugChainElement {
            tag: &'static str,
        }

        impl fmt::Debug for DebugChainElement {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("DebugChainElement")
                    .field("tag", &self.tag)
                    .finish()
            }
        }

        impl PartialEq for DebugChainElement {
            fn eq(&self, other: &Self) -> bool {
                self.tag == other.tag
            }
        }

        impl Eq for DebugChainElement {}

        impl std::hash::Hash for DebugChainElement {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.tag.hash(state);
            }
        }

        impl ModifierNodeElement for DebugChainElement {
            type Node = DebugChainNode;

            fn create(&self) -> Self::Node {
                DebugChainNode::new(self.tag)
            }

            fn update(&self, node: &mut Self::Node) {
                node.tag = self.tag;
            }

            fn capabilities(&self) -> NodeCapabilities {
                NodeCapabilities::empty()
            }
        }

        struct DebugChainNode {
            tag: &'static str,
            state: NodeState,
        }

        impl DebugChainNode {
            fn new(tag: &'static str) -> Self {
                Self {
                    tag,
                    state: NodeState::new(),
                }
            }
        }

        impl ModifierNode for DebugChainNode {
            fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
                eprintln!("[debug_chain:{}] Modifier chain attached", self.tag);
            }

            fn on_detach(&mut self) {
                eprintln!("[debug_chain:{}] Modifier chain detached", self.tag);
            }

            fn on_reset(&mut self) {
                eprintln!("[debug_chain:{}] Modifier chain reset", self.tag);
            }
        }

        impl compose_foundation::DelegatableNode for DebugChainNode {
            fn node_state(&self) -> &NodeState {
                &self.state
            }
        }

        let element = DebugChainElement { tag };
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier).with_inspector_metadata(inspector_metadata("debugChain", move |info| {
            info.add_property("tag", tag);
        }))
    }

    /// Concatenates this modifier with another.
    ///
    /// This creates a persistent tree structure (CombinedModifier pattern) rather than
    /// eagerly flattening into a vector, enabling O(1) composition and structural sharing.
    ///
    /// Mirrors Jetpack Compose: `infix fun then(other: Modifier): Modifier =
    ///     if (other === Modifier) this else CombinedModifier(this, other)`
    pub fn then(&self, next: Modifier) -> Modifier {
        if self.is_trivially_empty() {
            return next;
        }
        if next.is_trivially_empty() {
            return self.clone();
        }
        Modifier {
            kind: ModifierKind::Combined {
                outer: Rc::new(self.clone()),
                inner: Rc::new(next),
            },
        }
    }

    /// Returns the flattened list of elements in this modifier chain.
    /// For backward compatibility, this flattens the tree structure on-demand.
    /// Note: This allocates a new Vec for Combined modifiers.
    pub(crate) fn elements(&self) -> Vec<DynModifierElement> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { elements, .. } => elements.as_ref().clone(),
            ModifierKind::Combined { outer, inner } => {
                let mut result = outer.elements();
                result.extend(inner.elements());
                result
            }
        }
    }

    /// Returns the flattened list of inspector metadata in this modifier chain.
    /// For backward compatibility, this flattens the tree structure on-demand.
    /// Note: This allocates a new Vec for Combined modifiers.
    pub(crate) fn inspector_metadata(&self) -> Vec<InspectorMetadata> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { inspector, .. } => inspector.as_ref().clone(),
            ModifierKind::Combined { outer, inner } => {
                let mut result = outer.inspector_metadata();
                result.extend(inner.inspector_metadata());
                result
            }
        }
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

    pub fn draw_commands(&self) -> Vec<DrawCommand> {
        collect_slices_from_modifier(self).draw_commands().to_vec()
    }

    pub fn clips_to_bounds(&self) -> bool {
        collect_slices_from_modifier(self).clip_to_bounds()
    }

    /// Returns structured inspector records for each modifier element.
    pub fn collect_inspector_records(&self) -> Vec<ModifierInspectorRecord> {
        self.inspector_metadata()
            .iter()
            .map(|metadata| metadata.to_record())
            .collect()
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

    pub(crate) fn from_parts(elements: Vec<DynModifierElement>) -> Self {
        if elements.is_empty() {
            Self {
                kind: ModifierKind::Empty,
            }
        } else {
            Self {
                kind: ModifierKind::Single {
                    elements: Rc::new(elements),
                    inspector: Rc::new(Vec::new()),
                },
            }
        }
    }

    fn is_trivially_empty(&self) -> bool {
        matches!(self.kind, ModifierKind::Empty)
    }

    pub(crate) fn with_inspector_metadata(self, metadata: InspectorMetadata) -> Self {
        if metadata.is_empty() {
            return self;
        }
        match self.kind {
            ModifierKind::Empty => self,
            ModifierKind::Single {
                elements,
                inspector,
            } => {
                let mut new_inspector = inspector.as_ref().clone();
                new_inspector.push(metadata);
                Self {
                    kind: ModifierKind::Single {
                        elements,
                        inspector: Rc::new(new_inspector),
                    },
                }
            }
            ModifierKind::Combined { .. } => {
                // Combined modifiers shouldn't have inspector metadata added directly
                // This should only be called on freshly created modifiers
                panic!("Cannot add inspector metadata to a combined modifier")
            }
        }
    }
}

impl ComposeModifier for Modifier {
    /// Accumulates a value by visiting modifier elements in insertion order (left to right).
    /// Mirrors Jetpack Compose CombinedModifier:
    /// `inner.foldIn(outer.foldIn(initial, operation), operation)`
    fn fold_in<R, F>(&self, initial: R, operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R,
    {
        self.fold_in_impl(initial, &mut { operation })
    }

    /// Accumulates a value by visiting modifier elements in reverse order (right to left).
    /// Mirrors Jetpack Compose CombinedModifier:
    /// `outer.foldOut(inner.foldOut(initial, operation), operation)`
    fn fold_out<R, F>(&self, initial: R, operation: F) -> R
    where
        F: FnMut(R, &dyn AnyModifierElement) -> R,
    {
        self.fold_out_impl(initial, &mut { operation })
    }

    /// Returns true if any element in the chain satisfies the predicate.
    /// Mirrors Jetpack Compose CombinedModifier:
    /// `outer.any(predicate) || inner.any(predicate)`
    fn any<F>(&self, predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool,
    {
        self.any_impl(&mut { predicate })
    }

    /// Returns true only if all elements in the chain satisfy the predicate.
    /// Mirrors Jetpack Compose CombinedModifier:
    /// `outer.all(predicate) && inner.all(predicate)`
    fn all<F>(&self, predicate: F) -> bool
    where
        F: FnMut(&dyn AnyModifierElement) -> bool,
    {
        self.all_impl(&mut { predicate })
    }
}

impl Modifier {
    /// Internal implementation of `fold_in` that can be called recursively.
    fn fold_in_impl<R>(
        &self,
        mut initial: R,
        operation: &mut dyn FnMut(R, &dyn AnyModifierElement) -> R,
    ) -> R {
        match &self.kind {
            ModifierKind::Empty => initial,
            ModifierKind::Single { elements, .. } => {
                for element in elements.iter() {
                    let erased: &dyn AnyModifierElement = element.as_ref();
                    initial = operation(initial, erased);
                }
                initial
            }
            ModifierKind::Combined { outer, inner } => {
                // Process outer first, then inner (like Kotlin's CombinedModifier)
                let after_outer = outer.fold_in_impl(initial, operation);
                inner.fold_in_impl(after_outer, operation)
            }
        }
    }

    /// Internal implementation of `fold_out` that can be called recursively.
    fn fold_out_impl<R>(
        &self,
        mut initial: R,
        operation: &mut dyn FnMut(R, &dyn AnyModifierElement) -> R,
    ) -> R {
        match &self.kind {
            ModifierKind::Empty => initial,
            ModifierKind::Single { elements, .. } => {
                for element in elements.iter().rev() {
                    let erased: &dyn AnyModifierElement = element.as_ref();
                    initial = operation(initial, erased);
                }
                initial
            }
            ModifierKind::Combined { outer, inner } => {
                // Process inner first (in reverse), then outer (in reverse)
                let after_inner = inner.fold_out_impl(initial, operation);
                outer.fold_out_impl(after_inner, operation)
            }
        }
    }

    /// Internal implementation of `any` that can be called recursively.
    fn any_impl(&self, predicate: &mut dyn FnMut(&dyn AnyModifierElement) -> bool) -> bool {
        match &self.kind {
            ModifierKind::Empty => false,
            ModifierKind::Single { elements, .. } => {
                for element in elements.iter() {
                    if predicate(element.as_ref()) {
                        return true;
                    }
                }
                false
            }
            ModifierKind::Combined { outer, inner } => {
                outer.any_impl(predicate) || inner.any_impl(predicate)
            }
        }
    }

    /// Internal implementation of `all` that can be called recursively.
    fn all_impl(&self, predicate: &mut dyn FnMut(&dyn AnyModifierElement) -> bool) -> bool {
        match &self.kind {
            ModifierKind::Empty => true,
            ModifierKind::Single { elements, .. } => {
                for element in elements.iter() {
                    if !predicate(element.as_ref()) {
                        return false;
                    }
                }
                true
            }
            ModifierKind::Combined { outer, inner } => {
                outer.all_impl(predicate) && inner.all_impl(predicate)
            }
        }
    }
}

impl InspectableModifier for Modifier {
    fn inspect(&self, info: &mut InspectorInfo) {
        match &self.kind {
            ModifierKind::Empty => {}
            ModifierKind::Single { inspector, .. } => {
                for metadata in inspector.iter() {
                    metadata.append_to(info);
                }
            }
            ModifierKind::Combined { outer, inner } => {
                outer.inspect(info);
                inner.inspect(info);
            }
        }
    }
}

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (ModifierKind::Empty, ModifierKind::Empty) => true,
            (
                ModifierKind::Single {
                    elements: e1,
                    inspector: _,
                },
                ModifierKind::Single {
                    elements: e2,
                    inspector: _,
                },
            ) => {
                // Fast path: if they share the same Rc, they're definitely equal
                if Rc::ptr_eq(e1, e2) {
                    return true;
                }

                // Slow path: compare elements by value
                if e1.len() != e2.len() {
                    return false;
                }

                for (a, b) in e1.iter().zip(e2.iter()) {
                    if !a.equals_element(&**b) {
                        return false;
                    }
                }
                true
            }
            (
                ModifierKind::Combined {
                    outer: o1,
                    inner: i1,
                },
                ModifierKind::Combined {
                    outer: o2,
                    inner: i2,
                },
            ) => {
                // Fast path: if they share the same Rc pointers, they're definitely equal
                if Rc::ptr_eq(o1, o2) && Rc::ptr_eq(i1, i2) {
                    return true;
                }
                // Recursive comparison
                o1 == o2 && i1 == i2
            }
            _ => false,
        }
    }
}

impl Eq for Modifier {}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ModifierKind::Empty => write!(f, "Modifier.empty"),
            ModifierKind::Single { elements, .. } => {
                if elements.is_empty() {
                    return write!(f, "Modifier.empty");
                }
                write!(f, "Modifier[")?;
                for (index, element) in elements.iter().enumerate() {
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
            ModifierKind::Combined { outer: _, inner: _ } => {
                // Flatten the representation for display
                // This matches Kotlin's CombinedModifier toString behavior
                write!(f, "[")?;
                let elements = self.elements();
                for (index, element) in elements.iter().enumerate() {
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
    layout: LayoutProperties,
    offset: Point,
}

impl Default for ResolvedModifiers {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::default(),
            layout: LayoutProperties::default(),
            offset: Point::default(),
        }
    }
}

impl ResolvedModifiers {
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    pub fn layout_properties(&self) -> LayoutProperties {
        self.layout
    }

    pub fn offset(&self) -> Point {
        self.offset
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
