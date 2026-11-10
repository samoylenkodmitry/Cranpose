// A modifier element that is a value alone: a chain keeps it as words in
// the element's entry, with no node.

use std::{
    any::{Any, TypeId, type_name},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    rc::Rc,
};

use cranpose_core::{ProvidedValue, hash::default};
use cranpose_ui_layout::{ConstraintsHold, LayoutModifierMeasureResult, WrappedHold};

use super::super::{
    AnyModifierElement, Constraints, DynModifierElement, ElementMaker, InvalidationKind,
    Measurable, ModifierNodeContext, NodeCapabilities, Size,
};

/// The most words a plain element's value takes: with its type's table
/// they fit in the room of a chain entry, and a coordinator keeps a copy
/// of them for every layout modifier of a node.
pub const PLAIN_WORDS_MAX: usize = 5;

/// A modifier element that is a value and nothing else: it keeps no state
/// between frames, hears of no attach, detach or reset and delegates to no
/// node. A chain keeps its value as [`Self::WORDS`] words and makes no node
/// for it; a reader takes the value back with [`Self::from_words`].
pub trait PlainElement: fmt::Debug + Hash + PartialEq + Sized + 'static {
    /// How many words the value takes: at most [`PLAIN_WORDS_MAX`].
    const WORDS: usize;

    /// What the element takes part in: layout, draw or both.
    const CAPABILITIES: NodeCapabilities;

    /// The one pipeline stage a changed value invalidates, or `None` to
    /// invalidate every stage of [`Self::CAPABILITIES`].
    const UPDATE_INVALIDATION: Option<InvalidationKind> = None;

    /// Writes the value into `words`, which holds [`Self::WORDS`] words.
    fn to_words(&self, words: &mut [u32]);

    /// The value that [`Self::to_words`] wrote into `words`.
    fn from_words(words: &[u32]) -> Self;

    /// How the element measures the content it wraps, for an element with
    /// [`NodeCapabilities::LAYOUT`]: `Some(&PlainLayoutFns::of::<Self>())`
    /// for a [`PlainLayout`]. `None`, the default, passes the content's
    /// measure through.
    const LAYOUT: Option<&'static PlainLayoutFns> = None;

    /// Human readable name surfaced to inspector tooling.
    fn inspector_name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Records inspector properties for tooling.
    fn inspector_properties(&self, _inspector: &mut dyn FnMut(&'static str, String)) {}
}

/// A plain element that measures the content it wraps, as a
/// [`super::super::LayoutModifierNode`] does, from its value alone.
pub trait PlainLayout: PlainElement {
    /// Measures the wrapped content under `constraints` and returns the size
    /// the element takes and where it places the content.
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult;

    /// The element as a coordinator keeps it for measures at `density`: an
    /// element that rounds its values to device pixels may round them here
    /// once, and measure from the rounded values. The default is the
    /// element itself.
    fn prepared(self, _density: f32) -> Self {
        self
    }

    /// The incoming constraints the last measure, which gave `size` under
    /// `constraints`, holds for; see
    /// [`super::super::LayoutModifierNode::measure_hold`].
    fn measure_hold(
        &self,
        _density: f32,
        _constraints: Constraints,
        _size: Size,
        _wrapped: WrappedHold,
    ) -> Option<ConstraintsHold> {
        None
    }

    /// The minimum intrinsic width: the wrapped content's by default.
    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        measurable.min_intrinsic_width(height)
    }

    /// The maximum intrinsic width: the wrapped content's by default.
    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        measurable.max_intrinsic_width(height)
    }

    /// The minimum intrinsic height: the wrapped content's by default.
    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        measurable.min_intrinsic_height(width)
    }

    /// The maximum intrinsic height: the wrapped content's by default.
    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        measurable.max_intrinsic_height(width)
    }
}

/// Which intrinsic size a measure asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlainIntrinsic {
    /// The minimum width at a height.
    MinWidth(f32),
    /// The maximum width at a height.
    MaxWidth(f32),
    /// The minimum height at a width.
    MinHeight(f32),
    /// The maximum height at a width.
    MaxHeight(f32),
}

type MeasureFn = fn(
    &[u32],
    &mut dyn ModifierNodeContext,
    &dyn Measurable,
    Constraints,
) -> LayoutModifierMeasureResult;

/// How a plain layout element measures, read from its words: what a
/// coordinator keeps of the element's type.
pub struct PlainLayoutFns {
    prepare: fn(&[u32], f32, &mut [u32]),
    measure: MeasureFn,
    measure_hold: fn(&[u32], f32, Constraints, Size, WrappedHold) -> Option<ConstraintsHold>,
    intrinsic: fn(&[u32], &dyn Measurable, PlainIntrinsic, f32) -> f32,
}

impl fmt::Debug for PlainLayoutFns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlainLayoutFns").finish_non_exhaustive()
    }
}

impl PlainLayoutFns {
    /// The table of the plain layout element `E`.
    pub const fn of<E: PlainLayout>() -> Self {
        Self {
            prepare: |words, density, prepared| {
                E::from_words(words)
                    .prepared(density)
                    .to_words(&mut prepared[..E::WORDS]);
            },
            measure: |words, context, measurable, constraints| {
                E::from_words(words).measure(context, measurable, constraints)
            },
            measure_hold: |words, density, constraints, size, wrapped| {
                E::from_words(words).measure_hold(density, constraints, size, wrapped)
            },
            intrinsic: |words, measurable, kind, density| {
                let element = E::from_words(words);
                match kind {
                    PlainIntrinsic::MinWidth(height) => {
                        element.min_intrinsic_width(measurable, height, density)
                    }
                    PlainIntrinsic::MaxWidth(height) => {
                        element.max_intrinsic_width(measurable, height, density)
                    }
                    PlainIntrinsic::MinHeight(width) => {
                        element.min_intrinsic_height(measurable, width, density)
                    }
                    PlainIntrinsic::MaxHeight(width) => {
                        element.max_intrinsic_height(measurable, width, density)
                    }
                }
            },
        }
    }

    /// Writes into `prepared` the words of the element `words` hold as a
    /// coordinator keeps it for measures at `density`; see
    /// [`PlainLayout::prepared`]. The other calls take prepared words.
    pub fn prepare(&self, words: &[u32], density: f32, prepared: &mut [u32; PLAIN_WORDS_MAX]) {
        (self.prepare)(words, density, prepared);
    }

    /// Measures as the element `words` hold does.
    #[inline]
    pub fn measure(
        &self,
        words: &[u32],
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        (self.measure)(words, context, measurable, constraints)
    }

    /// The hold of the last measure of the element `words` hold.
    pub fn measure_hold(
        &self,
        words: &[u32],
        density: f32,
        constraints: Constraints,
        size: Size,
        wrapped: WrappedHold,
    ) -> Option<ConstraintsHold> {
        (self.measure_hold)(words, density, constraints, size, wrapped)
    }

    /// The intrinsic size `kind` names of the element `words` hold.
    pub fn intrinsic(
        &self,
        words: &[u32],
        measurable: &dyn Measurable,
        kind: PlainIntrinsic,
        density: f32,
    ) -> f32 {
        (self.intrinsic)(words, measurable, kind, density)
    }
}

/// What a chain keeps of a plain element's type beside its words: how many
/// they are, what the element takes part in, and how to read them.
pub struct PlainFns {
    element_type: fn() -> TypeId,
    words: u8,
    capabilities: NodeCapabilities,
    layout: Option<&'static PlainLayoutFns>,
    type_name: fn() -> &'static str,
    hash_code: fn(&[u32]) -> u64,
    element: fn(&[u32]) -> DynModifierElement,
}

impl fmt::Debug for PlainFns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlainFns").finish_non_exhaustive()
    }
}

impl PlainFns {
    /// The type of the element the words hold.
    pub fn element_type(&self) -> TypeId {
        (self.element_type)()
    }

    /// How many words the element's value takes.
    pub fn words(&self) -> usize {
        usize::from(self.words)
    }

    /// What the element takes part in.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.capabilities
    }

    /// How the element measures, when it does.
    pub fn layout(&self) -> Option<&'static PlainLayoutFns> {
        self.layout
    }

    /// The name of the element's type.
    pub fn type_name(&self) -> &'static str {
        (self.type_name)()
    }

    pub(in super::super) fn hash_code(&self, words: &[u32]) -> u64 {
        (self.hash_code)(words)
    }

    pub(in super::super) fn element(&self, words: &[u32]) -> DynModifierElement {
        (self.element)(words)
    }

    /// The element `words` hold, when it is an `E`.
    #[inline]
    pub fn read<E: PlainElement>(&self, words: &[u32]) -> Option<E> {
        (self.element_type() == TypeId::of::<E>()).then(|| E::from_words(words))
    }
}

struct FnsOf<E>(PhantomData<fn() -> E>);

impl<E: PlainElement> FnsOf<E> {
    const WORDS: u8 = {
        assert!(E::WORDS <= PLAIN_WORDS_MAX, "a plain element's words fit");
        E::WORDS as u8
    };

    const FNS: PlainFns = PlainFns {
        element_type: TypeId::of::<E>,
        words: Self::WORDS,
        capabilities: E::CAPABILITIES,
        layout: E::LAYOUT,
        type_name: type_name::<E>,
        hash_code: |words| hash_of(&E::from_words(words)),
        element: |words| plain_element(E::from_words(words)),
    };
}

fn hash_of<E: Hash>(element: &E) -> u64 {
    let mut hasher = default::new();
    element.hash(&mut hasher);
    hasher.finish()
}

/// A plain element as a chain reads it: its type's table, and its value as
/// words.
pub trait PlainValue {
    /// The table of the element's type.
    fn fns(&self) -> &'static PlainFns;

    /// Writes the value into `words`, as many as the table tells.
    fn write_words(&self, words: &mut [u32]);

    /// Whether `words`, written by an element of this type, hold a value
    /// equal to this one, as the element type compares.
    fn equals_words(&self, words: &[u32]) -> bool;
}

struct TypedPlainElement<E: PlainElement>(E);

impl<E: PlainElement> fmt::Debug for TypedPlainElement<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<E: PlainElement> PlainValue for TypedPlainElement<E> {
    fn fns(&self) -> &'static PlainFns {
        &FnsOf::<E>::FNS
    }

    fn write_words(&self, words: &mut [u32]) {
        self.0.to_words(words);
    }

    fn equals_words(&self, words: &[u32]) -> bool {
        E::from_words(words) == self.0
    }
}

impl<E: PlainElement> AnyModifierElement for TypedPlainElement<E> {
    fn element_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn maker(&self) -> ElementMaker<'_> {
        ElementMaker::Plain(self)
    }

    fn key(&self) -> Option<u64> {
        None
    }

    fn capabilities(&self) -> NodeCapabilities {
        E::CAPABILITIES
    }

    fn hash_code(&self) -> u64 {
        hash_of(&self.0)
    }

    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|typed| typed.0 == self.0)
    }

    fn inspector_name(&self) -> &'static str {
        self.0.inspector_name()
    }

    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String)) {
        self.0.inspector_properties(visitor);
    }

    fn requires_update(&self) -> bool {
        false
    }

    fn auto_invalidates_on_update(&self) -> bool {
        true
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        E::UPDATE_INVALIDATION
    }

    fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals_plain(&self, fns: &PlainFns, words: &[u32]) -> bool {
        fns.element_type() == TypeId::of::<E>() && E::from_words(words) == self.0
    }
}

/// The type-erased element of a plain `element`.
pub fn plain_element<E: PlainElement>(element: E) -> DynModifierElement {
    Rc::new(TypedPlainElement(element))
}
