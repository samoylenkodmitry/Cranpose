use crate::composer_context;
use crate::{Composer, ComposerCore, RecomposeScope};
use std::cell::RefCell;
use std::rc::Rc;

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
/// Used by the #[composable] macro to store Fn-like parameters in the slot table.
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
    if let Some(saved_scope) = captured_scope {
        if let Some(composer) = composer_context::current_composer() {
            let _scope_guard = CallbackScopeGuard::push(&composer, saved_scope);
            return f();
        }
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
    pub fn clone_rc(&self) -> impl Fn() + 'static {
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
    #[allow(clippy::type_complexity)]
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
mod callback_holder_tests {
    use super::{CallbackHolder, CallbackHolder1, ParamSlot};
    use crate::runtime::TestRuntime;
    use crate::RecomposeScope;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn param_slot_take_reports_absence_instead_of_panicking() {
        let slot = ParamSlot::default();

        assert_eq!(slot.take(), None);
        slot.set(11);
        assert_eq!(slot.take(), Some(11));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn callback_holder_default_forwarder_is_noop() {
        let forwarder = CallbackHolder::new().clone_rc();
        forwarder();
    }

    #[test]
    fn callback_holder_forwarder_uses_latest_callback() {
        let holder = CallbackHolder::new();
        let total = Rc::new(Cell::new(0));
        let forwarder = holder.clone_rc();

        let first_total = Rc::clone(&total);
        holder.update(move || first_total.set(first_total.get() + 1));
        forwarder();

        let second_total = Rc::clone(&total);
        holder.update(move || second_total.set(second_total.get() + 10));
        forwarder();

        assert_eq!(total.get(), 11);
    }

    #[test]
    fn callback_holder_does_not_invoke_after_creator_scope_deactivation() {
        let runtime = TestRuntime::new();
        let scope = RecomposeScope::new_for_test(runtime.handle());
        let holder = CallbackHolder::new();
        let invocations = Rc::new(Cell::new(0));
        let invocations_for_callback = Rc::clone(&invocations);
        holder.update(move || invocations_for_callback.set(invocations_for_callback.get() + 1));
        holder.creator_scope.replace(Some(scope.clone()));
        let forwarder = holder.clone_rc();

        forwarder();
        assert_eq!(invocations.get(), 1);

        scope.deactivate();
        forwarder();
        assert_eq!(
            invocations.get(),
            1,
            "callbacks cannot outlive the active composition scope that owns them",
        );
    }

    #[test]
    fn callback_holder_does_not_invoke_under_inactive_creator_ancestor() {
        let runtime = TestRuntime::new();
        let ancestor = RecomposeScope::new_for_test(runtime.handle());
        let creator = RecomposeScope::new_for_test(runtime.handle());
        creator.set_parent_scope(Some(ancestor.clone()));
        let holder = CallbackHolder::new();
        let invocations = Rc::new(Cell::new(0));
        let invocations_for_callback = Rc::clone(&invocations);
        holder.update(move || invocations_for_callback.set(invocations_for_callback.get() + 1));
        holder.creator_scope.replace(Some(creator));
        let forwarder = holder.clone_rc();

        forwarder();
        assert_eq!(invocations.get(), 1);

        ancestor.deactivate();
        forwarder();
        assert_eq!(
            invocations.get(),
            1,
            "callbacks cannot run beneath an inactive composition ancestor",
        );
    }

    #[test]
    fn callback_holder1_default_forwarder_is_noop() {
        let forwarder = CallbackHolder1::<i32>::new().clone_rc();
        forwarder(7);
    }

    #[test]
    fn callback_holder1_forwarder_uses_latest_callback() {
        let holder = CallbackHolder1::<i32>::new();
        let total = Rc::new(Cell::new(0));
        let forwarder = holder.clone_rc();

        let first_total = Rc::clone(&total);
        holder.update(move |value| first_total.set(first_total.get() + value));
        forwarder(2);

        let second_total = Rc::clone(&total);
        holder.update(move |value| second_total.set(second_total.get() + value * 5));
        forwarder(3);

        assert_eq!(total.get(), 17);
    }

    #[test]
    fn callback_holder1_does_not_invoke_after_creator_scope_deactivation() {
        let runtime = TestRuntime::new();
        let scope = RecomposeScope::new_for_test(runtime.handle());
        let holder = CallbackHolder1::<i32>::new();
        let total = Rc::new(Cell::new(0));
        let total_for_callback = Rc::clone(&total);
        holder.update(move |value| total_for_callback.set(total_for_callback.get() + value));
        holder.creator_scope.replace(Some(scope.clone()));
        let forwarder = holder.clone_rc();

        forwarder(3);
        assert_eq!(total.get(), 3);

        scope.deactivate();
        forwarder(5);
        assert_eq!(
            total.get(),
            3,
            "argument callbacks cannot outlive their active composition owner",
        );
    }
}
