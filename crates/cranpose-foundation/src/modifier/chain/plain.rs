// A modifier element that is a value alone: a chain keeps it as words in
// its payload, with no node.

use std::{
    any::{Any, TypeId, type_name},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    rc::Rc,
};

use cranpose_core::{ProvidedValue, hash::default};

use super::super::{
    AnyModifierElement, DynModifierElement, ElementMaker, InvalidationKind, NodeCapabilities,
};

/// The most words a plain element's value takes in a chain's payload.
pub const PLAIN_WORDS_MAX: usize = 8;

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

    /// Human readable name surfaced to inspector tooling.
    fn inspector_name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Records inspector properties for tooling.
    fn inspector_properties(&self, _inspector: &mut dyn FnMut(&'static str, String)) {}
}

/// What a chain keeps of a plain element's type beside its words: how many
/// they are, what the element takes part in, and how to read them.
pub struct PlainFns {
    element_type: fn() -> TypeId,
    words: u8,
    capabilities: NodeCapabilities,
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
