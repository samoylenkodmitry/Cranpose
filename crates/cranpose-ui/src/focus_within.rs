//! Telling a container whether keyboard focus is on a control inside it,
//! as Compose's `Modifier.onFocusChanged { it.hasFocus }` does, without
//! making the container a focus target of its own.
//!
//! A focus target built inside a [`FocusWithin`] keeps the observers
//! composed around it and tells them when it gains and loses focus. Each
//! observer counts the focused targets inside it. A focus move takes focus
//! from one target before it gives it to the next, so an observer that the
//! move leaves hears so only at the end of the move, and not at all when
//! focus came back inside it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf, remember};
use cranpose_macros::composable;

pub(crate) struct FocusWithinObserver {
    focused: Cell<u32>,
    on_change: RefCell<Rc<dyn Fn(bool)>>,
}

fn with_left_in_move<R>(f: impl FnOnce(&mut Option<Vec<Rc<FocusWithinObserver>>>) -> R) -> R {
    crate::thread_state::with(|state| f(&mut state.left_in_focus_move.borrow_mut()))
}

impl FocusWithinObserver {
    fn tell(&self, focused: bool) {
        let on_change = Rc::clone(&self.on_change.borrow());
        on_change(focused);
    }

    fn enter(observer: &Rc<Self>) {
        let focused = observer.focused.get() + 1;
        observer.focused.set(focused);
        if focused != 1 {
            return;
        }
        let came_back = with_left_in_move(|left| {
            left.as_mut()
                .and_then(|left| {
                    let index = left.iter().position(|other| Rc::ptr_eq(other, observer))?;
                    Some(left.swap_remove(index))
                })
                .is_some()
        });
        if !came_back {
            observer.tell(true);
        }
    }

    fn leave(observer: &Rc<Self>) {
        let Some(focused) = observer.focused.get().checked_sub(1) else {
            return;
        };
        observer.focused.set(focused);
        if focused != 0 {
            return;
        }
        let deferred = with_left_in_move(|left| {
            left.as_mut()
                .map(|left| left.push(Rc::clone(observer)))
                .is_some()
        });
        if !deferred {
            observer.tell(false);
        }
    }
}

/// Ends a focus move even when it unwinds.
struct FocusMove;

impl Drop for FocusMove {
    fn drop(&mut self) {
        let left = with_left_in_move(Option::take).unwrap_or_default();
        for observer in left {
            if observer.focused.get() == 0 {
                observer.tell(false);
            }
        }
    }
}

/// Runs `block`, a focus move that takes focus from one target and gives it
/// to another: an observer that focus leaves and enters again within it
/// hears nothing.
pub(crate) fn moving_focus(block: impl FnOnce()) {
    let starts_move = with_left_in_move(|left| {
        if left.is_some() {
            return false;
        }
        *left = Some(Vec::new());
        true
    });
    let _move = starts_move.then_some(FocusMove);
    block();
}

/// The focus observers around a focus target, innermost first.
#[derive(Clone)]
pub(crate) struct FocusWithinChain {
    observer: Rc<FocusWithinObserver>,
    outer: Option<Rc<FocusWithinChain>>,
}

/// The same observers in the same order.
impl PartialEq for FocusWithinChain {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.observer, &other.observer) && self.outer == other.outer
    }
}

impl FocusWithinChain {
    /// Tells every observer that a target inside it gained focus.
    pub(crate) fn enter(&self) {
        FocusWithinObserver::enter(&self.observer);
        if let Some(outer) = &self.outer {
            outer.enter();
        }
    }

    /// Tells every observer that a target inside it lost focus.
    pub(crate) fn leave(&self) {
        FocusWithinObserver::leave(&self.observer);
        if let Some(outer) = &self.outer {
            outer.leave();
        }
    }
}

fn local_focus_within() -> CompositionLocal<Option<FocusWithinChain>> {
    crate::thread_state::with_environment(|locals| {
        locals
            .focus_within
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// The focus observers around a focus target built in the running
/// composition; none outside a composition.
pub(crate) fn current_focus_within() -> Option<FocusWithinChain> {
    cranpose_core::composer_context::current_composer()?;
    local_focus_within().current()
}

/// Tells `on_change` whether keyboard focus is on a control composed in
/// `content`: `true` when focus moves onto one of them, `false` when it
/// leaves them all. The observer itself takes no focus, so Tab does not stop
/// on it.
///
/// Compose's `Modifier.onFocusChanged { it.hasFocus }`.
#[composable]
pub fn FocusWithin(on_change: impl Fn(bool) + 'static, content: impl FnOnce()) {
    let on_change: Rc<dyn Fn(bool)> = Rc::new(on_change);
    let observer = remember(|| {
        Rc::new(FocusWithinObserver {
            focused: Cell::new(0),
            on_change: RefCell::new(Rc::new(|_| {})),
        })
    })
    .with(Rc::clone);
    *observer.on_change.borrow_mut() = on_change;
    let local = local_focus_within();
    let chain = FocusWithinChain {
        observer,
        outer: local.current().map(Rc::new),
    };
    CompositionLocalProvider(vec![local.provides(Some(chain))], content);
}
