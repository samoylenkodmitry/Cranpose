#[cfg(not(target_arch = "wasm32"))]
use std::cell::{Cell, RefCell};
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    CoroutineScope, Key, Owned, RuntimeHandle, TaskSite, effect_key::EffectKey,
    task_site::TaskLabel, with_current_composer,
};

#[derive(Default)]
struct LaunchedEffectState {
    key: Option<EffectKey>,
    scope: Option<LaunchedEffectScope>,
}

impl LaunchedEffectState {
    fn should_run<K: PartialEq + 'static>(&self, keys: &K) -> bool {
        self.key.as_ref().is_none_or(|current| !current.holds(keys))
    }
}

impl Drop for LaunchedEffectState {
    fn drop(&mut self) {
        if let Some(scope) = self.scope.take() {
            scope.cancel();
        }
    }
}

/// The scope a [`LaunchedEffect`] receives: the effect's coroutine scope.
///
/// The effect is cancelled when its keys change or its call site leaves the
/// composition. The cancellation drops the effect's future where it waits.
/// No line after that `.await` runs. The cancellation also cancels every task
/// the scope launched.
#[derive(Clone)]
pub struct LaunchedEffectScope {
    active: Arc<AtomicBool>,
    runtime: RuntimeHandle,
    coroutines: CoroutineScope,
    #[cfg(not(target_arch = "wasm32"))]
    continuations: Rc<RefCell<Vec<u64>>>,
}

impl LaunchedEffectScope {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            active: Arc::new(AtomicBool::new(true)),
            coroutines: CoroutineScope::new(Some(runtime.clone())),
            runtime,
            #[cfg(not(target_arch = "wasm32"))]
            continuations: Rc::default(),
        }
    }

    fn cancel(&self) {
        self.active.store(false, Ordering::SeqCst);
        self.coroutines.close();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = std::mem::take(&mut *self.continuations.borrow_mut());
            for id in pending {
                self.runtime.cancel_ui_cont(id);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn release_continuation(&self, id: u64) {
        self.continuations.borrow_mut().retain(|entry| *entry != id);
    }

    /// Returns `true` until the effect is cancelled. The effect is cancelled
    /// when its keys change or its call site leaves the composition.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Returns a handle to the runtime that runs the effect.
    pub fn runtime(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    /// Waits for the next frame, then calls `on_frame` with the frame time in
    /// nanoseconds and returns what it returns. Compose's `withFrameNanos`.
    /// Await it in a loop to run once per frame:
    ///
    /// ```ignore
    /// LaunchedEffect((), async move |scope| loop {
    ///     scope.with_frame_nanos(|nanos| time.set(nanos)).await;
    /// });
    /// ```
    #[cfg(feature = "internal")]
    pub async fn with_frame_nanos<R>(&self, on_frame: impl FnOnce(u64) -> R) -> R {
        on_frame(self.runtime.frame_clock().next_frame().await)
    }

    /// Waits for the next frame, then calls `on_frame` with the frame time in
    /// milliseconds and returns what it returns. Compose's `withFrameMillis`.
    #[cfg(feature = "internal")]
    pub async fn with_frame_millis<R>(&self, on_frame: impl FnOnce(u64) -> R) -> R {
        self.with_frame_nanos(|nanos| on_frame(nanos / 1_000_000))
            .await
    }

    /// Launches `future` on the UI thread as a child of the effect, as
    /// Compose's `launch` inside a `LaunchedEffect` does: the effect's
    /// cancellation cancels it too.
    pub fn launch(&self, future: impl Future<Output = ()> + 'static) {
        self.coroutines.launch(future);
    }

    /// Runs background work and delivers results to the UI.
    ///
    /// On native targets, `work` runs on a worker thread. Its future runs to
    /// completion there. The `on_ui` continuation runs on the runtime thread.
    /// It may capture `Rc` and `RefCell` state. It does not run once the
    /// effect is cancelled.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn launch_background<T, Work, Ui, Fut>(&self, work: Work, on_ui: Ui)
    where
        T: Send + 'static,
        Work: FnOnce(CancelToken) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        Ui: FnOnce(T) + 'static,
    {
        if !self.is_active() {
            return;
        }
        let dispatcher = self.runtime.dispatcher();
        let active_for_thread = Arc::clone(&self.active);
        let continuation_scope = self.clone();
        let id_cell = Rc::new(Cell::new(0));
        let id_for_closure = Rc::clone(&id_cell);
        let continuation = move |value: T| {
            continuation_scope.release_continuation(id_for_closure.get());
            if continuation_scope.is_active() {
                on_ui(value);
            }
        };

        let Some(cont_id) = self.runtime.register_ui_cont(continuation) else {
            return;
        };
        id_cell.set(cont_id);
        self.continuations.borrow_mut().push(cont_id);

        std::thread::spawn(move || {
            let token = CancelToken::new(Arc::clone(&active_for_thread));
            let value = pollster::block_on(work(token.clone()));
            if token.is_cancelled() {
                return;
            }
            dispatcher.post_invoke(cont_id, value);
        });
    }

    /// Runs background work and delivers results to the UI.
    ///
    /// On the web, `work` runs as a child task of the effect on the runtime
    /// thread. The `on_ui` continuation runs on the same thread. It does not
    /// run once the effect is cancelled.
    #[cfg(target_arch = "wasm32")]
    pub fn launch_background<T, Work, Ui, Fut>(&self, work: Work, on_ui: Ui)
    where
        T: 'static,
        Work: FnOnce(CancelToken) -> Fut + 'static,
        Fut: Future<Output = T> + 'static,
        Ui: FnOnce(T) + 'static,
    {
        if !self.is_active() {
            return;
        }
        let token = CancelToken::new(Arc::clone(&self.active));
        self.launch(async move {
            let value = work(token.clone()).await;
            if token.is_active() {
                on_ui(value);
            }
        });
    }
}

#[derive(Clone)]
/// Cooperative cancellation token passed into background `LaunchedEffect` work.
///
/// The token flips to "cancelled" when the associated scope leaves composition.
/// Callers should periodically check [`CancelToken::is_cancelled`] in long-running
/// operations and exit early; blocking I/O will not be interrupted automatically.
pub struct CancelToken {
    active: Arc<AtomicBool>,
}

impl CancelToken {
    fn new(active: Arc<AtomicBool>) -> Self {
        Self { active }
    }

    /// Returns `true` once the associated scope has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        !self.active.load(Ordering::SeqCst)
    }

    /// Returns whether the scope is still active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

// Cancels the effect that `state` runs, if any. Opens the scope of the next.
// The old scope is cancelled outside the slot's borrow. Its cancellation
// drops its futures and what they hold.
fn restart(state: Owned<LaunchedEffectState>, runtime: RuntimeHandle) -> LaunchedEffectScope {
    let scope = LaunchedEffectScope::new(runtime);
    let previous = state.update(|state| state.scope.replace(scope.clone()));
    if let Some(previous) = previous {
        previous.cancel();
    }
    scope
}

/// Implements [`LaunchedEffect`] for a call site that has already computed its group key. Call
/// `LaunchedEffect` instead.
///
/// `site` is where the effect was written. It travels onto the task the effect
/// spawns. A task that the runtime still holds then names the effect that
/// started it.
#[doc(hidden)]
pub fn __launched_effect_impl<K, F, Fut>(group_key: Key, site: TaskSite, keys: K, block: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LaunchedEffectScope) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    with_current_composer(|composer| {
        let state = composer.remember_effect::<LaunchedEffectState>(group_key);
        if state.with(|state| state.should_run(&keys)) {
            let key = EffectKey::new(keys);
            state.update(|state| state.key = Some(key));
            let runtime = composer.runtime_handle();
            composer.register_side_effect(move || {
                let scope = restart(state, runtime);
                let future = Box::pin(block(scope.clone()));
                scope
                    .coroutines
                    .launch_labelled(future, TaskLabel::Site(site));
            });
        }
    });
}

/// Runs `block` as a coroutine when this call site first enters composition,
/// as Jetpack Compose's `LaunchedEffect` does. It runs `block` again whenever
/// `keys` no longer equals the value of the last run.
///
/// ```ignore
/// LaunchedEffect(line, async move |_| {
///     for typed in 1..=length {
///         delay(LETTER).await;
///         letters.set(typed);
///     }
/// });
/// ```
///
/// A change of `keys` cancels the running coroutine. The call site leaving the
/// composition cancels it too. The coroutine stops where it awaits. No line
/// after that `.await` runs. `keys` may be a tuple to depend on more than one
/// value, as Compose's `LaunchedEffect(vararg keys)` does.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn LaunchedEffect<K, F, Fut>(keys: K, block: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LaunchedEffectScope) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    __launched_effect_impl(
        crate::caller_location_key(),
        std::panic::Location::caller().into(),
        keys,
        block,
    );
}
