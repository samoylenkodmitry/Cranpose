use std::{
    any::Any,
    cell::RefCell,
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

use crate::{
    Composer, LocalKey, RuntimeHandle, composer_context,
    state::{MutationPolicy, OwnedMutableState},
};

fn provider_entry_source(key: &LocalKey, caller: crate::Key) -> crate::Key {
    (key.entry_source() ^ caller).wrapping_mul(0x0000_0100_0000_01b3)
}

pub struct ProvidedValue {
    key: LocalKey,
    #[expect(clippy::type_complexity)]
    apply: Box<dyn Fn(&Composer, crate::Key) -> Rc<dyn Any>>,
}

impl ProvidedValue {
    pub(crate) fn key(&self) -> &LocalKey {
        &self.key
    }

    pub(crate) fn into_entry(
        self,
        composer: &Composer,
        site: crate::Key,
    ) -> (LocalKey, Rc<dyn Any>) {
        let ProvidedValue { key, apply } = self;
        let entry = apply(composer, site);
        (key, entry)
    }
}

#[expect(non_snake_case)]
#[track_caller]
pub fn CompositionLocalProvider(
    values: impl IntoIterator<Item = ProvidedValue>,
    content: impl FnOnce(),
) {
    let site = crate::caller_location_key();
    composer_context::with_composer(|composer| {
        composer.with_composition_locals(values, site, |_composer| content());
    });
}

pub(crate) struct LocalStateEntry<T: Clone + 'static> {
    state: OwnedMutableState<LocalValue<T>>,
}

#[derive(Clone)]
enum LocalValue<T: Clone + 'static> {
    Value(T),
    State(OwnedMutableState<T>),
}

type LocalEquivalentFn<T> = dyn Fn(&T, &T) -> bool + Send + Sync + 'static;

struct LocalValuePolicy<T: Clone + 'static> {
    equivalent: Arc<LocalEquivalentFn<T>>,
}

impl<T: Clone + 'static> MutationPolicy<LocalValue<T>> for LocalValuePolicy<T> {
    fn equivalent(&self, a: &LocalValue<T>, b: &LocalValue<T>) -> bool {
        match (a, b) {
            (LocalValue::Value(a), LocalValue::Value(b)) => (self.equivalent)(a, b),
            (LocalValue::State(a), LocalValue::State(b)) => a.handle() == b.handle(),
            _ => false,
        }
    }
}

impl<T: Clone + 'static> LocalStateEntry<T> {
    fn new(
        initial: LocalValue<T>,
        runtime: RuntimeHandle,
        equivalent: Arc<LocalEquivalentFn<T>>,
    ) -> Self {
        Self {
            state: OwnedMutableState::with_runtime_and_policy(
                initial,
                runtime,
                Rc::new(LocalValuePolicy { equivalent }),
            ),
        }
    }

    fn set(&self, value: LocalValue<T>) {
        self.state.replace(value);
    }

    pub(crate) fn value(&self) -> T {
        self.state.read(|value| match value {
            LocalValue::Value(value) => value.clone(),
            LocalValue::State(state) => state.value(),
        })
    }
}

/// A retained local binding whose value is read in the phase that consumes it.
///
/// Capturing a reader does not subscribe the current composition. A deferred
/// consumer reads it inside a [`crate::SnapshotStateObserver`] to invalidate
/// its own layout or drawing work. The reader retains its provider and follows
/// changes to its value or source while the provider's runtime is alive.
#[derive(Clone)]
pub struct CompositionLocalReader<T: Clone + 'static> {
    pub(crate) local: CompositionLocal<T>,
    pub(crate) entry: Option<Rc<LocalStateEntry<T>>>,
}

impl<T: Clone + 'static> CompositionLocalReader<T> {
    /// Reads the current value, subscribing the current composition scope and
    /// recording the read for any enclosing snapshot observer.
    pub fn value(&self) -> T {
        self.entry
            .as_ref()
            .map_or_else(|| self.local.default_value(), |entry| entry.value())
    }
}

impl<T: Clone + 'static> PartialEq for CompositionLocalReader<T> {
    fn eq(&self, other: &Self) -> bool {
        self.local == other.local
            && match (&self.entry, &other.entry) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl<T: Clone + 'static> Eq for CompositionLocalReader<T> {}

impl<T: Clone + 'static> Hash for CompositionLocalReader<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.local.key.hash(state);
        self.entry.as_ref().map(Rc::as_ptr).hash(state);
    }
}

impl<T: Clone + 'static> fmt::Debug for CompositionLocalReader<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompositionLocalReader")
            .finish_non_exhaustive()
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
    #[track_caller]
    pub fn provides(&self, value: T) -> ProvidedValue {
        self.provide_source(LocalValue::Value(value))
    }

    /// Provides a live state without subscribing the provider's composition.
    ///
    /// State changes invalidate only readers of this local. The source state's
    /// mutation policy governs its updates; replacing the source at this
    /// provider also invalidates readers, including retained layout readers.
    #[track_caller]
    pub fn provides_state(&self, state: OwnedMutableState<T>) -> ProvidedValue {
        self.provide_source(LocalValue::State(state))
    }

    /// Captures the current provider without reading or subscribing to its value.
    pub fn reader(&self) -> CompositionLocalReader<T> {
        composer_context::with_composer(|composer| composer.composition_local_reader(self))
    }

    #[track_caller]
    fn provide_source(&self, value: LocalValue<T>) -> ProvidedValue {
        let key = self.key.clone();
        let entry_source = provider_entry_source(&key, crate::caller_location_key());
        let equivalent = Arc::clone(&self.equivalent);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: crate::Key| {
                let runtime = composer.runtime_handle();
                let source = (entry_source ^ site).wrapping_mul(0x0000_0100_0000_01b3);
                let entry_ref = composer.remember_internal(source, || {
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

#[cfg(test)]
fn malformed_provided_value_for_test(key: LocalKey, entry: Rc<dyn Any>) -> ProvidedValue {
    ProvidedValue {
        key,
        apply: Box::new(move |_, _| entry.clone()),
    }
}

#[cfg(test)]
pub(crate) fn malformed_composition_local_for_test<T: Clone + 'static>(
    local: &CompositionLocal<T>,
    entry: Rc<dyn Any>,
) -> ProvidedValue {
    malformed_provided_value_for_test(local.key.clone(), entry)
}

#[expect(non_snake_case)]
pub fn compositionLocalOf<T: Clone + PartialEq + 'static>(
    default: impl Fn() -> T + 'static,
) -> CompositionLocal<T> {
    compositionLocalOfWithPolicy(default, |current, next| current == next)
}

#[expect(non_snake_case)]
pub fn compositionLocalOfWithPolicy<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
    equivalent: impl Fn(&T, &T) -> bool + Send + Sync + 'static,
) -> CompositionLocal<T> {
    CompositionLocal {
        key: LocalKey::new(),
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
    #[track_caller]
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key.clone();
        let entry_source = provider_entry_source(&key, crate::caller_location_key());
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: crate::Key| {
                let source = (entry_source ^ site).wrapping_mul(0x0000_0100_0000_01b3);
                let entry_ref = composer
                    .remember_internal(source, || Rc::new(StaticLocalEntry::new(value.clone())));
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

#[cfg(test)]
pub(crate) fn malformed_static_composition_local_for_test<T: Clone + 'static>(
    local: &StaticCompositionLocal<T>,
    entry: Rc<dyn Any>,
) -> ProvidedValue {
    malformed_provided_value_for_test(local.key.clone(), entry)
}

#[expect(non_snake_case)]
pub fn staticCompositionLocalOf<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
) -> StaticCompositionLocal<T> {
    StaticCompositionLocal {
        key: LocalKey::new(),
        default: Rc::new(default),
    }
}
