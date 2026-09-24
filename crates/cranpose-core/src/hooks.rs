use std::{hash::Hash, rc::Rc, sync::Arc};

use crate::{
    composer_context,
    owned::Owned,
    runtime,
    state::{
        DerivedState, MutableState, OwnedMutableState, SnapshotStateList, SnapshotStateMap, State,
        StructuralEqual,
    },
};

#[track_caller]
pub fn remember<T: 'static>(init: impl FnOnce() -> T) -> Owned<T> {
    let source = crate::caller_location_key();
    composer_context::with_composer(|composer| composer.remember_at(source, init))
}

/// Like [`remember`], but recomputes whenever `key` changes.
///
/// A plain `remember` is keyed only by composition position: when a slot is
/// reused with different inputs (a list row that changes identity, an icon
/// whose path prop changes), the stale value survives. `rememberKeyed`
/// stores the key beside the value and re-runs `init` on mismatch — the JC
/// `remember(key1) { ... }` contract.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberKeyed<K, T>(key: K, init: impl FnOnce(&K) -> T) -> T
where
    K: PartialEq + 'static,
    T: Clone + 'static,
{
    let slot = remember(|| std::cell::RefCell::new(None::<(K, T)>));
    slot.with(|cell| {
        let mut stored = cell.borrow_mut();
        match &*stored {
            Some((stored_key, value)) if *stored_key == key => value.clone(),
            _ => {
                let value = init(&key);
                *stored = Some((key, value.clone()));
                value
            }
        }
    })
}

/// Returns a [`MutableState`] that always holds the latest value.
///
/// The state **reference** is stable across recompositions; only the **value** updates.
/// This allows closures to capture a stable reference while reading fresh values.
///
/// # Use Case
/// Use when a `remember`ed closure needs to read a value that changes each recomposition
/// without recreating the closure itself.
///
/// # Example
/// ```rust,ignore
/// let config = build_config(); // Rebuilt each recomposition
/// let config_state = rememberUpdatedState(config);
///
/// // This closure is created once, reads latest config via state
/// let callback = remember(|| {
///     let cfg = config_state;
///     Rc::new(move || do_something(&cfg.value()))
/// }).with(|c| c.clone());
/// ```
///
/// # JC Equivalent
/// ```kotlin
/// @Composable
/// fun <T> rememberUpdatedState(newValue: T): State<T> =
///     remember { mutableStateOf(newValue) }.apply { value = newValue }
/// ```
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberUpdatedState<T: Clone + 'static>(value: T) -> MutableState<T> {
    let source = crate::caller_location_key();
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        let state = composer.remember_at(source, || {
            OwnedMutableState::with_runtime(value.clone(), runtime)
        });
        state.with(|s| {
            s.set(value);
            s.handle()
        })
    })
}

#[cfg(feature = "internal")]
#[expect(non_snake_case)]
pub fn withFrameNanos(
    callback: impl FnOnce(u64) + 'static,
) -> crate::internal::FrameCallbackRegistration {
    composer_context::with_composer(|composer| {
        composer
            .runtime_handle()
            .frame_clock()
            .with_frame_nanos(callback)
    })
}

#[cfg(feature = "internal")]
#[expect(non_snake_case)]
pub fn withFrameMillis(
    callback: impl FnOnce(u64) + 'static,
) -> crate::internal::FrameCallbackRegistration {
    composer_context::with_composer(|composer| {
        composer
            .runtime_handle()
            .frame_clock()
            .with_frame_millis(callback)
    })
}

/// Creates a new `MutableState` initialized with the given value.
///
/// `MutableState` is a cheap copyable observable handle. Reads are tracked by the
/// current composer or snapshot, and writes trigger recomposition of scopes that
/// read it.
///
/// Writing a value equal to the one already there is not a change, so it does
/// not recompose anything -- Jetpack Compose's `structuralEqualityPolicy()`,
/// which is its default too. Use [`mutableStateOfNeverEqual`] for a value that
/// cannot be compared, or one whose every write must count.
///
/// # When to use
/// Use `mutableStateOf` when:
/// 1.  You are creating state properties inside a struct or class (not a composable function).
/// 2.  You are implementing a custom state management solution.
///
/// **Inside a `#[composable]` function this must sit inside a `remember`**,
/// either [`rememberMutableStateOf`] for a single state or [`remember`] around
/// the struct that holds several. Called straight from a composable body it
/// makes a new state every pass and loses the value, exactly as in Kotlin.
///
/// # Example
///
/// ```rust,ignore
/// struct MyViewModel {
///     name: MutableState<String>,
/// }
///
/// impl MyViewModel {
///     fn new() -> Self {
///         Self {
///             name: mutableStateOf(String::from("Alice")),
///         }
///     }
/// }
///
/// #[composable]
/// fn rememberMyViewModel() -> MyViewModel {
///     remember(MyViewModel::new).with(|model| model.clone())
/// }
/// ```
///
/// A state made while a [`remember`] is building its value belongs to that
/// slot and is released with it, which is what lets the struct above be
/// remembered whole. Made anywhere else it is owned by the runtime and lives
/// as long as the runtime does; to tie that to a Rust owner instead, store an
/// [`OwnedMutableState`] or call [`MutableState::retain`] on the handle.
#[expect(non_snake_case)]
pub fn mutableStateOf<T: Clone + PartialEq + 'static>(initial: T) -> MutableState<T> {
    current_runtime("mutableStateOf")
        .alloc_persistent_state_with_policy(initial, Arc::new(StructuralEqual))
}

/// Like [`mutableStateOf`], for a value that cannot be compared or whose every
/// write must count as a change.
///
/// This is Jetpack Compose's `neverEqualPolicy()`, and the non-remembered form
/// of [`rememberMutableStateOfNeverEqual`].
#[expect(non_snake_case)]
pub fn mutableStateOfNeverEqual<T: Clone + 'static>(initial: T) -> MutableState<T> {
    current_runtime("mutableStateOfNeverEqual").alloc_persistent_state(initial)
}

#[expect(non_snake_case)]
pub fn ownedMutableStateOf<T: Clone + PartialEq + 'static>(initial: T) -> OwnedMutableState<T> {
    OwnedMutableState::with_runtime_structural_eq(initial, current_runtime("ownedMutableStateOf"))
}

/// Like [`ownedMutableStateOf`], for a value that cannot be compared.
#[expect(non_snake_case)]
pub fn ownedMutableStateOfNeverEqual<T: Clone + 'static>(initial: T) -> OwnedMutableState<T> {
    OwnedMutableState::with_runtime(initial, current_runtime("ownedMutableStateOfNeverEqual"))
}

fn current_runtime(what: &str) -> runtime::RuntimeHandle {
    composer_context::try_with_composer(super::composer::Composer::runtime_handle)
        .or_else(runtime::current_runtime_handle)
        .unwrap_or_else(|| {
            panic!(
                "{what} requires an active runtime. Create state inside a composition or after a Runtime is created."
            )
        })
}

/// Like [`mutableStateOf`] but returns `None` if no runtime is available.
///
/// Use this when you want to lazily initialize reactive state and gracefully
/// handle the case where the runtime isn't yet available.
#[expect(non_snake_case)]
pub fn try_mutableStateOf<T: Clone + PartialEq + 'static>(initial: T) -> Option<MutableState<T>> {
    let runtime = composer_context::try_with_composer(super::composer::Composer::runtime_handle)
        .or_else(runtime::current_runtime_handle)?;
    Some(runtime.alloc_persistent_state_with_policy(initial, Arc::new(StructuralEqual)))
}

#[expect(non_snake_case)]
pub fn mutableStateListOf<T, I>(values: I) -> SnapshotStateList<T>
where
    T: Clone + 'static,
    I: IntoIterator<Item = T>,
{
    composer_context::with_composer(move |composer| composer.mutable_state_list_of(values))
}

#[expect(non_snake_case)]
pub fn mutableStateList<T: Clone + 'static>() -> SnapshotStateList<T> {
    mutableStateListOf(std::iter::empty::<T>())
}

#[expect(non_snake_case)]
pub fn mutableStateMapOf<K, V, I>(pairs: I) -> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
    I: IntoIterator<Item = (K, V)>,
{
    composer_context::with_composer(move |composer| composer.mutable_state_map_of(pairs))
}

#[expect(non_snake_case)]
pub fn mutableStateMap<K, V>() -> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    mutableStateMapOf(std::iter::empty::<(K, V)>())
}

/// A composable hook that creates and remembers a `MutableState`.
///
/// This is the primary way to define local state in a composable function.
/// It combines `remember` and `mutableStateOf`.
///
/// # Arguments
///
/// * `init` - A closure that provides the initial value. This is only called once
///   when the composable enters the composition.
///
/// # Example
///
/// ```rust,ignore
/// #[composable]
/// fn Counter() {
///     // "count" persists across recompositions.
///     // If we used mutableStateOf directly, it would reset to 0 every frame.
///     let count = rememberMutableStateOf(|| 0);
///
///     Button(
///         Modifier::empty(),
///         ButtonSpec::default(),
///         move || count.set(count.value() + 1),
///         || Text(format!("Count: {}", count.value()))
///     );
/// }
/// ```
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberMutableStateOf<T: Clone + PartialEq + 'static>(
    init: impl FnOnce() -> T,
) -> MutableState<T> {
    let source = crate::caller_location_key();
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember_at(source, || {
                OwnedMutableState::with_runtime_structural_eq(init(), runtime)
            })
            .with(super::state::OwnedMutableState::handle)
    })
}

#[expect(non_snake_case)]
#[track_caller]
pub fn rememberMutableStateOfNeverEqual<T: Clone + 'static>(
    init: impl FnOnce() -> T,
) -> MutableState<T> {
    let source = crate::caller_location_key();
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember_at(source, || OwnedMutableState::with_runtime(init(), runtime))
            .with(super::state::OwnedMutableState::handle)
    })
}

#[expect(non_snake_case)]
#[track_caller]
pub fn derivedStateOf<T: 'static + Clone>(compute: impl Fn() -> T + 'static) -> State<T> {
    let source = crate::caller_location_key();
    composer_context::with_composer(|composer| {
        composer.with_group(source, |composer| {
            let should_recompute = composer
                .current_recompose_scope()
                .is_none_or(|scope| scope.should_recompose());
            let runtime = composer.runtime_handle();
            let compute_rc: Rc<dyn Fn() -> T> = Rc::new(compute);
            let derived = composer.remember_at(source, || {
                DerivedState::new(runtime.clone(), compute_rc.clone())
            });
            derived.update(|derived| {
                derived.set_compute(compute_rc.clone());
                if should_recompute {
                    derived.recompute();
                }
            });
            derived.with(|derived| derived.state.as_state())
        })
    })
}
