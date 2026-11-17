// The elements a modifier holds, how two modifiers join, and the inspector
// metadata that travels with them.

use std::rc::Rc;

use cranpose_core::ProvidedValue;
use cranpose_foundation::DynModifierElement;
use smallvec::SmallVec;

use super::{InspectorMetadata, Modifier};

fn inspector_slice(inspector: &Option<Rc<Vec<InspectorMetadata>>>) -> &[InspectorMetadata] {
    inspector.as_deref().map_or(&[], Vec::as_slice)
}

/// The inspector metadata of two modifiers joined, first `first`'s, or
/// `None` when neither has any.
fn merged_inspector(
    first: &[InspectorMetadata],
    second: &[InspectorMetadata],
) -> Option<Rc<Vec<InspectorMetadata>>> {
    if first.is_empty() && second.is_empty() {
        return None;
    }
    Some(Rc::new(first.iter().chain(second).cloned().collect()))
}

/// A modifier's elements: a chain of up to four lives inline in its one
/// shared allocation.
pub(super) type ModifierElements = SmallVec<[DynModifierElement; 4]>;

/// The elements of a non-empty modifier. A lone element, as each link of a
/// builder chain starts out, is held as it is; joining it to more moves the
/// chain into one shared allocation, which the chain's later links extend
/// in place while it is the only owner.
#[derive(Clone)]
pub(super) enum Elements {
    One(DynModifierElement),
    Many(Rc<ModifierElements>),
}

impl Elements {
    /// These elements followed by `more`. Elements move into the joined
    /// chain; only those another modifier also holds are copied.
    fn joined(self, more: Self) -> Self {
        let mut chain = match self {
            Self::One(first) => {
                let mut chain = ModifierElements::new();
                chain.push(first);
                Rc::new(chain)
            }
            Self::Many(chain) => chain,
        };
        match Rc::get_mut(&mut chain) {
            Some(owned) => more.move_into(owned),
            None => {
                let mut copy: ModifierElements = chain.iter().cloned().collect();
                more.move_into(&mut copy);
                chain = Rc::new(copy);
            }
        }
        Self::Many(chain)
    }

    fn move_into(self, chain: &mut ModifierElements) {
        match self {
            Self::One(element) => chain.push(element),
            Self::Many(shared) => match Rc::try_unwrap(shared) {
                Ok(owned) => chain.extend(owned),
                Err(shared) => chain.extend(shared.iter().cloned()),
            },
        }
    }

    /// Whether both are the same storage, so equal without comparing.
    pub(super) fn shares_storage(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::One(a), Self::One(b)) => Rc::ptr_eq(a, b),
            (Self::Many(a), Self::Many(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl std::ops::Deref for Elements {
    type Target = [DynModifierElement];

    fn deref(&self) -> &[DynModifierElement] {
        match self {
            Self::One(element) => std::slice::from_ref(element),
            Self::Many(elements) => elements.as_slice(),
        }
    }
}

#[derive(Clone)]
pub(super) enum ModifierKind {
    Empty,
    Single {
        elements: Elements,
        /// `None` when no element records inspector metadata, which is
        /// always outside tests and modifier debugging: no allocation.
        inspector: Option<Rc<Vec<InspectorMetadata>>>,
    },
}

pub(crate) struct ModifierElementIterator<'a> {
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

impl Modifier {
    /// This modifier followed by `next`.
    ///
    /// It takes `self`: a modifier built link by link owns its elements
    /// alone, so each link joins them in place instead of copying the chain.
    /// Clone a modifier first to keep using it; the join then copies it.
    pub fn then(self, next: Self) -> Self {
        let ModifierKind::Single {
            elements,
            inspector,
        } = self.kind
        else {
            return next;
        };
        let ModifierKind::Single {
            elements: next_elements,
            inspector: next_inspector,
        } = next.kind
        else {
            return Self {
                kind: ModifierKind::Single {
                    elements,
                    inspector,
                },
                ..self
            };
        };

        let inspector = if next_inspector.is_none() {
            inspector
        } else {
            merged_inspector(
                inspector_slice(&inspector),
                inspector_slice(&next_inspector),
            )
        };
        Self {
            kind: ModifierKind::Single {
                elements: elements.joined(next_elements),
                inspector,
            },
            provides_composition_locals: self.provides_composition_locals
                || next.provides_composition_locals,
        }
    }

    /// How many elements the modifier holds.
    pub(crate) fn element_count(&self) -> usize {
        match &self.kind {
            ModifierKind::Empty => 0,
            ModifierKind::Single { elements, .. } => elements.len(),
        }
    }

    pub(crate) fn iter_elements(&self) -> ModifierElementIterator<'_> {
        ModifierElementIterator {
            inner: self.as_elements().iter(),
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
                inner: inspector_slice(inspector).iter(),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn elements(&self) -> Vec<DynModifierElement> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { elements, .. } => elements.to_vec(),
        }
    }

    pub(crate) fn inspector_metadata(&self) -> Vec<InspectorMetadata> {
        match &self.kind {
            ModifierKind::Empty => Vec::new(),
            ModifierKind::Single { inspector, .. } => {
                inspector.as_deref().cloned().unwrap_or_default()
            }
        }
    }

    /// A modifier of the one `element`.
    pub(crate) fn from_dyn_element(element: DynModifierElement) -> Self {
        Self {
            provides_composition_locals: element.provides_composition_locals(),
            kind: ModifierKind::Single {
                elements: Elements::One(element),
                inspector: None,
            },
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
                let mut list = inspector.as_deref().cloned().unwrap_or_default();
                list.push(metadata);
                let new_inspector = Rc::new(list);
                Self {
                    kind: ModifierKind::Single {
                        elements,
                        inspector: Some(new_inspector),
                    },
                    provides_composition_locals: self.provides_composition_locals,
                }
            }
        }
    }

    pub(super) fn as_elements(&self) -> &[DynModifierElement] {
        match &self.kind {
            ModifierKind::Empty => &[],
            ModifierKind::Single { elements, .. } => elements,
        }
    }
}
