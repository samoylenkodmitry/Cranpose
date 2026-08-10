use crate::composer_context;
use crate::location_key;
use crate::owned::Owned;
use crate::runtime;
use crate::state::{
    DerivedState, MutableState, OwnedMutableState, SnapshotStateList, SnapshotStateMap, State,
};
use std::hash::Hash;
use std::rc::Rc;

pub fn remember<T: 'static>(init: impl FnOnce() -> T) -> Owned<T> {
    composer_context::with_composer(|composer| composer.remember(init))
}

/// Like [`remember`], but recomputes whenever `key` changes.
///
/// A plain `remember` is keyed only by composition position: when a slot is
/// reused with different inputs (a list row that changes identity, an icon
/// whose path prop changes), the stale value survives. `remember_keyed`
/// stores the key beside the value and re-runs `init` on mismatch — the JC
/// `remember(key1) { ... }` contract.
#[allow(non_snake_case)]
pub fn remember_keyed<K, T>(key: K, init: impl FnOnce(&K) -> T) -> T
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
#[allow(non_snake_case)]
pub fn rememberUpdatedState<T: Clone + 'static>(value: T) -> MutableState<T> {
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        let state = composer.remember(|| OwnedMutableState::with_runtime(value.clone(), runtime));
        state.with(|s| {
            s.set(value);
            s.handle()
        })
    })
}

#[cfg(feature = "internal")]
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
/// # When to use
/// Use `mutableStateOf` when:
/// 1.  You are creating state properties inside a struct or class (not a composable function).
/// 2.  You are implementing a custom state management solution.
///
/// **If you are inside a `#[composable]` function, use [`useState`] instead.**
/// `useState` wraps `mutableStateOf` in `remember`, ensuring the state persists
/// across recompositions. Using `mutableStateOf` directly in a composable will
/// recreated the state on every frame, losing data.
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
///             name: mutableStateOf("Alice".into()),
///         }
///     }
/// }
/// ```
///
/// This creates a runtime-owned persistent state. If you need the state lifetime
/// tied to a Rust owner instead, store an [`OwnedMutableState`] or call
/// [`MutableState::retain`] on a handle returned by [`useState`].
#[allow(non_snake_case)]
pub fn mutableStateOf<T: Clone + 'static>(initial: T) -> MutableState<T> {
    let runtime = composer_context::try_with_composer(|composer| composer.runtime_handle())
        .or_else(runtime::current_runtime_handle)
        .expect("mutableStateOf requires an active runtime. Create state inside a composition or after a Runtime is created.");
    runtime.alloc_persistent_state(initial)
}

#[allow(non_snake_case)]
pub fn ownedMutableStateOf<T: Clone + 'static>(initial: T) -> OwnedMutableState<T> {
    let runtime = composer_context::try_with_composer(|composer| composer.runtime_handle())
        .or_else(runtime::current_runtime_handle)
        .expect("ownedMutableStateOf requires an active runtime. Create state inside a composition or after a Runtime is created.");
    OwnedMutableState::with_runtime(initial, runtime)
}

/// Like [`mutableStateOf`] but returns `None` if no runtime is available.
///
/// Use this when you want to lazily initialize reactive state and gracefully
/// handle the case where the runtime isn't yet available.
#[allow(non_snake_case)]
pub fn try_mutableStateOf<T: Clone + 'static>(initial: T) -> Option<MutableState<T>> {
    let runtime = composer_context::try_with_composer(|composer| composer.runtime_handle())
        .or_else(runtime::current_runtime_handle)?;
    Some(runtime.alloc_persistent_state(initial))
}

#[allow(non_snake_case)]
pub fn mutableStateListOf<T, I>(values: I) -> SnapshotStateList<T>
where
    T: Clone + 'static,
    I: IntoIterator<Item = T>,
{
    composer_context::with_composer(move |composer| composer.mutable_state_list_of(values))
}

#[allow(non_snake_case)]
pub fn mutableStateList<T: Clone + 'static>() -> SnapshotStateList<T> {
    mutableStateListOf(std::iter::empty::<T>())
}

#[allow(non_snake_case)]
pub fn mutableStateMapOf<K, V, I>(pairs: I) -> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
    I: IntoIterator<Item = (K, V)>,
{
    composer_context::with_composer(move |composer| composer.mutable_state_map_of(pairs))
}

#[allow(non_snake_case)]
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
///     let count = useState(|| 0);
///
///     Button(
///         Modifier::empty(),
///         ButtonSpec::default(),
///         move || count.set(count.value() + 1),
///         || Text(format!("Count: {}", count.value()))
///     );
/// }
/// ```
#[allow(non_snake_case)]
pub fn useState<T: Clone + PartialEq + 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(|| OwnedMutableState::with_runtime_structural_eq(init(), runtime))
            .with(|state| state.handle())
    })
}

#[allow(non_snake_case)]
pub fn useStateRaw<T: Clone + 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    composer_context::with_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(|| OwnedMutableState::with_runtime(init(), runtime))
            .with(|state| state.handle())
    })
}

#[allow(non_snake_case)]
pub fn derivedStateOf<T: 'static + Clone>(compute: impl Fn() -> T + 'static) -> State<T> {
    composer_context::with_composer(|composer| {
        let key = location_key(file!(), line!(), column!());
        composer.with_group(key, |composer| {
            let should_recompute = composer
                .current_recranpose_scope()
                .map(|scope| scope.should_recompose())
                .unwrap_or(true);
            let runtime = composer.runtime_handle();
            let compute_rc: Rc<dyn Fn() -> T> = Rc::new(compute);
            let derived =
                composer.remember(|| DerivedState::new(runtime.clone(), compute_rc.clone()));
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
