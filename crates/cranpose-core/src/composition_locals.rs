use crate::composer_context;
use crate::state::{MutationPolicy, OwnedMutableState};
use crate::{Composer, LocalKey, RuntimeHandle};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct ProvidedValue {
    key: LocalKey,
    #[allow(clippy::type_complexity)] // Closure returns trait object for flexible local values
    apply: Box<dyn Fn(&Composer) -> Rc<dyn Any>>,
}

impl ProvidedValue {
    pub(crate) fn into_entry(self, composer: &Composer) -> (LocalKey, Rc<dyn Any>) {
        let ProvidedValue { key, apply } = self;
        let entry = apply(composer);
        (key, entry)
    }
}

#[allow(non_snake_case)]
pub fn CompositionLocalProvider(
    values: impl IntoIterator<Item = ProvidedValue>,
    content: impl FnOnce(),
) {
    composer_context::with_composer(|composer| {
        let provided: Vec<ProvidedValue> = values.into_iter().collect();
        composer.with_composition_locals(provided, |_composer| content());
    })
}

pub(crate) struct LocalStateEntry<T: Clone + 'static> {
    state: OwnedMutableState<T>,
}

type LocalEquivalentFn<T> = dyn Fn(&T, &T) -> bool + Send + Sync + 'static;

struct LocalValuePolicy<T: Clone + 'static> {
    equivalent: Arc<LocalEquivalentFn<T>>,
}

impl<T: Clone + 'static> MutationPolicy<T> for LocalValuePolicy<T> {
    fn equivalent(&self, a: &T, b: &T) -> bool {
        (self.equivalent)(a, b)
    }
}

impl<T: Clone + 'static> LocalStateEntry<T> {
    fn new(initial: T, runtime: RuntimeHandle, equivalent: Arc<LocalEquivalentFn<T>>) -> Self {
        Self {
            state: OwnedMutableState::with_runtime_and_policy(
                initial,
                runtime,
                Arc::new(LocalValuePolicy { equivalent }),
            ),
        }
    }

    fn set(&self, value: T) {
        self.state.replace(value);
    }

    pub(crate) fn value(&self) -> T {
        self.state.value()
    }
}

pub(crate) struct StaticLocalEntry<T: Clone + 'static> {
    value: RefCell<T>,
}

impl<T: Clone + 'static> StaticLocalEntry<T> {
    fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
        }
    }

    fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
    }

    pub(crate) fn value(&self) -> T {
        self.value.borrow().clone()
    }
}

#[derive(Clone)]
pub struct CompositionLocal<T: Clone + 'static> {
    pub(crate) key: LocalKey,
    default: Rc<dyn Fn() -> T>,
    equivalent: Arc<LocalEquivalentFn<T>>,
}

impl<T: Clone + 'static> PartialEq for CompositionLocal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: Clone + 'static> Eq for CompositionLocal<T> {}

impl<T: Clone + 'static> CompositionLocal<T> {
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key;
        let equivalent = Arc::clone(&self.equivalent);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer| {
                let runtime = composer.runtime_handle();
                let entry_ref = composer.remember_internal(|| {
                    Rc::new(LocalStateEntry::new(
                        value.clone(),
                        runtime.clone(),
                        Arc::clone(&equivalent),
                    ))
                });
                entry_ref.update(|entry| entry.set(value.clone()));
                entry_ref.with(|entry| entry.clone() as Rc<dyn Any>)
            }),
        }
    }

    pub fn current(&self) -> T {
        composer_context::with_composer(|composer| composer.read_composition_local(self))
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

#[allow(non_snake_case)]
pub fn compositionLocalOf<T: Clone + PartialEq + 'static>(
    default: impl Fn() -> T + 'static,
) -> CompositionLocal<T> {
    compositionLocalOfWithPolicy(default, |current, next| current == next)
}

#[allow(non_snake_case)]
pub fn compositionLocalOfWithPolicy<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
    equivalent: impl Fn(&T, &T) -> bool + Send + Sync + 'static,
) -> CompositionLocal<T> {
    CompositionLocal {
        key: crate::next_local_key(),
        default: Rc::new(default),
        equivalent: Arc::new(equivalent),
    }
}

/// A `StaticCompositionLocal` is a CompositionLocal that is optimized for values that are
/// unlikely to change. Unlike `CompositionLocal`, reads of a `StaticCompositionLocal` are not
/// tracked by the recomposition system, which means:
/// - Reading `.current()` does NOT establish a subscription
/// - Changing the provided value does NOT automatically invalidate readers
/// - This makes it more efficient for truly static values
///
/// This matches the API of Jetpack Compose's `staticCompositionLocalOf` but with simplified
/// semantics. Use this for values that are guaranteed to never change during the lifetime of
/// the CompositionLocalProvider scope (e.g., application-wide constants, configuration)
#[derive(Clone)]
pub struct StaticCompositionLocal<T: Clone + 'static> {
    pub(crate) key: LocalKey,
    default: Rc<dyn Fn() -> T>,
}

impl<T: Clone + 'static> PartialEq for StaticCompositionLocal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: Clone + 'static> Eq for StaticCompositionLocal<T> {}

impl<T: Clone + 'static> StaticCompositionLocal<T> {
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key;
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer| {
                let entry_ref =
                    composer.remember_internal(|| Rc::new(StaticLocalEntry::new(value.clone())));
                entry_ref.update(|entry| entry.set(value.clone()));
                entry_ref.with(|entry| entry.clone() as Rc<dyn Any>)
            }),
        }
    }

    pub fn current(&self) -> T {
        composer_context::with_composer(|composer| composer.read_static_composition_local(self))
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

#[allow(non_snake_case)]
pub fn staticCompositionLocalOf<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
) -> StaticCompositionLocal<T> {
    StaticCompositionLocal {
        key: crate::next_local_key(),
        default: Rc::new(default),
    }
}
