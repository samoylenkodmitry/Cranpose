use std::{
    any::Any,
    cell::RefCell,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
    sync::Arc,
};

use crate::{
    Composer, Key, RuntimeHandle, avalanche_location_key, composer, composer_context,
    state::{MutationPolicy, OwnedMutableState},
};

fn provider_entry_source(key: &LocalKey, caller: Key) -> Key {
    (key.entry_source() ^ caller).wrapping_mul(0x0000_0100_0000_01b3)
}

/// A composition local paired with the value to provide, made by `provides` and passed to
/// `CompositionLocalProvider`.
pub struct ProvidedValue {
    key: LocalKey,
    #[expect(clippy::type_complexity, reason = "one boxed closure per local")]
    apply: Box<dyn Fn(&Composer, Key) -> (Rc<dyn Any>, bool)>,
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
        site: Key,
    ) -> (LocalKey, Rc<dyn Any>, bool) {
        let Self { key, apply } = self;
        let (entry, static_changed) = apply(composer, site);
        (key, entry, static_changed)
    }
}

/// Makes `values` available to `content` and everything it composes. Where two values are for the
/// same local, the last wins. Compose's `CompositionLocalProvider`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
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

type ComputeLocalFn<T> = dyn Fn(&CompositionLocalAccessorScope) -> T;

/// The value of a local whose provider computes it where it is read, from
/// the other locals there. A reader recomposes when the provider gives a
/// new computation, as with Compose's lambdas that are not remembered.
pub(crate) struct ComputedLocalEntry<T: 'static> {
    compute: OwnedMutableState<Rc<ComputeLocalFn<T>>>,
}

impl<T: 'static> ComputedLocalEntry<T> {
    pub(crate) fn value(&self) -> T {
        let compute = self.compute.value();
        compute(&CompositionLocalAccessorScope { _private: () })
    }
}

/// A static local's computed value: see [`ComputedLocalEntry`]. A new
/// computation recomposes all the provider's content, as a new static value
/// does.
pub(crate) struct StaticComputedLocalEntry<T: 'static> {
    compute: Rc<ComputeLocalFn<T>>,
}

impl<T: 'static> StaticComputedLocalEntry<T> {
    pub(crate) fn value(&self) -> T {
        (self.compute)(&CompositionLocalAccessorScope { _private: () })
    }
}

/// What a computed composition local's value reads other locals through:
/// their values where the computed local is read, not where it was
/// provided. Compose's `CompositionLocalAccessorScope`.
pub struct CompositionLocalAccessorScope {
    _private: (),
}

impl CompositionLocalAccessorScope {
    /// The value `local` has where the computed local is read. Compose's
    /// `CompositionLocal.currentValue` in this scope.
    pub fn current_value<T>(&self, local: &impl ReadableCompositionLocal<T>) -> T {
        local.read_current()
    }
}

/// A composition local whose current value a computed local can read; see
/// [`CompositionLocalAccessorScope`].
pub trait ReadableCompositionLocal<T> {
    /// The local's value where it is read, as `current` gives it.
    fn read_current(&self) -> T;
}

impl<T: Clone + 'static> ReadableCompositionLocal<T> for CompositionLocal<T> {
    fn read_current(&self) -> T {
        self.current()
    }
}

impl<T: Clone + 'static> ReadableCompositionLocal<T> for StaticCompositionLocal<T> {
    fn read_current(&self) -> T {
        self.current()
    }
}

/// A value that composables read from their surroundings without passing it as a parameter, as
/// Jetpack Compose's `compositionLocalOf`. A scope that reads `current` recomposes when the
/// provided value changes.
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
    /// Pairs this local with `value` for use in `CompositionLocalProvider`. When a later
    /// composition provides a different value, the scopes that read the local recompose.
    #[track_caller]
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key.clone();
        let entry_source = provider_entry_source(&key, crate::caller_location_key());
        let equivalent = Arc::clone(&self.equivalent);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: Key| {
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
                entry_ref.with(|entry| (Rc::clone(entry) as Rc<dyn Any>, false))
            }),
        }
    }

    /// Pairs this local with `compute`, which runs at each read of the local
    /// under the provider and reads other locals there through its scope.
    /// A reader recomposes when a local the computation read changes, and
    /// when a later composition of the provider gives a new computation.
    /// Compose's `providesComputed`.
    #[track_caller]
    pub fn provides_computed(
        &self,
        compute: impl Fn(&CompositionLocalAccessorScope) -> T + 'static,
    ) -> ProvidedValue {
        let key = self.key.clone();
        let entry_source = provider_entry_source(&key, crate::caller_location_key());
        let compute: Rc<ComputeLocalFn<T>> = Rc::new(compute);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: Key| {
                let runtime = composer.runtime_handle();
                let source = (entry_source ^ site).wrapping_mul(0x0000_0100_0000_01b3);
                let entry_ref = composer.remember_internal(source, || {
                    Rc::new(ComputedLocalEntry {
                        compute: OwnedMutableState::with_runtime_and_policy(
                            Rc::clone(&compute),
                            runtime.clone(),
                            Rc::new(LocalValuePolicy::<Rc<ComputeLocalFn<T>>> {
                                equivalent: Arc::new(Rc::ptr_eq),
                            }),
                        ),
                    })
                });
                entry_ref.update(|entry| entry.compute.replace(Rc::clone(&compute)));
                entry_ref.with(|entry| (Rc::clone(entry) as Rc<dyn Any>, false))
            }),
        }
    }

    /// Returns the value from the innermost enclosing provider, or the default value when none is
    /// in scope. Must be called during composition. The reading scope recomposes when the provided
    /// value changes.
    pub fn current(&self) -> T {
        composer_context::with_composer(|composer| composer.read_composition_local(self))
    }

    /// Returns the value that `current` gives where no provider is in scope.
    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

#[cfg(test)]
fn malformed_provided_value_for_test(key: LocalKey, entry: Rc<dyn Any>) -> ProvidedValue {
    ProvidedValue {
        key,
        apply: Box::new(move |_, _| (Rc::clone(&entry), false)),
    }
}

#[cfg(test)]
pub(crate) fn malformed_composition_local_for_test<T: Clone + 'static>(
    local: &CompositionLocal<T>,
    entry: Rc<dyn Any>,
) -> ProvidedValue {
    malformed_provided_value_for_test(local.key.clone(), entry)
}

/// Creates a [`CompositionLocal`] whose default is made by `default`. A provided value replaces the
/// previous one when it differs by `==`. Compose's `compositionLocalOf`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn compositionLocalOf<T: Clone + PartialEq + 'static>(
    default: impl Fn() -> T + 'static,
) -> CompositionLocal<T> {
    compositionLocalOfWithPolicy(default, |current, next| current == next)
}

/// Creates a [`CompositionLocal`] whose default is made by `default`. A provided value replaces the
/// previous one only when `equivalent` returns `false` for the two.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
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
    /// The provider frame the last read read under and what it found there,
    /// shared by the local's clones: the next read under the same frame
    /// takes it without walking the providers.
    last_read: Rc<RefCell<Option<StaticRead<T>>>>,
}

/// What a static local's read found under a provider frame.
enum Provided<T: Clone + 'static> {
    Value(Rc<StaticLocalEntry<T>>),
    Computed(Rc<StaticComputedLocalEntry<T>>),
    Default,
}

impl<T: Clone + 'static> Provided<T> {
    fn downgrade(&self) -> StaticFound<T> {
        match self {
            Self::Value(entry) => StaticFound::Value(Rc::downgrade(entry)),
            Self::Computed(entry) => StaticFound::Computed(Rc::downgrade(entry)),
            Self::Default => StaticFound::Default,
        }
    }

    /// The value of an entry, read each time as its provider updates the
    /// entry in place, or what reads it once the provider stack is free.
    fn read(self) -> Result<T, StaticDeferred<T>> {
        match self {
            Self::Value(entry) => Ok(entry.value()),
            Self::Computed(entry) => Err(StaticDeferred::Computed(entry)),
            Self::Default => Err(StaticDeferred::Default),
        }
    }
}

/// [`Provided`], held so the last read keeps no provider's value alive.
enum StaticFound<T: Clone + 'static> {
    Value(Weak<StaticLocalEntry<T>>),
    Computed(Weak<StaticComputedLocalEntry<T>>),
    Default,
}

impl<T: Clone + 'static> StaticFound<T> {
    fn upgrade(&self) -> Option<Provided<T>> {
        match self {
            Self::Value(entry) => entry.upgrade().map(Provided::Value),
            Self::Computed(entry) => entry.upgrade().map(Provided::Computed),
            Self::Default => Some(Provided::Default),
        }
    }
}

/// A static local's value that runs once the provider stack is released,
/// as it may read other locals.
pub(crate) enum StaticDeferred<T: Clone + 'static> {
    Computed(Rc<StaticComputedLocalEntry<T>>),
    Default,
}

impl<T: Clone + 'static> StaticDeferred<T> {
    pub(crate) fn value(self, local: &StaticCompositionLocal<T>) -> T {
        match self {
            Self::Computed(entry) => entry.value(),
            Self::Default => local.default_value(),
        }
    }
}

/// The innermost provider frame a static local was last read under, and
/// what it found there.
struct StaticRead<T: Clone + 'static> {
    frame: Option<Weak<composer::LocalFrame>>,
    found: StaticFound<T>,
}

/// Whether `frame` is the innermost frame of `stack`. A weak handle keeps
/// its frame's allocation, so no other frame takes the address meanwhile.
fn is_top_frame(frame: &Option<Weak<composer::LocalFrame>>, stack: &LocalStackSnapshot) -> bool {
    match (frame, stack) {
        (Some(frame), Some(top)) => std::ptr::eq(frame.as_ptr(), Rc::as_ptr(top)),
        (None, None) => true,
        _ => false,
    }
}

impl<T: Clone + 'static> PartialEq for StaticCompositionLocal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: Clone + 'static> Eq for StaticCompositionLocal<T> {}

impl<T: Clone + 'static> StaticCompositionLocal<T> {
    /// The value `stack` provides for this local, or what to read it from
    /// when it is computed or the default: those may read other locals, so
    /// they run once the caller releases the stack.
    pub(crate) fn provided_under(
        &self,
        stack: &LocalStackSnapshot,
    ) -> Result<T, StaticDeferred<T>> {
        let mut last = self.last_read.borrow_mut();
        if let Some(read) = &*last
            && is_top_frame(&read.frame, stack)
            && let Some(provided) = read.found.upgrade()
        {
            return provided.read();
        }
        let provided = self.find(stack);
        *last = Some(StaticRead {
            frame: stack.as_ref().map(Rc::downgrade),
            found: provided.downgrade(),
        });
        provided.read()
    }

    fn find(&self, stack: &LocalStackSnapshot) -> Provided<T> {
        let Some(entry) = composer::provided_entry(stack, &self.key) else {
            return Provided::Default;
        };
        match entry.downcast::<StaticLocalEntry<T>>() {
            Ok(entry) => Provided::Value(entry),
            Err(entry) => match entry.downcast::<StaticComputedLocalEntry<T>>() {
                Ok(entry) => Provided::Computed(entry),
                Err(_) => {
                    log::error!(
                        "composition local entry type mismatch for key {:?}",
                        self.key
                    );
                    Provided::Default
                }
            },
        }
    }

    /// Pairs this local with `value` for use in `CompositionLocalProvider`. When a later
    /// composition provides a different value, everything under the provider recomposes.
    #[track_caller]
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key.clone();
        let entry_source = provider_entry_source(&key, crate::caller_location_key());
        let equivalent = Arc::clone(&self.equivalent);
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer, site: Key| {
                let source = (entry_source ^ site).wrapping_mul(0x0000_0100_0000_01b3);
                let entry_ref = composer
                    .remember_internal(source, || Rc::new(StaticLocalEntry::new(value.clone())));
                let changed = entry_ref.update(|entry| entry.set(value.clone(), &*equivalent));
                entry_ref.with(|entry| (Rc::clone(entry) as Rc<dyn Any>, changed))
            }),
        }
    }

    /// Pairs this local with `compute`, which runs at each read of the local
    /// under the provider and reads other locals there through its scope.
    /// Each composition of the provider gives a new computation, which
    /// recomposes everything under it. Compose's `providesComputed`.
    #[track_caller]
    pub fn provides_computed(
        &self,
        compute: impl Fn(&CompositionLocalAccessorScope) -> T + 'static,
    ) -> ProvidedValue {
        let compute: Rc<ComputeLocalFn<T>> = Rc::new(compute);
        ProvidedValue {
            key: self.key.clone(),
            apply: Box::new(move |_: &Composer, _: Key| {
                let entry = Rc::new(StaticComputedLocalEntry {
                    compute: Rc::clone(&compute),
                });
                (entry as Rc<dyn Any>, true)
            }),
        }
    }

    /// Returns the value from the innermost enclosing provider, or the default value when none is
    /// in scope. Must be called during composition. A read subscribes nothing.
    pub fn current(&self) -> T {
        composer_context::with_composer(|composer| composer.read_static_composition_local(self))
    }

    /// Returns the value that `current` gives where no provider is in scope.
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
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn staticCompositionLocalOf<T: Clone + PartialEq + 'static>(
    default: impl Fn() -> T + 'static,
) -> StaticCompositionLocal<T> {
    staticCompositionLocalOfWithPolicy(default, |current, next| current == next)
}

/// A [`StaticCompositionLocal`] whose provider recomposes its content when
/// `equivalent` says a new value differs from the last one.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn staticCompositionLocalOfWithPolicy<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
    equivalent: impl Fn(&T, &T) -> bool + Send + Sync + 'static,
) -> StaticCompositionLocal<T> {
    StaticCompositionLocal {
        key: LocalKey::new(),
        default: Rc::new(default),
        equivalent: Arc::new(equivalent),
        last_read: Rc::default(),
    }
}

/// A [`CompositionLocal`] whose default is computed at each read without a
/// provider, from the other locals there. A scope that reads it recomposes
/// when a dynamic local the computation read changes. As the default reads
/// locals, it is computed during composition only. Compose's
/// `compositionLocalWithComputedDefaultOf`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn compositionLocalWithComputedDefaultOf<T: Clone + PartialEq + 'static>(
    compute: impl Fn(&CompositionLocalAccessorScope) -> T + 'static,
) -> CompositionLocal<T> {
    compositionLocalOf(move || compute(&CompositionLocalAccessorScope { _private: () }))
}

/// A [`StaticCompositionLocal`] whose default is computed at each read
/// without a provider, from the other locals there. Compose's
/// `staticCompositionLocalWithComputedDefaultOf`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn staticCompositionLocalWithComputedDefaultOf<T: Clone + PartialEq + 'static>(
    compute: impl Fn(&CompositionLocalAccessorScope) -> T + 'static,
) -> StaticCompositionLocal<T> {
    staticCompositionLocalOf(move || compute(&CompositionLocalAccessorScope { _private: () }))
}

/// The composition locals in scope: the innermost provider's frame, whose
/// parents lead out to the outermost, or `None` outside every provider. A
/// provider pushes one frame over the chain, so a snapshot of the locals is
/// a shared pointer and never a copy of them.
pub(crate) type LocalStackSnapshot = Option<Rc<composer::LocalFrame>>;

#[derive(Clone)]
pub(crate) struct LocalKey(Rc<()>);

impl LocalKey {
    fn new() -> Self {
        Self(Rc::new(()))
    }

    pub(crate) fn entry_source(&self) -> Key {
        avalanche_location_key(Rc::as_ptr(&self.0) as usize as u64)
    }
}

impl std::fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalKey")
            .field(&(Rc::as_ptr(&self.0) as usize))
            .finish()
    }
}

impl PartialEq for LocalKey {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LocalKey {}

impl Hash for LocalKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}
