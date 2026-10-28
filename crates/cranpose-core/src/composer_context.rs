use std::{
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{Composer, ComposerCore, StateId, thread_state};

/// How many threads have a composer installed. Code outside any
/// composition, as a draw closure's branch guards are, reads this instead of
/// the thread-local, which costs a call into the platform on Android.
static INSTALLED_COMPOSERS: AtomicUsize = AtomicUsize::new(0);

/// Whether any thread may have a composer: `false` means this one has none.
#[inline]
fn any_composer_installed() -> bool {
    INSTALLED_COMPOSERS.load(Ordering::Relaxed) != 0
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
    let installs = composer.is_some();
    let previous = thread_state::runtimes(|state| state.current_composer.replace(composer));
    match (previous.is_some(), installs) {
        (false, true) => {
            INSTALLED_COMPOSERS.fetch_add(1, Ordering::Relaxed);
        }
        (true, false) => {
            INSTALLED_COMPOSERS.fetch_sub(1, Ordering::Relaxed);
        }
        _ => {}
    }
    previous
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
#[inline]
pub fn with_composer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    f(&__current_composer())
}

/// The current thread's composer, for the code `#[composable]` expands to.
///
/// # Panics
/// Panics if there is no active composer.
#[doc(hidden)]
pub fn __current_composer() -> Composer {
    thread_state::runtimes(|state| {
        Composer::from_core(Rc::clone(
            state
                .current_composer
                .borrow()
                .as_ref()
                .expect("with_composer: no active composer"),
        ))
    })
}

/// Runs `f` with the current thread's composer core, `None` without one.
/// The check that no thread has one is inlined where it is asked: a draw
/// closure's guards run it many times a draw, and a function holding the
/// lookup too saved its registers before it checked.
#[inline]
pub(crate) fn with_current_core<R>(f: impl FnOnce(&ComposerCore) -> R) -> Option<R> {
    if !any_composer_installed() {
        return None;
    }
    with_installed_core(f)
}

#[inline]
fn with_installed_core<R>(f: impl FnOnce(&ComposerCore) -> R) -> Option<R> {
    installed_core().map(|core| f(&core))
}

// The thread's composer core outside the thread state, so the caller's
// closure runs with no borrow held, and a generic caller makes no thread
// state access of its own: each one was a function a web page compiles at
// its first call (624 in the demo's Liquid UI tab).
#[inline(never)]
fn installed_core() -> Option<Rc<ComposerCore>> {
    thread_state::try_runtimes(|state| state.current_composer.borrow().clone()).flatten()
}

/// Return the current thread's composer.
pub fn current_composer() -> Option<Composer> {
    if !any_composer_installed() {
        return None;
    }
    thread_state::try_runtimes(|state| {
        let core = Rc::clone(state.current_composer.borrow().as_ref()?);
        Some(Composer::from_core(core))
    })
    .flatten()
}

/// Registers `host`, the slot table of a subcomposition, as nested in the active slot table of the
/// current composer, so that forgetting effects in the outer table reaches `host` too. Does nothing
/// without a current composer.
pub fn note_nested_slots_host(host: &Rc<crate::SlotsHost>) {
    let Some(composer) = current_composer() else {
        return;
    };
    let holder = composer.active_slots_host();
    if Rc::ptr_eq(&holder, host) {
        return;
    }
    holder.note_nested_host(host);
}

/// Try to access the current thread's composer.
/// Returns None if there is no active composer.
pub fn try_with_composer<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    current_composer().map(|composer| f(&composer))
}

// forwards on purpose: Compose's name for with_current_composer
/// Runs `f` with the composer that is current on this thread. Panics if there is none. Same as
/// `with_current_composer`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn withCurrentComposer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    with_composer(f)
}

pub(crate) fn with_current_composer_opt<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    try_with_composer(f)
}

#[doc(hidden)]
pub fn current_recompose_scope_invalidated_only_by(
    allowed_sources: impl IntoIterator<Item = StateId>,
) -> Option<bool> {
    with_current_composer_opt(|composer| {
        // The bodies that run now run because the restarted scope called
        // them, with the arguments its body worked out: those come from the
        // states that invalidated it.
        composer
            .restarted_scope()?
            .invalidated_only_by(&allowed_sources.into_iter().collect())
    })
    .flatten()
}
