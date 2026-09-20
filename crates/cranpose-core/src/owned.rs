use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use crate::runtime::StateHandleLease;

/// Single-threaded owner for values remembered by the Composer.
///
/// This type stores `T` inside an `Rc<RefCell<...>>`, allowing cheap cloning of the
/// handle while keeping ownership of `T` within the composition.
pub struct Owned<T> {
    inner: Rc<OwnedInner<T>>,
}

struct OwnedInner<T> {
    value: RefCell<T>,
    _states: Vec<Rc<StateHandleLease>>,
}

impl<T> Clone for Owned<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Owned<T> {
    pub fn new(value: T) -> Self {
        Self::with_states(value, Vec::new())
    }

    pub(crate) fn with_states(value: T, states: Vec<Rc<StateHandleLease>>) -> Self {
        Self {
            inner: Rc::new(OwnedInner {
                value: RefCell::new(value),
                _states: states,
            }),
        }
    }

    /// Run `f` with an immutable reference to the stored value.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let borrow = self.inner.value.borrow();
        f(&*borrow)
    }

    /// Run `f` with a mutable reference to the stored value.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut borrow = self.inner.value.borrow_mut();
        f(&mut *borrow)
    }

    /// Borrow the stored value immutably.
    pub fn borrow(&self) -> Ref<'_, T> {
        self.inner.value.borrow()
    }

    /// Borrow the stored value mutably.
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.inner.value.borrow_mut()
    }

    /// Replace the stored value entirely.
    pub fn replace(&self, new_value: T) {
        *self.inner.value.borrow_mut() = new_value;
    }
}
