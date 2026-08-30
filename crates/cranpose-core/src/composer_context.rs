use std::{cell::RefCell, rc::Rc};

use crate::{Composer, ComposerCore};

thread_local! {
    static COMPOSER_STACK: RefCell<Vec<Rc<ComposerCore>>> = const { RefCell::new(Vec::new()) };
}

/// Guard that pops the composer stack on drop.
#[must_use = "ComposerScopeGuard pops the composer stack on drop"]
pub struct ComposerScopeGuard;

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        COMPOSER_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            stack.pop();
        });
    }
}

/// Pushes the composer onto the thread-local stack for the duration of the scope.
/// Returns a guard that will pop it on drop.
pub fn enter(composer: &Composer) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| {
        stack.borrow_mut().push(composer.clone_core());
    });
    ComposerScopeGuard
}

/// Access the current composer from the thread-local stack.
///
/// # Panics
/// Panics if there is no active composer.
pub fn with_composer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let core = stack
            .borrow()
            .last()
            .expect("with_composer: no active composer")
            .clone();
        let composer = Composer::from_core(core);
        f(&composer)
    })
}

/// Return the current composer from the thread-local stack.
pub fn current_composer() -> Option<Composer> {
    COMPOSER_STACK.with(|stack| {
        let core = stack.borrow().last()?.clone();
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

/// Try to access the current composer from the thread-local stack.
/// Returns None if there is no active composer.
pub fn try_with_composer<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    current_composer().map(|composer| f(&composer))
}
