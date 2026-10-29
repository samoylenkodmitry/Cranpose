//! `computedStateOf`: a state whose value its calculation works out on
//! every read.

use std::{
    cell::{Cell, OnceCell},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

use crate::{
    OwnedMutableState, RuntimeHandle, composer_context,
    snapshot_state_observer::SnapshotStateObserver,
};

/// A state whose value `calculation` works out on every read, as Kotlin's
/// `computedStateOf`; see [`computedStateOf`].
pub struct ComputedState<T: Clone + PartialEq + 'static> {
    inner: Rc<ComputedInner<T>>,
}

impl<T: Clone + PartialEq + 'static> Clone for ComputedState<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

/// A state whose value `calculation` works out on every read, as Kotlin's
/// `computedStateOf`. Unlike [`derivedStateOf`](crate::derivedStateOf), it
/// keeps no cached value to read and needs no composition to be made in:
/// a view model can hold one.
///
/// A composable that reads it recomposes only when a state the calculation
/// read changed and the calculation then gives a value other than the one
/// read last. The calculation may run more often than the value is read,
/// once per frame after such a change. It must not write any state.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn computedStateOf<T: Clone + PartialEq + 'static>(
    calculation: impl Fn() -> T + 'static,
) -> ComputedState<T> {
    ComputedState {
        inner: Rc::new_cyclic(|this| ComputedInner {
            calculation: Box::new(calculation),
            this: Weak::clone(this),
            read: OnceCell::new(),
            refresh_queued: Cell::new(false),
        }),
    }
}

impl<T: Clone + PartialEq + 'static> ComputedState<T> {
    /// Works the value out. Read in a composition, it subscribes the
    /// reading scope, which recomposes when the value changes.
    pub fn value(&self) -> T {
        let Some(runtime) = composer_context::try_with_composer(crate::Composer::runtime_handle)
        else {
            return (self.inner.calculation)();
        };
        let value = self.inner.observed();
        let (read, _) = self.inner.read.get_or_init(|| {
            let read =
                OwnedMutableState::with_runtime_structural_eq(value.clone(), runtime.clone());
            (read, runtime)
        });
        // Readers that read another value recompose; this one now reads
        // the value it gets.
        read.set_value(value.clone());
        read.as_state().read(|_| ());
        value
    }

    /// Same as `value`.
    pub fn get(&self) -> T {
        self.value()
    }
}

struct ComputedInner<T: Clone + PartialEq + 'static> {
    calculation: Box<dyn Fn() -> T>,
    this: Weak<Self>,
    /// The value the readers in compositions read last, which they
    /// subscribe to, and the runtime it lives in: set again whenever a
    /// state the calculation read changed, it recomposes them when the value
    /// differs.
    read: OnceCell<(OwnedMutableState<T>, RuntimeHandle)>,
    /// A refresh waits in the runtime's queue.
    refresh_queued: Cell<bool>,
}

impl<T: Clone + PartialEq + 'static> ComputedInner<T> {
    fn key(&self) -> ComputedKey {
        ComputedKey {
            id: self.id(),
            target: Weak::<Self>::clone(&self.this),
        }
    }

    fn id(&self) -> usize {
        std::ptr::from_ref(self).addr()
    }

    /// Runs the calculation with the shared observer noting what it reads,
    /// outside the composer: the reading scope subscribes to the value,
    /// not to what makes it.
    fn observed(&self) -> T {
        with_observer(|observer| {
            observer.observe_reads(self.key(), ComputedKey::changed, || {
                composer_context::without_composer(|| (self.calculation)())
            })
        })
    }
}

trait ComputedRefresh {
    /// A state the calculation read changed: the value is worked out again
    /// before the runtime's next recomposition.
    fn queue_refresh(&self);
}

impl<T: Clone + PartialEq + 'static> ComputedRefresh for ComputedInner<T> {
    fn queue_refresh(&self) {
        let Some((_, runtime)) = self.read.get() else {
            return;
        };
        if self.refresh_queued.replace(true) {
            return;
        }
        let this = Weak::clone(&self.this);
        runtime.enqueue_ui_task(Box::new(move || {
            if let Some(inner) = this.upgrade() {
                inner.refresh_queued.set(false);
                let value = inner.observed();
                if let Some((read, _)) = inner.read.get() {
                    read.set_value(value);
                }
            }
        }));
    }
}

impl<T: Clone + PartialEq + 'static> Drop for ComputedInner<T> {
    fn drop(&mut self) {
        let key = ComputedKey {
            id: self.id(),
            target: Weak::<Released>::new(),
        };
        try_with_observer(|observer| observer.clear(&key));
    }
}

/// What the shared observer files a computed state's reads under.
#[derive(Clone)]
struct ComputedKey {
    id: usize,
    target: Weak<dyn ComputedRefresh>,
}

impl ComputedKey {
    fn changed(&self) {
        if let Some(target) = self.target.upgrade() {
            target.queue_refresh();
        }
    }
}

impl PartialEq for ComputedKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ComputedKey {}

impl Hash for ComputedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// The target of a key made to clear a released state's reads.
struct Released;

impl ComputedRefresh for Released {
    fn queue_refresh(&self) {}
}

/// Runs `f` with the thread's observer of what computed states read.
fn with_observer<R>(f: impl FnOnce(&SnapshotStateObserver) -> R) -> R {
    f(&observer())
}

fn observer() -> SnapshotStateObserver {
    crate::thread_state::runtimes(|state| {
        state
            .computed_observer
            .borrow_mut()
            .get_or_insert_with(|| {
                let observer = SnapshotStateObserver::new(|change| change());
                observer.start();
                observer
            })
            .clone()
    })
}

fn try_with_observer(f: impl FnOnce(&SnapshotStateObserver)) {
    if let Some(observer) = started_observer() {
        f(&observer);
    }
}

fn started_observer() -> Option<SnapshotStateObserver> {
    crate::thread_state::try_runtimes(|state| {
        state
            .computed_observer
            .try_borrow()
            .ok()
            .and_then(|observer| observer.clone())
    })
    .flatten()
}
