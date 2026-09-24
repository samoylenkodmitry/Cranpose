use std::{cell::RefCell, rc::Rc};

use crate::{Composer, ComposerCore, RecomposeScope, composer_context};

pub struct ParamState<T> {
    pub(crate) value: Option<T>,
}

impl<T> ParamState<T> {
    pub fn update(&mut self, new_value: &T) -> bool
    where
        T: PartialEq + Clone,
    {
        match self.value.as_mut() {
            Some(old) if old == new_value => false,
            Some(old) => {
                old.clone_from(new_value);
                true
            }
            None => {
                self.value = Some(new_value.clone());
                true
            }
        }
    }

    pub fn value(&self) -> Option<T>
    where
        T: Clone,
    {
        self.value.clone()
    }
}

/// ParamSlot holds function/closure parameters by ownership (no PartialEq/Clone required).
/// Used by the `#[composable]` macro to store Fn-like parameters in the slot table.
pub struct ParamSlot<T> {
    val: RefCell<Option<T>>,
}

impl<T> Default for ParamSlot<T> {
    fn default() -> Self {
        Self {
            val: RefCell::new(None),
        }
    }
}

impl<T> ParamSlot<T> {
    pub fn set(&self, v: T) {
        *self.val.borrow_mut() = Some(v);
    }

    /// Takes the value out temporarily for a recomposition callback.
    pub fn take(&self) -> Option<T> {
        self.val.borrow_mut().take()
    }
}

type CallbackCell = Rc<RefCell<Option<Box<dyn FnMut()>>>>;
type CallbackScopeCell = Rc<RefCell<Option<RecomposeScope>>>;

struct CallbackScopeGuard {
    core: Rc<ComposerCore>,
}

impl CallbackScopeGuard {
    fn push(composer: &Composer, scope: RecomposeScope) -> Self {
        composer.core.scope_stack.borrow_mut().push(scope);
        Self {
            core: composer.clone_core(),
        }
    }
}

impl Drop for CallbackScopeGuard {
    fn drop(&mut self) {
        self.core.scope_stack.borrow_mut().pop();
    }
}

fn with_callback_scope<R>(scope: &CallbackScopeCell, f: impl FnOnce() -> R) -> R {
    let captured_scope = scope.borrow().clone();
    if let Some(saved_scope) = captured_scope
        && let Some(composer) = composer_context::current_composer()
    {
        let _scope_guard = CallbackScopeGuard::push(&composer, saved_scope);
        return f();
    }

    f()
}

fn callback_owner_is_active(scope: &CallbackScopeCell) -> bool {
    scope
        .borrow()
        .as_ref()
        .is_none_or(RecomposeScope::is_effectively_active)
}

fn callback_owner_scope(composer: &Composer) -> Option<RecomposeScope> {
    composer.core.scope_stack.borrow().last().cloned()
}

#[derive(Clone)]
pub struct CallbackHolder {
    rc: CallbackCell,
    creator_scope: CallbackScopeCell,
}

impl CallbackHolder {
    /// Create a new holder with a no-op callback so that callers can immediately invoke it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the stored callback with a new closure provided by the caller.
    pub fn update<F>(&self, f: F)
    where
        F: FnMut() + 'static,
    {
        self.update_boxed(Box::new(f));
    }

    /// Boxed form of [`Self::update`]: lets generated composable helpers take
    /// callbacks type-erased at the public-fn boundary, so helper bodies are
    /// compiled once instead of once per caller closure type.
    pub fn update_boxed(&self, f: Box<dyn FnMut() + 'static>) {
        *self.rc.borrow_mut() = Some(f);
        *self.creator_scope.borrow_mut() =
            composer_context::try_with_composer(callback_owner_scope).flatten();
    }

    /// Produce a forwarder closure that keeps the holder alive and forwards calls to it.
    pub fn clone_rc(&self) -> impl Fn() + 'static + use<> {
        let rc = self.rc.clone();
        let creator_scope = self.creator_scope.clone();
        move || {
            if !callback_owner_is_active(&creator_scope) {
                return;
            }
            with_callback_scope(&creator_scope, || {
                if let Some(callback) = rc.borrow_mut().as_mut() {
                    callback();
                }
            });
        }
    }
}

impl Default for CallbackHolder {
    fn default() -> Self {
        Self {
            rc: Rc::new(RefCell::new(None)),
            creator_scope: Rc::new(RefCell::new(None)),
        }
    }
}

/// CallbackHolder1 keeps the latest single-argument callback closure alive across recompositions.
/// It mirrors [`CallbackHolder`] but supports callbacks that receive one argument.
#[derive(Clone)]
pub struct CallbackHolder1<A: 'static> {
    #[expect(clippy::type_complexity)]
    rc: Rc<RefCell<Option<Box<dyn FnMut(A)>>>>,
    creator_scope: CallbackScopeCell,
}

impl<A: 'static> CallbackHolder1<A> {
    /// Create a new holder with a no-op callback so callers can invoke it immediately.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the stored callback with a new closure provided by the caller.
    pub fn update<F>(&self, f: F)
    where
        F: FnMut(A) + 'static,
    {
        *self.rc.borrow_mut() = Some(Box::new(f));
        *self.creator_scope.borrow_mut() =
            composer_context::try_with_composer(callback_owner_scope).flatten();
    }

    /// Produce a forwarder closure that keeps the holder alive and forwards calls to it.
    pub fn clone_rc(&self) -> impl Fn(A) + 'static {
        let rc = self.rc.clone();
        let creator_scope = self.creator_scope.clone();
        move |arg| {
            if !callback_owner_is_active(&creator_scope) {
                return;
            }
            with_callback_scope(&creator_scope, || {
                if let Some(callback) = rc.borrow_mut().as_mut() {
                    callback(arg);
                }
            });
        }
    }
}

impl<A: 'static> Default for CallbackHolder1<A> {
    fn default() -> Self {
        Self {
            rc: Rc::new(RefCell::new(None)),
            creator_scope: Rc::new(RefCell::new(None)),
        }
    }
}

pub struct ReturnSlot<T> {
    value: Option<T>,
}

impl<T: Clone> ReturnSlot<T> {
    pub fn store(&mut self, value: T) {
        self.value = Some(value);
    }

    pub fn get(&self) -> Option<T> {
        self.value.clone()
    }
}

impl<T> Default for ParamState<T> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<T> Default for ReturnSlot<T> {
    fn default() -> Self {
        Self { value: None }
    }
}

#[cfg(test)]
#[path = "tests/callbacks_callback_holder_tests.rs"]
mod callback_holder_tests;
