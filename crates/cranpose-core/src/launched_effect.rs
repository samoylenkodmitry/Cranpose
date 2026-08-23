use crate::effect_key::EffectKey;
use crate::{with_current_composer, Key, RuntimeHandle, TaskHandle};
#[cfg(not(target_arch = "wasm32"))]
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct LaunchedEffectState {
    key: Option<EffectKey>,
    cancel: Option<LaunchedEffectCancellation>,
}

struct LaunchedEffectCancellation {
    #[cfg(not(target_arch = "wasm32"))]
    runtime: RuntimeHandle,
    active: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    continuations: Rc<RefCell<Vec<u64>>>,
}

/// Where an effect was written, carried onto the task it spawns.
///
/// A leak report that can only say "one task is still queued" names nothing:
/// every run prints the same number and no run says which effect it was. This
/// is what turns that count into a place to look, and it stays a pair of plain
/// fields so carrying it costs nothing until a task is actually spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSite {
    pub file: &'static str,
    pub line: u32,
}

impl TaskSite {
    /// The site a macro captured with `file!()` and `line!()`.
    pub const fn new(file: &'static str, line: u32) -> TaskSite {
        TaskSite { file, line }
    }
}

impl Default for TaskSite {
    fn default() -> TaskSite {
        TaskSite::new("unknown", 0)
    }
}

impl std::fmt::Display for TaskSite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.file, self.line)
    }
}

impl From<&'static std::panic::Location<'static>> for TaskSite {
    /// The site a `#[track_caller]` function was called from.
    fn from(location: &'static std::panic::Location<'static>) -> TaskSite {
        TaskSite::new(location.file(), location.line())
    }
}

#[derive(Default)]
struct LaunchedEffectAsyncState {
    key: Option<EffectKey>,
    cancel: Option<LaunchedEffectCancellation>,
    task: Option<TaskHandle>,
    site: TaskSite,
}

impl LaunchedEffectState {
    fn should_run(&self, key: &EffectKey) -> bool {
        match &self.key {
            Some(current) => key.differs_from(current),
            None => true,
        }
    }

    fn set_key(&mut self, key: EffectKey) {
        self.key = Some(key);
    }

    fn launch(
        &mut self,
        runtime: RuntimeHandle,
        effect: impl FnOnce(LaunchedEffectScope) + 'static,
    ) {
        self.cancel_current();
        let active = Arc::new(AtomicBool::new(true));
        #[cfg(not(target_arch = "wasm32"))]
        let continuations = Rc::new(RefCell::new(Vec::new()));
        self.cancel = Some(LaunchedEffectCancellation {
            #[cfg(not(target_arch = "wasm32"))]
            runtime: runtime.clone(),
            active: Arc::clone(&active),
            #[cfg(not(target_arch = "wasm32"))]
            continuations: Rc::clone(&continuations),
        });
        let scope = LaunchedEffectScope {
            active: Arc::clone(&active),
            runtime: runtime.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            continuations,
        };
        runtime.enqueue_ui_task(Box::new(move || effect(scope)));
    }

    fn cancel_current(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

impl LaunchedEffectCancellation {
    fn cancel(&self) {
        self.active.store(false, Ordering::SeqCst);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut pending = self.continuations.borrow_mut();
            for id in pending.drain(..) {
                self.runtime.cancel_ui_cont(id);
            }
        }
    }
}

impl LaunchedEffectAsyncState {
    fn should_run(&self, key: &EffectKey) -> bool {
        match &self.key {
            Some(current) => key.differs_from(current),
            None => true,
        }
    }

    fn set_key(&mut self, key: EffectKey) {
        self.key = Some(key);
    }

    fn set_site(&mut self, site: TaskSite) {
        self.site = site;
    }

    fn launch(
        &mut self,
        runtime: RuntimeHandle,
        mk_future: impl FnOnce(LaunchedEffectScope) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
    ) {
        self.cancel_current();
        let active = Arc::new(AtomicBool::new(true));
        #[cfg(not(target_arch = "wasm32"))]
        let continuations = Rc::new(RefCell::new(Vec::new()));
        self.cancel = Some(LaunchedEffectCancellation {
            #[cfg(not(target_arch = "wasm32"))]
            runtime: runtime.clone(),
            active: Arc::clone(&active),
            #[cfg(not(target_arch = "wasm32"))]
            continuations: Rc::clone(&continuations),
        });
        let scope = LaunchedEffectScope {
            active: Arc::clone(&active),
            runtime: runtime.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            continuations,
        };
        let future = mk_future(scope.clone());
        let active_flag = Arc::clone(&scope.active);
        crate::label_next_ui_task(self.site.to_string());
        match runtime.spawn_ui(async move {
            future.await;
            active_flag.store(false, Ordering::SeqCst);
        }) {
            Some(handle) => {
                self.task = Some(handle);
            }
            None => {
                active.store(false, Ordering::SeqCst);
                self.cancel = None;
            }
        }
    }

    fn cancel_current(&mut self) {
        if let Some(handle) = self.task.take() {
            handle.cancel();
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

impl Drop for LaunchedEffectState {
    fn drop(&mut self) {
        self.cancel_current();
    }
}

impl Drop for LaunchedEffectAsyncState {
    fn drop(&mut self) {
        self.cancel_current();
    }
}

#[derive(Clone)]
pub struct LaunchedEffectScope {
    active: Arc<AtomicBool>,
    runtime: RuntimeHandle,
    #[cfg(not(target_arch = "wasm32"))]
    continuations: Rc<RefCell<Vec<u64>>>,
}

impl LaunchedEffectScope {
    #[cfg(not(target_arch = "wasm32"))]
    fn track_continuation(&self, id: u64) {
        self.continuations.borrow_mut().push(id);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn release_continuation(&self, id: u64) {
        let mut continuations = self.continuations.borrow_mut();
        if let Some(index) = continuations.iter().position(|entry| *entry == id) {
            continuations.remove(index);
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn runtime(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    /// Runs a follow-up `LaunchedEffect` task on the UI thread.
    ///
    /// The provided closure executes on the runtime thread and may freely
    /// capture `Rc`/`RefCell` state. This must only be called from the UI
    /// thread, typically inside another effect callback.
    pub fn launch(&self, task: impl FnOnce(LaunchedEffectScope) + 'static) {
        if !self.is_active() {
            return;
        }
        let scope = self.clone();
        self.runtime.enqueue_ui_task(Box::new(move || {
            if scope.is_active() {
                task(scope);
            }
        }));
    }

    /// Posts UI-only work that will execute on the runtime thread.
    ///
    /// The closure never crosses threads, so it may capture non-`Send` values.
    /// Callers must invoke this from the UI thread.
    pub fn post_ui(&self, task: impl FnOnce() + 'static) {
        if !self.is_active() {
            return;
        }
        let active = Arc::clone(&self.active);
        self.runtime.enqueue_ui_task(Box::new(move || {
            if active.load(Ordering::SeqCst) {
                task();
            }
        }));
    }

    /// Runs background work and delivers results to the UI.
    ///
    /// On native targets, `work` runs on a worker thread and its future is
    /// driven to completion there. On WASM, `work` runs as a task on the
    /// browser event loop. The `on_ui` continuation always runs on the runtime
    /// thread, so it may capture `Rc`/`RefCell` state safely.
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
        let continuation_active = Arc::clone(&self.active);
        let id_cell = Rc::new(Cell::new(0));
        let id_for_closure = Rc::clone(&id_cell);
        let continuation = move |value: T| {
            let id = id_for_closure.get();
            continuation_scope.release_continuation(id);
            if continuation_active.load(Ordering::SeqCst) {
                on_ui(value);
            }
        };

        let Some(cont_id) = self.runtime.register_ui_cont(continuation) else {
            return;
        };
        id_cell.set(cont_id);
        self.track_continuation(cont_id);

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
    /// On native targets, `work` runs on a worker thread and its future is
    /// driven to completion there. On WASM, `work` runs as a task on the
    /// browser event loop. The `on_ui` continuation always runs on the runtime
    /// thread, so it may capture `Rc`/`RefCell` state safely.
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
        let active_for_task = Arc::clone(&self.active);
        let scope = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let token = CancelToken::new(Arc::clone(&active_for_task));
            let value = work(token.clone()).await;
            if token.is_cancelled() {
                return;
            }
            scope.post_ui(move || {
                if token.is_active() {
                    on_ui(value);
                }
            });
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

pub fn __launched_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LaunchedEffectScope) + 'static,
{
    // Create a group using the caller's location to ensure each LaunchedEffect
    // gets its own slot table entry, even in conditional branches
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key = EffectKey::new(keys);
            let state = composer.remember_effect::<LaunchedEffectState>();
            if state.with(|state| state.should_run(&key)) {
                state.update(|state| state.set_key(key));
                let runtime = composer.runtime_handle();
                let state_for_effect = state.clone();
                let mut effect_opt = Some(effect);
                composer.register_side_effect(move || {
                    if let Some(effect) = effect_opt.take() {
                        state_for_effect.update(|state| state.launch(runtime.clone(), effect));
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! LaunchedEffect {
    ($keys:expr, $effect:expr) => {
        $crate::__launched_effect_impl(
            $crate::location_key(file!(), line!(), column!()),
            $keys,
            $effect,
        )
    };
}

/// `site` is where the effect was written. It travels onto the task the effect
/// spawns, so a task the runtime is still holding says which effect started it.
pub fn __launched_effect_async_impl<K, F>(group_key: Key, site: TaskSite, keys: K, mk_future: F)
where
    K: PartialEq + 'static,
    F: FnOnce(LaunchedEffectScope) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
{
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key = EffectKey::new(keys);
            let state = composer.remember_effect::<LaunchedEffectAsyncState>();
            if state.with(|state| state.should_run(&key)) {
                state.update(|state| {
                    state.set_key(key);
                    state.set_site(site);
                });
                let runtime = composer.runtime_handle();
                let state_for_effect = state.clone();
                let mut mk_future_opt = Some(mk_future);
                composer.register_side_effect(move || {
                    if let Some(mk_future) = mk_future_opt.take() {
                        state_for_effect.update(|state| {
                            state.launch(runtime.clone(), mk_future);
                        });
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! LaunchedEffectAsync {
    ($keys:expr, $future:expr) => {
        $crate::__launched_effect_async_impl(
            $crate::location_key(file!(), line!(), column!()),
            $crate::TaskSite::new(file!(), line!()),
            $keys,
            $future,
        )
    };
}
