//! The children a measure policy measures, as Compose's `List<Measurable>`.

use std::{fmt, iter::FusedIterator, ops::Index};

use crate::Measurable;

/// A list of children that [`Measurables`] reads: its length and each child
/// by its index. A `Vec`, an array or a slice of any [`Measurable`] type is
/// such a list, and so is a list of `&dyn Measurable` for children of
/// several types.
pub trait MeasurableList {
    /// How many children the list holds.
    fn len(&self) -> usize;

    /// Whether the list holds no child.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The child at `index`; `None` past the end.
    fn get(&self, index: usize) -> Option<&dyn Measurable>;
}

impl<M: Measurable> MeasurableList for Vec<M> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn get(&self, index: usize) -> Option<&dyn Measurable> {
        self.as_slice()
            .get(index)
            .map(|child| child as &dyn Measurable)
    }
}

impl<M: Measurable, const N: usize> MeasurableList for [M; N] {
    fn len(&self) -> usize {
        N
    }

    fn get(&self, index: usize) -> Option<&dyn Measurable> {
        self.as_slice()
            .get(index)
            .map(|child| child as &dyn Measurable)
    }
}

impl<M: Measurable> MeasurableList for &[M] {
    fn len(&self) -> usize {
        <[M]>::len(self)
    }

    fn get(&self, index: usize) -> Option<&dyn Measurable> {
        <[M]>::get(self, index).map(|child| child as &dyn Measurable)
    }
}

// The list of a layout with no children.
struct NoChildren;

impl MeasurableList for NoChildren {
    fn len(&self) -> usize {
        0
    }

    fn get(&self, _index: usize) -> Option<&dyn Measurable> {
        None
    }
}

/// The children a [`MeasurePolicy`](crate::MeasurePolicy) measures and
/// places, in their order: Compose's `List<Measurable>`. It is a view of a
/// list its layout owns, so it copies freely. Index it or iterate it for each
/// child as a [`Measurable`]; an index past the end panics, as it does on a
/// slice.
///
/// ```
/// use cranpose_ui_layout::{Measurable, Measurables};
///
/// fn widest(children: Measurables<'_>, height: f32) -> f32 {
///     children
///         .iter()
///         .map(|child| child.max_intrinsic_width(height))
///         .fold(0.0, f32::max)
/// }
///
/// let none: Vec<&dyn Measurable> = Vec::new();
/// assert_eq!(widest(Measurables::new(&none), 10.0), 0.0);
/// ```
#[derive(Clone, Copy)]
pub struct Measurables<'a> {
    list: &'a dyn MeasurableList,
    // The list's length, read once: a policy reads it often.
    len: usize,
}

impl<'a> Measurables<'a> {
    /// A view of the children `list` holds.
    pub fn new(list: &'a dyn MeasurableList) -> Self {
        Self {
            list,
            len: list.len(),
        }
    }

    /// No children, as a layout without content has.
    pub fn empty() -> Self {
        Self {
            list: &NoChildren,
            len: 0,
        }
    }

    /// How many children there are.
    pub fn len(self) -> usize {
        self.len
    }

    /// Whether there is no child.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    /// The child at `index`; `None` past the end.
    pub fn get(self, index: usize) -> Option<&'a dyn Measurable> {
        self.list.get(index)
    }

    /// The first child; `None` without children.
    pub fn first(self) -> Option<&'a dyn Measurable> {
        self.list.get(0)
    }

    /// The last child; `None` without children.
    pub fn last(self) -> Option<&'a dyn Measurable> {
        self.len
            .checked_sub(1)
            .and_then(|index| self.list.get(index))
    }

    /// The children in their order.
    pub fn iter(self) -> MeasurablesIter<'a> {
        MeasurablesIter {
            list: self.list,
            front: 0,
            back: self.len,
        }
    }
}

impl<'a> Index<usize> for Measurables<'a> {
    type Output = dyn Measurable + 'a;

    fn index(&self, index: usize) -> &Self::Output {
        let len = self.len();
        self.get(index)
            .unwrap_or_else(|| panic!("index {index} is past the end of {len} measurables"))
    }
}

impl<'a> IntoIterator for Measurables<'a> {
    type Item = &'a dyn Measurable;
    type IntoIter = MeasurablesIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl fmt::Debug for Measurables<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Measurables")
            .field("len", &self.len())
            .finish()
    }
}

/// The children of a [`Measurables`], in their order.
#[derive(Clone)]
pub struct MeasurablesIter<'a> {
    list: &'a dyn MeasurableList,
    front: usize,
    back: usize,
}

impl<'a> Iterator for MeasurablesIter<'a> {
    type Item = &'a dyn Measurable;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let child = self.list.get(self.front);
        self.front += 1;
        child
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back - self.front;
        (len, Some(len))
    }
}

impl DoubleEndedIterator for MeasurablesIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        self.list.get(self.back)
    }
}

impl ExactSizeIterator for MeasurablesIter<'_> {}

impl FusedIterator for MeasurablesIter<'_> {}

impl fmt::Debug for MeasurablesIter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeasurablesIter")
            .field("front", &self.front)
            .field("back", &self.back)
            .finish()
    }
}
