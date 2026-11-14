use std::rc::Rc;

use cranpose_ui_layout::ParentData;

// A chain's parent data, which the records of its node's parent share:
// `None` for the default data, which most nodes carry. A chain keeps the
// pointer while its data stays equal, so one pointer test tells a record
// that nothing changed.
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedParentData(Option<Rc<ParentData>>);

impl SharedParentData {
    pub(crate) fn of(data: ParentData) -> Self {
        Self((data != ParentData::default()).then(|| Rc::new(data)))
    }

    // Holds `data` in place of the data held. A chain that alone holds its
    // data writes over it: no record keeps that pointer to compare with.
    pub(crate) fn set(&mut self, data: ParentData) {
        match self.0.as_mut().and_then(Rc::get_mut) {
            Some(held) if data != ParentData::default() => *held = data,
            _ => *self = Self::of(data),
        }
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&ParentData) -> R) -> R {
        match &self.0 {
            Some(data) => read(data),
            None => read(&ParentData::default()),
        }
    }

    // Whether both share one data, so that it cannot differ.
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(data), Some(other)) => Rc::ptr_eq(data, other),
            _ => false,
        }
    }
}
