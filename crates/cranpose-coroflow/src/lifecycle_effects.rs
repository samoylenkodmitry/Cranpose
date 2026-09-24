use cranpose_core::{DisposableEffect, DisposableEffectResult};
use cranpose_services::{LifecycleState, rememberLifecycleState};

use crate::lifecycle::is_active_for;

/// Where a [`LifecycleStartEffect`] says what undoes its start.
pub struct LifecycleStartStopEffectScope(());

/// What undoes a [`LifecycleStartEffect`]'s start.
pub struct LifecycleStopOrDisposeEffectResult(DisposableEffectResult);

impl LifecycleStartStopEffectScope {
    /// Runs `on_stop` when the host stops, when the effect's keys change, or
    /// when the effect leaves the composition — Compose's
    /// `onStopOrDispose { }`.
    pub fn on_stop_or_dispose(
        &self,
        on_stop: impl FnOnce() + 'static,
    ) -> LifecycleStopOrDisposeEffectResult {
        LifecycleStopOrDisposeEffectResult(DisposableEffectResult::new(on_stop))
    }
}

/// Where a [`LifecycleResumeEffect`] says what undoes its resume.
pub struct LifecycleResumePauseEffectScope(());

/// What undoes a [`LifecycleResumeEffect`]'s resume.
pub struct LifecyclePauseOrDisposeEffectResult(DisposableEffectResult);

impl LifecycleResumePauseEffectScope {
    /// Runs `on_pause` when the host pauses, when the effect's keys change, or
    /// when the effect leaves the composition — Compose's
    /// `onPauseOrDispose { }`.
    pub fn on_pause_or_dispose(
        &self,
        on_pause: impl FnOnce() + 'static,
    ) -> LifecyclePauseOrDisposeEffectResult {
        LifecyclePauseOrDisposeEffectResult(DisposableEffectResult::new(on_pause))
    }
}

/// Runs `effect` every time the host starts while this position is in the
/// composition, and the cleanup it returns when the host stops — Compose's
/// `LifecycleStartEffect(keys) { ...; onStopOrDispose { } }`.
///
/// Changing `keys` runs the cleanup and then `effect` again. A host that
/// reports no lifecycle counts as started.
#[track_caller]
pub fn LifecycleStartEffect<K, F>(keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LifecycleStartStopEffectScope) -> LifecycleStopOrDisposeEffectResult + 'static,
{
    lifecycle_effect(keys, LifecycleState::Started, move || {
        effect(LifecycleStartStopEffectScope(())).0
    });
}

/// Runs `effect` every time the host resumes while this position is in the
/// composition, and the cleanup it returns when the host pauses — Compose's
/// `LifecycleResumeEffect(keys) { ...; onPauseOrDispose { } }`.
///
/// Changing `keys` runs the cleanup and then `effect` again. A host that
/// reports no lifecycle counts as resumed.
#[track_caller]
pub fn LifecycleResumeEffect<K, F>(keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LifecycleResumePauseEffectScope) -> LifecyclePauseOrDisposeEffectResult + 'static,
{
    lifecycle_effect(keys, LifecycleState::Resumed, move || {
        effect(LifecycleResumePauseEffectScope(())).0
    });
}

#[track_caller]
fn lifecycle_effect<K: PartialEq + 'static>(
    keys: K,
    min: LifecycleState,
    effect: impl FnOnce() -> DisposableEffectResult + 'static,
) {
    let active = is_active_for(rememberLifecycleState().get(), min);
    DisposableEffect((keys, active), move |_| {
        if active {
            effect()
        } else {
            DisposableEffectResult::default()
        }
    });
}
