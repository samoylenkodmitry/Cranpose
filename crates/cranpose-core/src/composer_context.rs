use std::{cell::RefCell, rc::Rc};

use crate::{Composer, ComposerCore};

thread_local! {
    static CURRENT_COMPOSER: RefCell<Option<Rc<ComposerCore>>> = const { RefCell::new(None) };
}

/// Restores the previous composer when its scope ends.
#[must_use = "ComposerScopeGuard restores the previous composer on drop"]
pub struct ComposerScopeGuard {
    previous: Option<Rc<ComposerCore>>,
}

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        replace_current(self.previous.take());
    }
}

/// Sets the current thread's composer for the duration of the scope.
/// Returns a guard that restores the previous composer on drop.
pub fn enter(composer: &Composer) -> ComposerScopeGuard {
    ComposerScopeGuard {
        previous: replace_current(Some(composer.clone_core())),
    }
}

fn replace_current(composer: Option<Rc<ComposerCore>>) -> Option<Rc<ComposerCore>> {
    CURRENT_COMPOSER.with(|current| current.replace(composer))
}

pub(crate) fn without_composer<R>(f: impl FnOnce() -> R) -> R {
    let _suspended = ComposerScopeGuard {
        previous: replace_current(None),
    };
    f()
}

/// Access the current thread's composer.
///
/// # Panics
/// Panics if there is no active composer.
pub fn with_composer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    CURRENT_COMPOSER.with(|current| {
        let core = current
            .borrow()
            .as_ref()
            .expect("with_composer: no active composer")
            .clone();
        let composer = Composer::from_core(core);
        f(&composer)
    })
}

pub(crate) fn with_current_core<R>(f: impl FnOnce(&ComposerCore) -> R) -> Option<R> {
    CURRENT_COMPOSER.with(|current| current.borrow().as_deref().map(f))
}

/// Return the current thread's composer.
pub fn current_composer() -> Option<Composer> {
    CURRENT_COMPOSER.with(|current| {
        let core = current.borrow().as_ref()?.clone();
        Some(Composer::from_core(core))
    })
}

pub fn note_nested_slots_host(host: &std::rc::Rc<crate::SlotsHost>) {
    let Some(composer) = current_composer() else {
        return;
    };
    let holder = composer.active_slots_host();
    if std::rc::Rc::ptr_eq(&holder, host) {
        return;
    }
    holder.note_nested_host(host);
}

/// Try to access the current thread's composer.
/// Returns None if there is no active composer.
pub fn try_with_composer<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    current_composer().map(|composer| f(&composer))
}
