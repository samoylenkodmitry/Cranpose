use crate::{Key, caller_location_key, effect_key, with_current_composer};

#[derive(Default)]
pub(crate) struct DisposableEffectState {
    key: Option<effect_key::EffectKey>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectState {
    fn should_run<K: PartialEq + 'static>(&self, keys: &K) -> bool {
        self.key.as_ref().is_none_or(|current| !current.holds(keys))
    }

    fn set_key(&mut self, key: effect_key::EffectKey) {
        self.key = Some(key);
    }

    fn set_cleanup(&mut self, cleanup: Option<Box<dyn FnOnce()>>) {
        self.cleanup = cleanup;
    }

    fn run_cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for DisposableEffectState {
    fn drop(&mut self) {
        self.run_cleanup();
    }
}

/// The value a [`DisposableEffect`] body receives. Its `on_dispose` method makes the result that
/// the body returns.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisposableEffectScope;

/// What a [`DisposableEffect`] body returns: the cleanup to run when the effect is disposed. Make
/// it with `DisposableEffectScope::on_dispose`.
#[derive(Default)]
pub struct DisposableEffectResult {
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectScope {
    /// Returns a result that runs `cleanup` when the effect leaves the composition or its keys
    /// change.
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) -> DisposableEffectResult {
        DisposableEffectResult::new(cleanup)
    }
}

impl DisposableEffectResult {
    /// Creates a result that runs `cleanup` when the effect is disposed.
    pub fn new(cleanup: impl FnOnce() + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        self.cleanup
    }
}

/// Queues `effect` to run each time this call site is composed, after the node changes of the pass
/// are applied. Compose's `SideEffect`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn SideEffect(effect: impl FnOnce() + 'static) {
    with_current_composer(|composer| composer.register_side_effect(effect));
}

/// Implements [`DisposableEffect`] for a call site that has already computed its group key. Call
/// `DisposableEffect` instead.
#[doc(hidden)]
pub fn __disposable_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    with_current_composer(|composer| {
        let state = composer.remember_effect::<DisposableEffectState>(group_key);
        if state.with(|state| state.should_run(&keys)) {
            let key = effect_key::EffectKey::new(keys);
            state.update(|state| {
                state.run_cleanup();
                state.set_key(key);
            });
            let mut effect_opt = Some(effect);
            composer.register_side_effect(move || {
                if let Some(effect) = effect_opt.take() {
                    let result = effect(DisposableEffectScope);
                    state.update(|state| state.set_cleanup(result.into_cleanup()));
                }
            });
        }
    });
}

/// Runs `effect` when this call site first enters composition, and again
/// whenever `keys` no longer equals the value it ran with last time; the
/// previous run's [`DisposableEffectScope::on_dispose`] cleanup, if any,
/// runs first, and also when the call site leaves composition entirely.
///
/// `keys` may be a tuple to depend on more than one value, matching Jetpack
/// Compose's `DisposableEffect(vararg keys)`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn DisposableEffect<K, F>(keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    __disposable_effect_impl(caller_location_key(), keys, effect);
}

/// Clones values into new bindings and evaluates a body with those bindings in scope, which saves
/// writing out the clones before a `move` closure. Each alias before the semicolon is either a
/// name, which clones the variable of that name, or `name = expression`, which clones the value of
/// the expression. The body follows the semicolon.
#[macro_export]
macro_rules! clone_captures {
    ($($alias:ident $(= $value:expr)?),+ $(,)?; $body:expr) => {{
        $(let $alias = $crate::clone_captures!(@clone $alias $(= $value)?);)+
        $body
    }};
    (@clone $alias:ident = $value:expr) => {
        ($value).clone()
    };
    (@clone $alias:ident) => {
        $alias.clone()
    };
}
