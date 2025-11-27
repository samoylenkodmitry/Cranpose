use std::cell::RefCell;
use std::rc::Rc;

use crate::{Composer, ComposerCore};

// Thread-local stack of Composer handles (safe, no raw pointers).
thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
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

/// Try to access the current composer from the thread-local stack.
/// Returns None if there is no active composer.
pub fn try_with_composer<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    COMPOSER_STACK.with(|stack| {
        let core = stack.borrow().last()?.clone();
        let composer = Composer::from_core(core);
        Some(f(&composer))
    })
}
