use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use smallvec::SmallVec;

use crate::{Composer, ComposerCore, RecomposeScope, RecomposeScopeInner, composer_context};

pub struct ParamState<T> {
    pub(crate) value: Option<T>,
}

/// A parameter that points at a shared allocation: two that point at the
/// same one are equal without comparing what it holds.
pub trait SharedParam {
    /// Whether `self` and `other` point at the same allocation.
    fn same_allocation(&self, other: &Self) -> bool;
}

impl<T: ?Sized> SharedParam for Rc<T> {
    fn same_allocation(&self, other: &Self) -> bool {
        Rc::ptr_eq(self, other)
    }
}

impl<T: ?Sized> SharedParam for std::sync::Arc<T> {
    fn same_allocation(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(self, other)
    }
}

impl<T> ParamState<T> {
    /// Brings a call's stored parameters up to date: `fresh` stores them on
    /// the call's first composition, and `refresh` updates each stored field
    /// that differs and says whether any did. Unchanged fields are not
    /// cloned again.
    pub fn update_fields(
        &mut self,
        fresh: impl FnOnce() -> T,
        refresh: impl FnOnce(&mut T) -> bool,
    ) -> bool {
        match self.value.as_mut() {
            Some(stored) => refresh(stored),
            None => {
                self.value = Some(fresh());
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

/// Updates one stored parameter to `new` and says whether it differed,
/// reusing the stored value's allocations where `Clone::clone_from` can.
pub fn refresh_param<T: PartialEq + Clone>(stored: &mut T, new: &T) -> bool {
    if stored == new {
        return false;
    }
    stored.clone_from(new);
    true
}

/// [`refresh_param`] for an `Rc` or `Arc` parameter: the same allocation is
/// unchanged without comparing its contents, which `PartialEq` on a pointer
/// to a type that is not `Eq` otherwise walks in full on every recomposition.
pub fn refresh_shared_param<T: SharedParam + PartialEq + Clone>(stored: &mut T, new: &T) -> bool {
    if stored.same_allocation(new) || stored == new {
        return false;
    }
    stored.clone_from(new);
    true
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

type CallbackScopeCell = Rc<RefCell<Option<RecomposeScope>>>;

struct CallbackScopeGuard {
    core: Rc<ComposerCore>,
}

impl CallbackScopeGuard {
    fn push(composer: &Composer, scope: RecomposeScope) -> Self {
        let mut stack = composer.core.scope_stack.borrow_mut();
        composer
            .core
            .callback_scope_marks
            .borrow_mut()
            .push(stack.len());
        stack.push(scope);
        Self {
            core: composer.clone_core(),
        }
    }
}

impl Drop for CallbackScopeGuard {
    fn drop(&mut self) {
        self.core.scope_stack.borrow_mut().pop();
        self.core.callback_scope_marks.borrow_mut().pop();
    }
}

fn with_callback_scope<R>(scope: Option<RecomposeScope>, f: impl FnOnce() -> R) -> R {
    if let Some(saved_scope) = scope
        && let Some(composer) = composer_context::current_composer()
    {
        let _scope_guard = CallbackScopeGuard::push(&composer, saved_scope);
        return f();
    }

    f()
}

fn callback_owner_is_active(scope: &RefCell<Option<RecomposeScope>>) -> bool {
    scope
        .borrow()
        .as_ref()
        .is_none_or(RecomposeScope::is_effectively_active)
}

fn callback_owner_scope(composer: &Composer) -> Option<RecomposeScope> {
    composer.core.scope_stack.borrow().last().cloned()
}

/// What a [`CallbackHolder`] shares with its forwarders.
#[derive(Default)]
struct CallbackShared {
    callback: RefCell<Option<Box<dyn FnMut()>>>,
    creator_scope: RefCell<Option<RecomposeScope>>,
    /// The scopes whose composition ran the callback. When the composable
    /// that received the callback skips, they run its newest closure again
    /// where the old one ran.
    invokers: RefCell<SmallVec<[Weak<RecomposeScopeInner>; 1]>>,
}

impl CallbackShared {
    fn invoke(&self) {
        if !callback_owner_is_active(&self.creator_scope) {
            return;
        }
        let creator_scope = self.creator_scope.borrow().clone();
        if let Some(composer) = composer_context::current_composer() {
            if let Some(invoker) = composer.content_invocation_scope() {
                self.record_invoker(&invoker);
            }
            if let Some(creator_scope) = creator_scope {
                let _scope_guard = CallbackScopeGuard::push(&composer, creator_scope);
                self.call();
                return;
            }
        }
        self.call();
    }

    fn call(&self) {
        if let Some(callback) = self.callback.borrow_mut().as_mut() {
            callback();
        }
    }

    fn record_invoker(&self, invoker: &RecomposeScope) {
        let mut invokers = self.invokers.borrow_mut();
        if invokers.iter().any(|known| invoker.is(known)) {
            return;
        }
        invokers.retain(|known| known.strong_count() > 0);
        invokers.push(invoker.downgrade());
    }
}

#[derive(Clone, Default)]
pub struct CallbackHolder {
    shared: Rc<CallbackShared>,
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
        *self.shared.callback.borrow_mut() = Some(f);
        *self.shared.creator_scope.borrow_mut() =
            composer_context::try_with_composer(callback_owner_scope).flatten();
    }

    /// Stores the callback a composable call received and says whether the
    /// call's own `scope` ran the previous one while composing. Such a call
    /// runs its body again; any other call may skip, and
    /// [`Self::rerun_invokers`] then shows the new callback.
    pub fn refresh<F>(&self, f: F, scope: &RecomposeScope) -> bool
    where
        F: FnMut() + 'static,
    {
        self.refresh_boxed(Box::new(f), scope)
    }

    /// Boxed form of [`Self::refresh`].
    pub fn refresh_boxed(&self, f: Box<dyn FnMut() + 'static>, scope: &RecomposeScope) -> bool {
        self.update_boxed(f);
        self.shared
            .invokers
            .borrow()
            .iter()
            .any(|invoker| scope.is(invoker))
    }

    /// Invalidates the scopes that ran the previous callback while composing,
    /// for a composable call that skipped its body, and adds them to
    /// `rerun`: each runs the newest callback where the previous one ran.
    pub(crate) fn rerun_invokers(&self, rerun: &mut Vec<RecomposeScope>) {
        self.shared.invokers.borrow_mut().retain(|invoker| {
            match RecomposeScope::upgrade(invoker) {
                Some(scope) => {
                    scope.invalidate();
                    rerun.push(scope);
                    true
                }
                None => false,
            }
        });
    }

    /// Produce a forwarder closure that keeps the holder alive and forwards calls to it.
    pub fn clone_rc(&self) -> impl Fn() + 'static + use<> {
        let shared = Rc::clone(&self.shared);
        move || shared.invoke()
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
            with_callback_scope(creator_scope.borrow().clone(), || {
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

#[cfg(test)]
#[path = "tests/callbacks_param_state_tests.rs"]
mod param_state_tests;
