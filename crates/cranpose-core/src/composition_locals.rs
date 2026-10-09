use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

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
    apply: Box<dyn Fn(&Composer, crate::Key) -> (Rc<dyn Any>, bool)>,
}

impl ProvidedValue {
    pub(crate) fn key(&self) -> &LocalKey {
        &self.key
    }

    /// The provided entry, and whether it replaced a different value of a
    /// static local that this site provided before.
    pub(crate) fn into_entry(
        self,
        composer: &Composer,
        site: crate::Key,
    ) -> (LocalKey, Rc<dyn Any>, bool) {
        let ProvidedValue { key, apply } = self;
        let (entry, static_changed) = apply(composer, site);
        (key, entry, static_changed)
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
                Rc::new(LocalValuePolicy { equivalent }),
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

    /// Stores `value` and says whether it differs from the stored one.
    fn set(&self, value: T, equivalent: &LocalEquivalentFn<T>) -> bool {
        let mut current = self.value.borrow_mut();
        if equivalent(&current, &value) {
            return false;
        }
        *current = value;
        true
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
                entry_ref.with(|entry| (entry.clone() as Rc<dyn Any>, false))
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
        apply: Box::new(move |_, _| (entry.clone(), false)),
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

/// A composition local for values that rarely change, as Jetpack Compose's
/// `staticCompositionLocalOf`. A read of `.current()` subscribes nothing, so
/// it costs less than a read of a [`CompositionLocal`]. In return, a
/// provider whose value changes recomposes everything it provides to, not
/// only the readers: every composable call under it runs its body, and
/// every subcomposition under it composes again the same way.
#[derive(Clone)]
pub struct StaticCompositionLocal<T: Clone + 'static> {
    pub(crate) key: LocalKey,
    default: Rc<dyn Fn() -> T>,
    equivalent: Arc<LocalEquivalentFn<T>>,
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
        let equivalent = Arc::clone(&self.equivalent);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: crate::Key| {
                let source = (entry_source ^ site).wrapping_mul(0x0000_0100_0000_01b3);
                let entry_ref = composer
                    .remember_internal(source, || Rc::new(StaticLocalEntry::new(value.clone())));
                let changed = entry_ref.update(|entry| entry.set(value.clone(), &*equivalent));
                entry_ref.with(|entry| (entry.clone() as Rc<dyn Any>, changed))
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

/// A [`StaticCompositionLocal`] whose provided values compare with `==`.
#[expect(non_snake_case)]
pub fn staticCompositionLocalOf<T: Clone + PartialEq + 'static>(
    default: impl Fn() -> T + 'static,
) -> StaticCompositionLocal<T> {
    staticCompositionLocalOfWithPolicy(default, |current, next| current == next)
}

/// A [`StaticCompositionLocal`] whose provider recomposes its content when
/// `equivalent` says a new value differs from the last one.
#[expect(non_snake_case)]
pub fn staticCompositionLocalOfWithPolicy<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
    equivalent: impl Fn(&T, &T) -> bool + Send + Sync + 'static,
) -> StaticCompositionLocal<T> {
    StaticCompositionLocal {
        key: LocalKey::new(),
        default: Rc::new(default),
        equivalent: Arc::new(equivalent),
    }
}
