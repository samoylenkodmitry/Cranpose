//! The state `derivedStateOf` keeps: the value it derives and how it derives
//! it again.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use super::{OwnedMutableState, State};
use crate::{RecomposeScope, RecomposeScopeInner, RuntimeHandle, runtime};

/// The value `derivedStateOf` derives, kept in a state cell its readers
/// subscribe to.
pub(crate) struct DerivedState<T: Clone + PartialEq + 'static> {
    core: Rc<DerivedCore<T>>,
}

struct DerivedCore<T: Clone + PartialEq + 'static> {
    compute: RefCell<Rc<dyn Fn() -> T>>,
    state: OwnedMutableState<T>,
    /// The scope whose recomposition derives the value: the states the
    /// derivation reads invalidate it.
    derivation: Weak<RecomposeScopeInner>,
    /// A read derives the value right now, so a read inside the derivation
    /// takes the last value.
    refreshing: Cell<bool>,
}

impl<T: Clone + PartialEq + 'static> DerivedState<T> {
    /// A derived state whose value `compute` derives, again whenever
    /// `derivation` recomposes and before a read once a state it read
    /// changed.
    pub(crate) fn new(
        runtime: RuntimeHandle,
        compute: Rc<dyn Fn() -> T>,
        derivation: Option<&RecomposeScope>,
    ) -> Self {
        let initial = compute();
        let core = Rc::new(DerivedCore {
            compute: RefCell::new(compute),
            state: OwnedMutableState::with_runtime_structural_eq(initial, runtime),
            derivation: derivation.map_or_else(Weak::new, RecomposeScope::downgrade),
            refreshing: Cell::new(false),
        });
        let cell = core.state.handle();
        let refresh: Weak<dyn runtime::DerivedRefresh> = Rc::downgrade(&core) as Weak<_>;
        runtime::with_state_arena_by_id(cell.runtime_id, |arena| {
            arena.register_derived(cell.id, refresh);
        });
        Self { core }
    }

    pub(crate) fn set_compute(&self, compute: Rc<dyn Fn() -> T>) {
        *self.core.compute.borrow_mut() = compute;
    }

    pub(crate) fn recompute(&self) {
        self.core.derive();
    }

    /// The recompose callback of the derivation's scope, which derives the
    /// value again from this state without reading a slot.
    pub(crate) fn rerun(&self) -> impl FnMut(&crate::Composer) + use<T> {
        let core = Rc::downgrade(&self.core);
        move |_| {
            if let Some(core) = core.upgrade() {
                core.derive();
            }
        }
    }

    /// The view readers read the value through.
    pub(crate) fn state(&self) -> State<T> {
        self.core.state.as_state().of_derived()
    }
}

impl<T: Clone + PartialEq + 'static> DerivedCore<T> {
    fn compute(&self) -> Rc<dyn Fn() -> T> {
        Rc::clone(&self.compute.borrow())
    }

    fn derive(&self) {
        let value = self.compute()();
        self.state.set_value(value);
    }
}

impl<T: Clone + PartialEq + 'static> runtime::DerivedRefresh for DerivedCore<T> {
    fn refresh(&self) {
        let stale =
            RecomposeScope::upgrade(&self.derivation).is_some_and(|scope| scope.is_invalid());
        if !stale || self.refreshing.replace(true) {
            return;
        }
        // No scope subscribes to what this derivation reads: the
        // derivation's own recomposition keeps that subscription.
        let value = crate::composer_context::without_composer(|| self.compute()());
        self.refreshing.set(false);
        self.state.set_value(value);
    }
}
