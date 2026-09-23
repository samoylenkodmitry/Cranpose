//! Cranpose screens. The only layer that imports Cranpose.

#![allow(non_snake_case)]

pub mod app;
pub mod diagnostics_screen;
pub mod notes_screen;
pub mod theme;

use std::{ops::Deref, rc::Rc};

/// A shared, main-thread object passed into composables, compared by identity
/// so an unchanged view model lets its composables skip.
pub struct Handle<T>(pub Rc<T>);

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Deref for Handle<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}
