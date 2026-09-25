//! Modifier system for Cranpose
//!
//! This module now acts as a thin builder around modifier elements. Each
//! [`Modifier`] stores the element chain required by the modifier node system
//! together with inspector metadata while resolved state is computed directly
//! from the modifier nodes.

use std::{
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{ProvidedValue, hash::default};

mod alignment;
mod background;
mod blur;
mod chain;
mod clickable;
mod drag_and_drop;
mod draw_cache;
mod fill;
mod focus;
mod focus_ring;
mod graphics_layer;
mod local;
mod minimum_interactive;
mod offset;
mod padding;
mod pointer_icon;
pub(crate) mod pointer_input;
mod rotary_input;
mod scroll;
mod selectable;
mod semantics;
mod shadow;
mod size;
mod slices;
mod toggleable;
mod weight;
mod window_root;

pub use chain::{ModifierChainHandle, ModifierChainInspectorNode, ModifierLocalsHandle};
pub use cranpose_foundation::{
    AnyModifierElement, DynModifierElement, FocusState, PointerEvent, PointerEventKind,
    PointerSource, RotaryScrollEvent, SemanticsConfiguration, modifier_element,
};
use cranpose_foundation::{ModifierNodeElement, NodeCapabilities, ProgressBarRangeInfo};
#[expect(unused_imports)]
pub use cranpose_ui_graphics::{
    BlendMode, BlurredEdgeTreatment, Brush, Color, ColorFilter, CompositingStrategy, CornerRadii,
    CursorIcon, CustomPointerIcon, CutDirection, Dp, DpOffset, EdgeInsets, GradientCutMaskSpec,
    GradientFadeMaskSpec, GraphicsLayer, LayerShape, Point, PointerIcon, PointerIconError, Rect,
    RenderEffect, RoundedCornerShape, RuntimeShader, Shadow, ShadowScope, Size, TransformOrigin,
};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};
pub use drag_and_drop::{
    DragAndDropEvent, DragAndDropOutcome, DragAndDropPayload, DragAndDropPoint, DragAndDropSource,
    DragAndDropSourceElement, DragAndDropSourceNode, DragAndDropState, DragAndDropTarget,
    DragAndDropTargetElement, DragAndDropTargetNode,
};
use focus::FocusTargetElement;
pub use focus::{FocusDirection, FocusRequestError, FocusRequester, FocusRequesterElement};
pub use graphics_layer::GlassMaterial;
pub(crate) use local::{
    ModifierLocalAncestorResolver, ModifierLocalSource, ModifierLocalToken, ResolvedModifierLocal,
};
use local::{ModifierLocalConsumerElement, ModifierLocalProviderElement};
pub use local::{ModifierLocalKey, ModifierLocalReadScope};
#[expect(unused_imports)]
pub use pointer_input::{AwaitPointerEventScope, PointerInputScope};
pub use rotary_input::RotaryInputModifierNode;
#[cfg(test)]
pub(crate) use scroll::lazy_scroll_semantics;
#[cfg(feature = "test-helpers")]
pub use scroll::{last_fling_velocity, reset_last_fling_velocity};
use semantics::SemanticsElement;
pub use semantics::{
    SemanticsRequester, SemanticsRequesterElement, collect_semantics_from_chain,
    collect_semantics_from_modifier, semantics_reach_of_chain,
};
pub use slices::{
    ModifierNodeSlices, ModifierNodeSlicesDebugStats, collect_modifier_slices,
    collect_modifier_slices_into, collect_slices_from_modifier,
};
pub use window_root::{
    WindowRootDescriptor, WindowRootElement, WindowRootEntry, WindowRootNode, WindowRootRegistry,
    is_window_root, nearest_window_root, nearest_window_roots, window_roots, window_roots_revision,
};

pub use crate::draw::{DrawCacheBuilder, DrawCommand};
use crate::modifier_nodes::ClipToBoundsElement;

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct InspectorProperty {
    pub name: &'static str,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModifierInspectorRecord {
    pub name: &'static str,
    pub properties: Vec<InspectorProperty>,
}

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
    if !inspector_metadata_enabled() {
        return InspectorMetadata::new(name, |_| {});
    }
    InspectorMetadata::new(name, recorder)
}

pub(crate) fn modifier_debug_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cranpose_core::env_flag!("COMPOSE_DEBUG_MODIFIERS")
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

fn inspector_metadata_enabled() -> bool {
    cfg!(test) || modifier_debug_enabled()
}

#[derive(Clone)]
enum ModifierKind {
    Empty,
    Single {
        elements: Rc<Vec<DynModifierElement>>,
        inspector: Rc<Vec<InspectorMetadata>>,
    },
}

const FINGERPRINT_KIND_EMPTY: u8 = 0;
const FINGERPRINT_KIND_SINGLE: u8 = 1;

const FINGERPRINT_EMPTY_STRICT_SEED: u64 = 0x243f_6a88_85a3_08d3;
const FINGERPRINT_EMPTY_STRUCTURAL_SEED: u64 = 0x1319_8a2e_0370_7344;
const FINGERPRINT_SINGLE_STRICT_SEED: u64 = 0xa409_3822_299f_31d0;
const FINGERPRINT_SINGLE_STRUCTURAL_SEED: u64 = 0x082e_fa98_ec4e_6c89;
const FINGERPRINT_SEQUENCE_MUL: u64 = 0x9e37_79b1_85eb_ca87;
const FINGERPRINT_STRICT_UPDATE_TAG: u64 = 0xdbe6_d5d5_fe4c_ce2f;
const FINGERPRINT_STRUCTURAL_DRAW_ONLY_TAG: u64 = 0x94d0_49bb_1331_11eb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModifierFingerprints {
    strict: u64,
    structural: u64,
}

#[inline]
fn mix_fingerprint_bits(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

#[inline]
fn fold_fingerprint(state: u64, value: u64) -> u64 {
    mix_fingerprint_bits(state ^ value.wrapping_add(FINGERPRINT_SEQUENCE_MUL))
        .wrapping_mul(FINGERPRINT_SEQUENCE_MUL)
}

#[inline]
fn empty_fingerprints() -> ModifierFingerprints {
    ModifierFingerprints {
        strict: fold_fingerprint(FINGERPRINT_EMPTY_STRICT_SEED, FINGERPRINT_KIND_EMPTY as u64),
        structural: fold_fingerprint(
            FINGERPRINT_EMPTY_STRUCTURAL_SEED,
            FINGERPRINT_KIND_EMPTY as u64,
        ),
    }
}

#[inline]
fn single_fingerprint_seed() -> ModifierFingerprints {
    let strict = fold_fingerprint(
        FINGERPRINT_SINGLE_STRICT_SEED,
        FINGERPRINT_KIND_SINGLE as u64,
    );
    let structural = fold_fingerprint(
        FINGERPRINT_SINGLE_STRUCTURAL_SEED,
        FINGERPRINT_KIND_SINGLE as u64,
    );
    ModifierFingerprints { strict, structural }
}

#[inline]
fn element_common_fingerprint(element: &DynModifierElement) -> u64 {
    let mut hasher = default::new();
    element.element_type().hash(&mut hasher);
    element.capabilities().bits().hash(&mut hasher);
    hasher.finish()
}

#[inline]
fn element_fingerprints(element: &DynModifierElement) -> ModifierFingerprints {
    let common = element_common_fingerprint(element);
    let requires_update = element.requires_update();
    let strict_payload = if requires_update {
        let element_ptr = Rc::as_ptr(element) as *const () as usize as u64;
        element_ptr ^ FINGERPRINT_STRICT_UPDATE_TAG
    } else {
        element.hash_code()
    };
    let strict = mix_fingerprint_bits(common ^ strict_payload);

    let is_draw_only = element.capabilities() == NodeCapabilities::DRAW;
    let structural_payload = if is_draw_only {
        FINGERPRINT_STRUCTURAL_DRAW_ONLY_TAG
    } else {
        element.hash_code()
    };
    let structural = mix_fingerprint_bits(common ^ structural_payload);

    ModifierFingerprints { strict, structural }
}

#[inline]
fn append_fingerprints(
    mut fingerprints: ModifierFingerprints,
    elements: &[DynModifierElement],
) -> ModifierFingerprints {
    for element in elements {
        let element_fingerprints = element_fingerprints(element);
        fingerprints.strict = fold_fingerprint(fingerprints.strict, element_fingerprints.strict);
        fingerprints.structural =
            fold_fingerprint(fingerprints.structural, element_fingerprints.structural);
    }
    fingerprints
}

fn single_fingerprints(elements: &[DynModifierElement]) -> ModifierFingerprints {
    append_fingerprints(single_fingerprint_seed(), elements)
}

pub struct ModifierElementIterator<'a> {
    inner: std::slice::Iter<'a, DynModifierElement>,
}

impl<'a> Iterator for ModifierElementIterator<'a> {
    type Item = &'a DynModifierElement;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ModifierElementIterator<'_> {}

pub(crate) struct ModifierInspectorIterator<'a> {
    inner: std::slice::Iter<'a, InspectorMetadata>,
}

impl<'a> Iterator for ModifierInspectorIterator<'a> {
    type Item = &'a InspectorMetadata;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ModifierInspectorIterator<'_> {}

/// A modifier chain that can be applied to composable elements.
///
/// Modifiers allow you to decorate or augment a composable. Common operations include:
/// - Adjusting layout (e.g., `padding`, `fill_max_size`)
/// - Adding behavior (e.g., `clickable`, `scrollable`)
/// - Drawing (e.g., `background`, `border`)
///
/// Modifiers are immutable and form a chain using the builder pattern.
/// The order of modifiers matters: previous modifiers wrap subsequent ones.
///
/// # Example
///
/// ```rust,ignore
/// Modifier::padding(16.0)     // Applied first (outer)
///     .background(Color::Red) // Applied second
///     .clickable(|| println!("Clicked")) // Applied last (inner)
/// ```
#[derive(Clone)]
pub struct Modifier {
    kind: ModifierKind,
    strict_fingerprint: u64,
    structural_fingerprint: u64,
    element_count: usize,
    provides_composition_locals: bool,
}

impl Default for Modifier {
    fn default() -> Self {
        let fingerprints = empty_fingerprints();
        Self {
            kind: ModifierKind::Empty,
            strict_fingerprint: fingerprints.strict,
            structural_fingerprint: fingerprints.structural,
            element_count: 0,
            provides_composition_locals: false,
        }
    }
}

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a modifier from a custom modifier node element.
    pub fn from_element<E>(element: E) -> Self
    where
        E: ModifierNodeElement,
    {
        Self::with_element(element)
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

    /// Says what a screen reader reads for this node, as one spec value.
    ///
    /// This is [`Modifier::semantics`] with a value in place of a closure.
    /// Compose has only the closure form, because Kotlin's receiver lambda
    /// makes `semantics { contentDescription = "Save" }` read well; Rust has
    /// none, so a spec value reads better, costs one chain element rather
    /// than one per property, and can be compared with another.
    ///
    /// Example:
    /// `Modifier::empty().semantics_spec(SemanticsSpec::new().content_description("Amount").error("needs a number"))`
    pub fn semantics_spec(self, spec: cranpose_foundation::SemanticsSpec) -> Self {
        self.semantics(move |config: &mut SemanticsConfiguration| config.merge(&spec))
    }
    pub fn semantics<F>(self, recorder: F) -> Self
    where
        F: Fn(&mut SemanticsConfiguration) + 'static,
    {
        let recorder: std::rc::Rc<dyn Fn(&mut SemanticsConfiguration)> = std::rc::Rc::new(recorder);
        let metadata = if inspector_metadata_enabled() {
            let mut preview = SemanticsConfiguration::default();
            recorder(&mut preview);
            let description = preview.content_description.clone();
            let state_description = preview.state_description.clone();
            let role = preview.role;
            let is_clickable = preview.is_activatable();
            let canvas_children = preview.canvas_children.len();
            inspector_metadata("semantics", move |info| {
                if let Some(desc) = &description {
                    info.add_property("contentDescription", desc.clone());
                }
                if let Some(state) = &state_description {
                    info.add_property("stateDescription", state.clone());
                }
                if let Some(role) = role {
                    info.add_property("role", format!("{role:?}"));
                }
                if is_clickable {
                    info.add_property("isClickable", "true");
                }
                if canvas_children > 0 {
                    info.add_property("canvasSemanticsChildren", canvas_children.to_string());
                }
            })
        } else {
            inspector_metadata("semantics", |_| {})
        };
        let element = SemanticsElement::new(recorder);
        let modifier =
            Modifier::from_parts(vec![modifier_element(element)]).with_inspector_metadata(metadata);
        self.then(modifier)
    }

    /// Tells a screen reader the value this control holds inside a range, so
    /// it reads the value and offers its own way to change it.
    ///
    /// This is Compose's `Modifier.progressSemantics(value, valueRange,
    /// steps)`. Without it a slider reads as text and a person who cannot see
    /// the screen has no way to move it.
    ///
    /// Example: `Modifier::empty().progress_semantics(0.35, 0.0, 1.0, 0)`
    pub fn progress_semantics(self, current: f32, start: f32, end: f32, steps: u32) -> Self {
        let info = ProgressBarRangeInfo::new(current, start, end, steps);
        self.semantics(move |config| config.progress = Some(info))
    }

    /// Marks this control as one that opens a list of choices, so a reader
    /// says "combo box" rather than "button" and knows to look for the
    /// choice it holds. Compose's `Role.DropdownList`.
    pub fn dropdown_list(self) -> Self {
        self.role(cranpose_foundation::SemanticsWidgetRole::DropdownList)
    }

    /// Marks this control as one that holds one value out of an ordered set,
    /// so a reader steps through them. Compose's `Role.ValuePicker`.
    pub fn value_picker(self) -> Self {
        self.role(cranpose_foundation::SemanticsWidgetRole::ValuePicker)
    }

    /// Says what this control does when a screen reader asks it to open, and
    /// marks it as closed right now: a reader offers "expand" and says the
    /// control is collapsed. Compose's
    /// `Modifier.semantics { expand { … } }`.
    pub fn expand(self, action: impl Fn() -> bool + 'static) -> Self {
        let action = cranpose_foundation::SemanticsExpand::new(action);
        self.semantics(move |config| config.expand = Some(action.clone()))
    }

    /// Says what a long press on this control does, so a screen reader can
    /// ask for it and read the label out first: "Remove receipt". Compose's
    /// `Modifier.semantics { onLongClick("Remove receipt") { … } }`, which
    /// `Modifier.combinedClickable(onLongClickLabel = …)` fills in for a
    /// control that takes a long press from a finger too.
    ///
    /// The label is asked for, not optional as in Compose: Android is the one
    /// platform of the four with a long press of its own, and the other three
    /// list the action by name, so a nameless one reads as nothing.
    pub fn on_long_click(
        self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        let label = label.into();
        let action = cranpose_foundation::SemanticsLongClick::new(action);
        self.semantics(move |config| {
            config.on_long_click_label = Some(label.clone());
            config.on_long_click = Some(action.clone());
        })
    }

    /// Says what this control does on VoiceOver's magic tap, the two finger
    /// double tap for the main action of a screen, and the verb phrase the
    /// other platforms list it under: "Take the photo". SwiftUI's
    /// `accessibilityAction(.magicTap)`.
    pub fn on_magic_tap(
        self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        let label = label.into();
        let action = cranpose_foundation::SemanticsMagicTap::new(action);
        self.semantics(move |config| {
            config.on_magic_tap_label = Some(label.clone());
            config.on_magic_tap = Some(action.clone());
        })
    }

    /// The short names a person says to Voice Control to reach this control,
    /// when the name a reader hears is too long to say. SwiftUI's
    /// `accessibilityInputLabels`.
    pub fn input_labels<S: Into<String>>(self, labels: impl IntoIterator<Item = S>) -> Self {
        let labels: Vec<String> = labels.into_iter().map(Into::into).collect();
        self.semantics(move |config| config.input_labels.clone_from(&labels))
    }

    /// The language of this control's text, as a BCP 47 tag such as "de" or
    /// "pt-BR", so a reader picks the right voice. SwiftUI's
    /// `accessibilityLanguage`, ARIA's `lang`.
    pub fn language(self, tag: impl Into<String>) -> Self {
        let tag = tag.into();
        self.semantics(move |config| config.language = Some(tag.clone()))
    }

    /// Says what this control does when a screen reader asks it to close, and
    /// marks it as open right now. Compose's
    /// `Modifier.semantics { collapse { … } }`.
    pub fn collapse(self, action: impl Fn() -> bool + 'static) -> Self {
        let action = cranpose_foundation::SemanticsExpand::new(action);
        self.semantics(move |config| config.collapse = Some(action.clone()))
    }

    /// Says what this control does when a screen reader asks to send it away:
    /// a row a sighted person swipes off, a sheet a sighted person taps
    /// outside of. A reader that cannot make the gesture gets the same way
    /// out. Compose's `Modifier.semantics { dismiss { … } }`.
    pub fn dismiss(self, action: impl Fn() -> bool + 'static) -> Self {
        let action = cranpose_foundation::SemanticsDismiss::new(action);
        self.semantics(move |config| config.dismiss = Some(action.clone()))
    }

    /// Says what this list does when a screen reader asks for the row at an
    /// index, so a reader reaches row 300 of a long list at once instead of
    /// paging to it. The index counts rows from zero and the answer says
    /// whether the list moved. `LazyColumn` and `LazyRow` declare it on their
    /// own. Compose's `Modifier.semantics { scrollToIndex { … } }`.
    pub fn scroll_to_index(self, action: impl Fn(usize) -> bool + 'static) -> Self {
        let action = cranpose_foundation::SemanticsScrollToIndex::new(action);
        self.semantics(move |config| config.scroll_to_index = Some(action.clone()))
    }

    /// Moves this node in the order a screen reader visits the nodes beside
    /// it: a smaller number comes first, and a node left alone keeps the
    /// order the app laid it out in. A search field drawn last but meant to
    /// be read first takes a negative number. Compose's
    /// `Modifier.semantics { traversalIndex = -1f }`.
    pub fn traversal_index(self, index: f32) -> Self {
        self.semantics(move |config| config.traversal_index = index)
    }

    /// Marks a field as one that holds a secret, so no screen reader reads
    /// its text out: a reader hears the name the app gave the field, and
    /// "password" in place of the text. Compose's
    /// `Modifier.semantics { password() }`.
    pub fn password(self) -> Self {
        self.semantics(|config| config.password = true)
    }

    /// Says why the control's content is wrong, so a screen reader reads
    /// "invalid, the amount needs a number" after the control's state.
    /// Compose's `Modifier.semantics { error("...") }`.
    pub fn error(self, message: impl Into<String>) -> Self {
        let message = message.into();
        self.semantics(move |config| config.error = Some(message.clone()))
    }

    /// Names the screen or pane this node is the root of, so a screen reader
    /// hears where it is when the app moves on: "Library" as the library
    /// opens. Compose's `Modifier.semantics { paneTitle = "..." }`.
    pub fn pane_title(self, title: impl Into<String>) -> Self {
        let title = title.into();
        self.semantics(move |config| config.pane_title = Some(title.clone()))
    }

    /// Makes the selectable controls under this node one group, so a screen
    /// reader says which of how many a tab or a radio button is: "Library,
    /// tab, 2 of 5". `LiquidTabBar` declares it on its own. Compose's
    /// `Modifier.selectableGroup()`.
    pub fn selectable_group(self) -> Self {
        self.semantics(|config| config.selectable_group = true)
    }

    /// Makes a screen reader take this node and the text under it as one
    /// stop, the way it does for a button: a row whose name, count and price
    /// belong together reads as "Milk, 2, 3.40" and not as three stops.
    /// Compose's `Modifier.semantics(mergeDescendants = true) {}`.
    pub fn merge_descendants(self) -> Self {
        self.semantics(|config| config.merge_descendants = true)
    }

    /// Takes this node and everything under it out of what a screen reader
    /// sees: a decorative image, or a placeholder drawn under a field that
    /// carries the same words as its name. Compose's
    /// `semantics { hideFromAccessibility() }`.
    pub fn hide_from_accessibility(self) -> Self {
        self.semantics(|config| config.hidden = true)
    }

    /// Marks this component as a heading, so a screen reader lists it among
    /// the headings of the screen and a person can jump between them.
    ///
    /// This is Compose's `Modifier.semantics { heading() }`.
    pub fn heading(self) -> Self {
        self.role(cranpose_foundation::SemanticsWidgetRole::Header)
    }

    /// Tells a screen reader what kind of control this is, when the widget
    /// does not say so on its own.
    ///
    /// This is Compose's `Modifier.semantics { role = Role.Button }`.
    pub fn role(self, role: cranpose_foundation::SemanticsWidgetRole) -> Self {
        self.semantics(move |config| config.role = Some(role))
    }

    /// Makes a screen reader read this component's text out whenever it
    /// changes, without the reader's cursor on it: a status line, a toast, a
    /// count that moves.
    ///
    /// This is Compose's `Modifier.semantics { liveRegion = LiveRegionMode.Polite }`.
    pub fn live_region(self, mode: cranpose_foundation::LiveRegionMode) -> Self {
        self.semantics(move |config| config.live_region = Some(mode))
    }

    /// Gives this component the text a screen reader reads for it, for a
    /// drawing, an icon or a control with no text of its own.
    ///
    /// This is Compose's `Modifier.semantics { contentDescription = "..." }`.
    pub fn content_description(self, description: impl Into<String>) -> Self {
        let description = description.into();
        self.semantics(move |config| config.content_description = Some(description.clone()))
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

    /// Binds a [`FocusRequester`] to this node, so an app can move focus onto
    /// it imperatively with [`FocusRequester::request_focus`].
    ///
    /// Pair it with [`focus_target`](Self::focus_target) (or
    /// [`on_focus_changed`](Self::on_focus_changed)) on the same node —
    /// `request_focus` moves whichever focus targets are attached there.
    pub fn focus_requester(self, requester: &FocusRequester) -> Self {
        let element = FocusRequesterElement::new(requester.clone());
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
    }

    /// Binds a [`SemanticsRequester`] to this node, so an app can mark the
    /// node's semantics for re-collection without recomposing or laying out.
    ///
    /// Pair it with [`semantics`](Self::semantics) on the same node when the
    /// recorder reads state the composition does not observe — app state behind
    /// a `RefCell`, a game's own model — which is the case a recorder cannot
    /// signal for itself.
    pub fn semantics_requester(self, requester: &SemanticsRequester) -> Self {
        let element = SemanticsRequesterElement::new(requester.clone());
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
    /// ```text
    /// Modifier::empty()
    ///     .padding(8.0)
    ///     .background(Color(1.0, 0.0, 0.0, 1.0))
    ///     .debug_chain("MyWidget")
    /// ```
    pub fn debug_chain(self, tag: &'static str) -> Self {
        use cranpose_foundation::{ModifierNode, ModifierNodeContext, NodeCapabilities, NodeState};

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

        impl cranpose_foundation::DelegatableNode for DebugChainNode {
            fn node_state(&self) -> &NodeState {
                &self.state
            }
        }

        let element = DebugChainElement { tag };
        let modifier = Modifier::from_parts(vec![modifier_element(element)]);
        self.then(modifier)
            .with_inspector_metadata(inspector_metadata("debugChain", move |info| {
                info.add_property("tag", tag);
            }))
    }

    /// Concatenates this modifier with another.
    ///
    /// Eagerly concatenates both element vectors into a single flat `Single`
    /// variant, avoiding recursive Rc tree overhead on drop and comparison.
    pub fn then(&self, next: Modifier) -> Modifier {
        if self.is_trivially_empty() {
            return next;
        }
        if next.is_trivially_empty() {
            return self.clone();
        }

        let Some((self_elements, self_inspector)) = self.single_parts() else {
            return next;
        };
        let Some((next_elements, next_inspector)) = next.single_parts() else {
            return self.clone();
        };

        let mut merged_elements = Vec::with_capacity(self_elements.len() + next_elements.len());
        merged_elements.extend_from_slice(self_elements);
        merged_elements.extend_from_slice(next_elements);

        let mut merged_inspector = Vec::with_capacity(self_inspector.len() + next_inspector.len());
        merged_inspector.extend_from_slice(self_inspector);
        merged_inspector.extend_from_slice(next_inspector);

        let fingerprints = append_fingerprints(
            ModifierFingerprints {
                strict: self.strict_fingerprint,
                structural: self.structural_fingerprint,
            },
            next_elements,
        );
        Modifier {
            kind: ModifierKind::Single {
                elements: Rc::new(merged_elements),
                inspector: Rc::new(merged_inspector),
            },
            strict_fingerprint: fingerprints.strict,
            structural_fingerprint: fingerprints.structural,
            element_count: self.element_count + next.element_count,
            provides_composition_locals: self.provides_composition_locals
                || next.provides_composition_locals,
        }
    }

    pub(crate) fn iter_elements(&self) -> ModifierElementIterator<'_> {
        match &self.kind {
            ModifierKind::Empty => ModifierElementIterator { inner: [].iter() },
            ModifierKind::Single { elements, .. } => ModifierElementIterator {
                inner: elements.iter(),
            },
        }
    }

    pub(crate) fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        if !self.provides_composition_locals {
            return Vec::new();
        }
        self.iter_elements()
            .flat_map(|element| element.provided_composition_locals())
            .collect()
    }

    pub(crate) fn iter_inspector_metadata(&self) -> ModifierInspectorIterator<'_> {
        match &self.kind {
            ModifierKind::Empty => ModifierInspectorIterator { inner: [].iter() },
            ModifierKind::Single { inspector, .. } => ModifierInspectorIterator {
                inner: inspector.iter(),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn elements(&self) -> Vec<DynModifierElement> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { elements, .. } => elements.as_ref().clone(),
        }
    }

    pub(crate) fn inspector_metadata(&self) -> Vec<InspectorMetadata> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { inspector, .. } => inspector.as_ref().clone(),
        }
    }

    pub(crate) fn rehouse_for_live_compaction(&self) -> Self {
        match &self.kind {
            ModifierKind::Empty => Self::default(),
            ModifierKind::Single {
                elements,
                inspector,
            } => Self {
                kind: ModifierKind::Single {
                    elements: Rc::new(elements.iter().cloned().collect()),
                    inspector: Rc::new(inspector.as_ref().clone()),
                },
                strict_fingerprint: self.strict_fingerprint,
                structural_fingerprint: self.structural_fingerprint,
                element_count: self.element_count,
                provides_composition_locals: self.provides_composition_locals,
            },
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
            .map(InspectorMetadata::to_record)
            .collect()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(self);
        handle.resolved_modifiers()
    }

    /// A modifier of the one `element`. Platform crates build their own
    /// modifiers on it, the way [`Modifier::window_root`] is built.
    pub fn with_element<E>(element: E) -> Self
    where
        E: ModifierNodeElement,
    {
        let dyn_element = modifier_element(element);
        Self::from_parts(vec![dyn_element])
    }

    pub(crate) fn from_parts(elements: Vec<DynModifierElement>) -> Self {
        if elements.is_empty() {
            Self::default()
        } else {
            let element_count = elements.len();
            let provides_composition_locals = elements
                .iter()
                .any(|element| element.provides_composition_locals());
            let fingerprints = single_fingerprints(elements.as_slice());
            Self {
                kind: ModifierKind::Single {
                    elements: Rc::new(elements),
                    inspector: Rc::new(Vec::new()),
                },
                strict_fingerprint: fingerprints.strict,
                structural_fingerprint: fingerprints.structural,
                element_count,
                provides_composition_locals,
            }
        }
    }

    fn is_trivially_empty(&self) -> bool {
        matches!(self.kind, ModifierKind::Empty)
    }

    fn single_parts(&self) -> Option<(&[DynModifierElement], &[InspectorMetadata])> {
        match &self.kind {
            ModifierKind::Empty => None,
            ModifierKind::Single {
                elements,
                inspector,
            } => Some((elements.as_slice(), inspector.as_slice())),
        }
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
                    strict_fingerprint: self.strict_fingerprint,
                    structural_fingerprint: self.structural_fingerprint,
                    element_count: self.element_count,
                    provides_composition_locals: self.provides_composition_locals,
                }
            }
        }
    }

    /// Checks whether two modifiers are structurally equivalent for layout decisions.
    ///
    /// This ignores identity-sensitive modifier elements (e.g., draw closures) so
    /// draw-only updates do not force measure/layout invalidation.
    pub fn structural_eq(&self, other: &Self) -> bool {
        self.eq_internal(other, false)
    }

    fn eq_internal(&self, other: &Self, consider_always_update: bool) -> bool {
        if self.element_count != other.element_count {
            return false;
        }
        if consider_always_update {
            if self.strict_fingerprint != other.strict_fingerprint {
                return false;
            }
        } else if self.structural_fingerprint != other.structural_fingerprint {
            return false;
        }

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
                if Rc::ptr_eq(e1, e2) {
                    return true;
                }

                if e1.len() != e2.len() {
                    return false;
                }

                for (a, b) in e1.iter().zip(e2.iter()) {
                    if !consider_always_update
                        && a.element_type() == b.element_type()
                        && a.capabilities() == NodeCapabilities::DRAW
                        && b.capabilities() == NodeCapabilities::DRAW
                    {
                        continue;
                    }

                    if consider_always_update && (a.requires_update() || b.requires_update()) {
                        if !Rc::ptr_eq(a, b) {
                            return false;
                        }
                        continue;
                    }

                    if !a.equals_element(&**b) {
                        return false;
                    }
                }

                true
            }
            _ => false,
        }
    }
}

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        self.eq_internal(other, true)
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

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ResolvedModifiers {
    padding: EdgeInsets,
    layout: LayoutProperties,
    offset: Point,
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
}

#[cfg(test)]
#[path = "tests/modifier_tests.rs"]
mod tests;
