use crate::collections::map::HashMap;
use crate::collections::map::HashSet;
use crate::MutableStateInner;
use std::any::Any;
use std::cell::{Cell, Ref, RefCell};
use std::collections::{HashMap as StdHashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;
use std::thread_local;

use crate::frame_clock::FrameClock;
use crate::platform::RuntimeScheduler;
use crate::{Applier, Command, FrameCallbackId, NodeError, RecomposeScopeInner, ScopeId};

enum UiMessage {
    Task(Box<dyn FnOnce() + Send + 'static>),
    Invoke { id: u64, value: Box<dyn Any + Send> },
}

type UiContinuation = Box<dyn Fn(Box<dyn Any>) + 'static>;
type UiContinuationMap = HashMap<u64, UiContinuation>;

trait AnyStateCell {
    fn as_any(&self) -> &dyn Any;
}

struct TypedStateCell<T: Clone + 'static> {
    inner: MutableStateInner<T>,
}

impl<T: Clone + 'static> AnyStateCell for TypedStateCell<T> {
    fn as_any(&self) -> &dyn Any {
        &self.inner
    }
}

#[derive(Default)]
pub(crate) struct StateArena {
    cells: RefCell<Vec<Option<Box<dyn AnyStateCell>>>>,
}

impl StateArena {
    pub(crate) fn alloc<T: Clone + 'static>(&self, value: T, runtime: RuntimeHandle) -> StateId {
        let mut cells = self.cells.borrow_mut();
        let id = StateId(cells.len() as u32);
        let inner = MutableStateInner::new(value, runtime.clone());
        inner.install_snapshot_observer(id);
        let cell: Box<dyn AnyStateCell> = Box::new(TypedStateCell { inner });
        cells.push(Some(cell));
        id
    }

    fn get_cell(&self, id: StateId) -> Ref<'_, Box<dyn AnyStateCell>> {
        Ref::map(self.cells.borrow(), |cells| {
            cells
                .get(id.0 as usize)
                .and_then(|cell| cell.as_ref())
                .expect("state cell missing")
        })
    }

    pub(crate) fn get_typed<T: Clone + 'static>(
        &self,
        id: StateId,
    ) -> Ref<'_, MutableStateInner<T>> {
        Ref::map(self.get_cell(id), |cell| {
            cell.as_any()
                .downcast_ref::<MutableStateInner<T>>()
                .expect("state cell type mismatch")
        })
    }

    pub(crate) fn get_typed_opt<T: Clone + 'static>(
        &self,
        id: StateId,
    ) -> Option<Ref<'_, MutableStateInner<T>>> {
        Ref::filter_map(self.get_cell(id), |cell| {
            cell.as_any().downcast_ref::<MutableStateInner<T>>()
        })
        .ok()
    }
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static RUNTIME_HANDLES: RefCell<StdHashMap<RuntimeId, RuntimeHandle>> =
        RefCell::new(StdHashMap::new());
}

fn register_runtime_handle(handle: &RuntimeHandle) {
    RUNTIME_HANDLES.with(|registry| {
        registry.borrow_mut().insert(handle.id, handle.clone());
    });
}

pub(crate) fn runtime_handle_for(id: RuntimeId) -> Option<RuntimeHandle> {
    RUNTIME_HANDLES.with(|registry| registry.borrow().get(&id).cloned())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StateId(pub(crate) u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeId(u32);

impl RuntimeId {
    fn next() -> Self {
        static NEXT_RUNTIME_ID: AtomicU32 = AtomicU32::new(1);
        Self(NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed))
    }
}

struct UiDispatcherInner {
    scheduler: Arc<dyn RuntimeScheduler>,
    tx: mpsc::Sender<UiMessage>,
    pending: AtomicUsize,
}

impl UiDispatcherInner {
    fn new(scheduler: Arc<dyn RuntimeScheduler>, tx: mpsc::Sender<UiMessage>) -> Self {
        Self {
            scheduler,
            tx,
            pending: AtomicUsize::new(0),
        }
    }

    fn post(&self, task: impl FnOnce() + Send + 'static) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(UiMessage::Task(Box::new(task)));
        self.scheduler.schedule_frame();
    }

    fn post_invoke(&self, id: u64, value: Box<dyn Any + Send>) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(UiMessage::Invoke { id, value });
        self.scheduler.schedule_frame();
    }

    fn has_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }
}

struct PendingGuard<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> PendingGuard<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        Self { counter }
    }
}

impl<'a> Drop for PendingGuard<'a> {
    fn drop(&mut self) {
        let previous = self.counter.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous > 0, "UI dispatcher pending count underflowed");
    }
}

#[derive(Clone)]
pub struct UiDispatcher {
    inner: Arc<UiDispatcherInner>,
}

impl UiDispatcher {
    fn new(inner: Arc<UiDispatcherInner>) -> Self {
        Self { inner }
    }

    pub fn post(&self, task: impl FnOnce() + Send + 'static) {
        self.inner.post(task);
    }

    pub fn post_invoke<T>(&self, id: u64, value: T)
    where
        T: Send + 'static,
    {
        self.inner.post_invoke(id, Box::new(value));
    }

    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }
}

struct RuntimeInner {
    scheduler: Arc<dyn RuntimeScheduler>,
    needs_frame: RefCell<bool>,
    node_updates: RefCell<Vec<Command>>, // FUTURE(no_std): replace Vec with ring buffer.
    invalid_scopes: RefCell<HashSet<ScopeId>>, // FUTURE(no_std): replace HashSet with sparse bitset.
    scope_queue: RefCell<Vec<(ScopeId, Weak<RecomposeScopeInner>)>>, // FUTURE(no_std): use smallvec-backed queue.
    frame_callbacks: RefCell<VecDeque<FrameCallbackEntry>>, // FUTURE(no_std): migrate to ring buffer.
    next_frame_callback_id: Cell<u64>,
    ui_dispatcher: Arc<UiDispatcherInner>,
    ui_rx: RefCell<mpsc::Receiver<UiMessage>>,
    local_tasks: RefCell<VecDeque<Box<dyn FnOnce() + 'static>>>,
    ui_conts: RefCell<UiContinuationMap>,
    next_cont_id: Cell<u64>,
    ui_thread_id: ThreadId,
    tasks: RefCell<Vec<TaskEntry>>, // FUTURE(no_std): migrate to smallvec-backed storage.
    next_task_id: Cell<u64>,
    task_waker: RefCell<Option<Waker>>,
    state_arena: StateArena,
    runtime_id: RuntimeId,
}

struct TaskEntry {
    id: u64,
    future: Pin<Box<dyn Future<Output = ()> + 'static>>,
}

impl RuntimeInner {
    fn new(scheduler: Arc<dyn RuntimeScheduler>) -> Self {
        let (tx, rx) = mpsc::channel();
        let dispatcher = Arc::new(UiDispatcherInner::new(scheduler.clone(), tx));
        Self {
            scheduler,
            needs_frame: RefCell::new(false),
            node_updates: RefCell::new(Vec::new()),
            invalid_scopes: RefCell::new(HashSet::default()),
            scope_queue: RefCell::new(Vec::new()),
            frame_callbacks: RefCell::new(VecDeque::new()),
            next_frame_callback_id: Cell::new(1),
            ui_dispatcher: dispatcher,
            ui_rx: RefCell::new(rx),
            local_tasks: RefCell::new(VecDeque::new()),
            ui_conts: RefCell::new(UiContinuationMap::default()),
            next_cont_id: Cell::new(1),
            ui_thread_id: std::thread::current().id(),
            tasks: RefCell::new(Vec::new()),
            next_task_id: Cell::new(1),
            task_waker: RefCell::new(None),
            state_arena: StateArena::default(),
            runtime_id: RuntimeId::next(),
        }
    }

    fn init_task_waker(this: &Rc<Self>) {
        let weak = Rc::downgrade(this);
        let waker = RuntimeTaskWaker::new(weak).into_waker();
        *this.task_waker.borrow_mut() = Some(waker);
    }

    fn schedule(&self) {
        *self.needs_frame.borrow_mut() = true;
        self.scheduler.schedule_frame();
    }

    fn enqueue_update(&self, command: Command) {
        self.node_updates.borrow_mut().push(command);
    }

    fn take_updates(&self) -> Vec<Command> {
        // FUTURE(no_std): return stack-allocated smallvec.
        self.node_updates.borrow_mut().drain(..).collect()
    }

    fn has_updates(&self) -> bool {
        !self.node_updates.borrow().is_empty() || self.has_invalid_scopes()
    }

    fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        let mut invalid = self.invalid_scopes.borrow_mut();
        if invalid.insert(id) {
            self.scope_queue.borrow_mut().push((id, scope));
            self.schedule();
        }
    }

    fn mark_scope_recomposed(&self, id: ScopeId) {
        self.invalid_scopes.borrow_mut().remove(&id);
    }

    fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        // FUTURE(no_std): return iterator over small array storage.
        let mut queue = self.scope_queue.borrow_mut();
        if queue.is_empty() {
            return Vec::new();
        }
        let pending: Vec<_> = queue.drain(..).collect();
        drop(queue);
        let invalid = self.invalid_scopes.borrow();
        pending
            .into_iter()
            .filter(|(id, _)| invalid.contains(id))
            .collect()
    }

    fn has_invalid_scopes(&self) -> bool {
        !self.invalid_scopes.borrow().is_empty()
    }

    fn has_frame_callbacks(&self) -> bool {
        !self.frame_callbacks.borrow().is_empty()
    }

    /// Queues a closure that is already bound to the UI thread's local queue.
    ///
    /// The closure may capture `Rc`/`RefCell` values because it never leaves the
    /// runtime thread. Callers must only invoke this from the runtime thread.
    fn enqueue_ui_task(&self, task: Box<dyn FnOnce() + 'static>) {
        self.local_tasks.borrow_mut().push_back(task);
        self.schedule();
    }

    fn spawn_ui_task(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> u64 {
        let id = self.next_task_id.get();
        self.next_task_id.set(id + 1);
        self.tasks.borrow_mut().push(TaskEntry { id, future });
        self.schedule();
        id
    }

    fn cancel_task(&self, id: u64) {
        let mut tasks = self.tasks.borrow_mut();
        if tasks.iter().any(|entry| entry.id == id) {
            tasks.retain(|entry| entry.id != id);
        }
    }

    fn poll_async_tasks(&self) -> bool {
        let waker = match self.task_waker.borrow().as_ref() {
            Some(waker) => waker.clone(),
            None => return false,
        };
        let mut cx = Context::from_waker(&waker);
        let mut tasks_ref = self.tasks.borrow_mut();
        let tasks = std::mem::take(&mut *tasks_ref);
        drop(tasks_ref);
        let mut pending = Vec::with_capacity(tasks.len());
        let mut made_progress = false;
        for mut entry in tasks.into_iter() {
            match entry.future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    made_progress = true;
                }
                Poll::Pending => {
                    pending.push(entry);
                }
            }
        }
        if !pending.is_empty() {
            self.tasks.borrow_mut().extend(pending);
        }
        made_progress
    }

    fn drain_ui(&self) {
        loop {
            let mut executed = false;

            {
                let rx = &mut *self.ui_rx.borrow_mut();
                for message in rx.try_iter() {
                    executed = true;
                    let _guard = PendingGuard::new(&self.ui_dispatcher.pending);
                    match message {
                        UiMessage::Task(task) => {
                            task();
                        }
                        UiMessage::Invoke { id, value } => {
                            self.invoke_ui_cont(id, value);
                        }
                    }
                }
            }

            loop {
                let task = {
                    let mut local = self.local_tasks.borrow_mut();
                    local.pop_front()
                };

                match task {
                    Some(task) => {
                        executed = true;
                        task();
                    }
                    None => break,
                }
            }

            if self.poll_async_tasks() {
                executed = true;
            }

            if !executed {
                break;
            }
        }
    }

    fn has_pending_ui(&self) -> bool {
        let local_pending = self
            .local_tasks
            .try_borrow()
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true);

        let async_pending = self
            .tasks
            .try_borrow()
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true);

        local_pending || self.ui_dispatcher.has_pending() || async_pending
    }

    fn register_ui_cont<T: 'static>(&self, f: impl FnOnce(T) + 'static) -> u64 {
        debug_assert_eq!(
            std::thread::current().id(),
            self.ui_thread_id,
            "UI continuation registered off the runtime thread",
        );
        let id = self.next_cont_id.get();
        self.next_cont_id.set(id + 1);
        let callback = RefCell::new(Some(f));
        self.ui_conts.borrow_mut().insert(
            id,
            Box::new(move |value: Box<dyn Any>| {
                let slot = callback
                    .borrow_mut()
                    .take()
                    .expect("UI continuation invoked more than once");
                let value = value
                    .downcast::<T>()
                    .expect("UI continuation type mismatch");
                slot(*value);
            }),
        );
        id
    }

    fn invoke_ui_cont(&self, id: u64, value: Box<dyn Any + Send>) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.ui_thread_id,
            "UI continuation invoked off the runtime thread",
        );
        if let Some(callback) = self.ui_conts.borrow_mut().remove(&id) {
            let value: Box<dyn Any> = value;
            callback(value);
        }
    }

    fn cancel_ui_cont(&self, id: u64) {
        self.ui_conts.borrow_mut().remove(&id);
    }

    fn register_frame_callback(&self, callback: Box<dyn FnOnce(u64) + 'static>) -> FrameCallbackId {
        let id = self.next_frame_callback_id.get();
        self.next_frame_callback_id.set(id + 1);
        self.frame_callbacks
            .borrow_mut()
            .push_back(FrameCallbackEntry {
                id,
                callback: Some(callback),
            });
        self.schedule();
        id
    }

    fn cancel_frame_callback(&self, id: FrameCallbackId) {
        let mut callbacks = self.frame_callbacks.borrow_mut();
        if let Some(index) = callbacks.iter().position(|entry| entry.id == id) {
            callbacks.remove(index);
        }
        let callbacks_empty = callbacks.is_empty();
        drop(callbacks);
        let local_pending = self
            .local_tasks
            .try_borrow()
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true);
        let async_pending = self
            .tasks
            .try_borrow()
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true);
        if !self.has_invalid_scopes()
            && !self.has_updates()
            && callbacks_empty
            && !local_pending
            && !self.ui_dispatcher.has_pending()
            && !async_pending
        {
            *self.needs_frame.borrow_mut() = false;
        }
    }

    fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        let mut callbacks = self.frame_callbacks.borrow_mut();
        let mut pending: Vec<Box<dyn FnOnce(u64) + 'static>> = Vec::with_capacity(callbacks.len());
        while let Some(mut entry) = callbacks.pop_front() {
            if let Some(callback) = entry.callback.take() {
                pending.push(callback);
            }
        }
        drop(callbacks);
        for callback in pending {
            callback(frame_time_nanos);
        }
        if !self.has_invalid_scopes()
            && !self.has_updates()
            && !self.has_frame_callbacks()
            && !self.has_pending_ui()
        {
            *self.needs_frame.borrow_mut() = false;
        }
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Rc<RuntimeInner>, // FUTURE(no_std): replace Rc with arena-managed runtime storage.
}

impl Runtime {
    pub fn new(scheduler: Arc<dyn RuntimeScheduler>) -> Self {
        let inner = Rc::new(RuntimeInner::new(scheduler));
        RuntimeInner::init_task_waker(&inner);
        let runtime = Self { inner };
        register_runtime_handle(&runtime.handle());
        runtime
    }

    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle {
            inner: Rc::downgrade(&self.inner),
            dispatcher: UiDispatcher::new(self.inner.ui_dispatcher.clone()),
            ui_thread_id: self.inner.ui_thread_id,
            id: self.inner.runtime_id,
        }
    }

    pub fn has_updates(&self) -> bool {
        self.inner.has_updates()
    }

    pub fn needs_frame(&self) -> bool {
        *self.inner.needs_frame.borrow() || self.inner.ui_dispatcher.has_pending()
    }

    pub fn set_needs_frame(&self, value: bool) {
        *self.inner.needs_frame.borrow_mut() = value;
    }

    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.handle())
    }
}

#[derive(Default)]
pub struct DefaultScheduler;

impl RuntimeScheduler for DefaultScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
#[derive(Default)]
pub struct TestScheduler;

#[cfg(test)]
impl RuntimeScheduler for TestScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
pub struct TestRuntime {
    runtime: Runtime,
}

#[cfg(test)]
impl Default for TestRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl TestRuntime {
    pub fn new() -> Self {
        Self {
            runtime: Runtime::new(Arc::new(TestScheduler)),
        }
    }

    pub fn handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }
}

#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Weak<RuntimeInner>,
    dispatcher: UiDispatcher,
    ui_thread_id: ThreadId,
    id: RuntimeId,
}

pub struct TaskHandle {
    id: u64,
    runtime: RuntimeHandle,
}

impl RuntimeHandle {
    pub fn id(&self) -> RuntimeId {
        self.id
    }

    pub(crate) fn alloc_state<T: Clone + 'static>(&self, value: T) -> StateId {
        self.with_state_arena(|arena| arena.alloc(value, self.clone()))
    }

    pub(crate) fn with_state_arena<R>(&self, f: impl FnOnce(&StateArena) -> R) -> R {
        self.inner
            .upgrade()
            .map(|inner| f(&inner.state_arena))
            .unwrap_or_else(|| panic!("runtime dropped"))
    }

    pub fn schedule(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.schedule();
        }
    }

    pub fn enqueue_node_update(&self, command: Command) {
        if let Some(inner) = self.inner.upgrade() {
            inner.enqueue_update(command);
        }
    }

    /// Schedules work that must run on the runtime thread.
    ///
    /// The closure executes on the UI thread immediately when the runtime
    /// drains its local queue, so it may capture `Rc`/`RefCell` values. Calling
    /// this from any other thread is a logic error and will panic in debug
    /// builds via the inner assertion.
    pub fn enqueue_ui_task(&self, task: Box<dyn FnOnce() + 'static>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.enqueue_ui_task(task);
        } else {
            task();
        }
    }

    pub fn spawn_ui<F>(&self, fut: F) -> Option<TaskHandle>
    where
        F: Future<Output = ()> + 'static,
    {
        self.inner.upgrade().map(|inner| {
            let id = inner.spawn_ui_task(Box::pin(fut));
            TaskHandle {
                id,
                runtime: self.clone(),
            }
        })
    }

    pub fn cancel_task(&self, id: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_task(id);
        }
    }

    /// Enqueues work from any thread to run on the UI thread.
    ///
    /// The closure must be `Send` because it may cross threads before executing
    /// on the runtime thread. Use this when posting from background work.
    pub fn post_ui(&self, task: impl FnOnce() + Send + 'static) {
        self.dispatcher.post(task);
    }

    pub fn register_ui_cont<T: 'static>(&self, f: impl FnOnce(T) + 'static) -> Option<u64> {
        self.inner.upgrade().map(|inner| inner.register_ui_cont(f))
    }

    pub fn cancel_ui_cont(&self, id: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_ui_cont(id);
        }
    }

    pub fn drain_ui(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.drain_ui();
        }
    }

    pub fn has_pending_ui(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_pending_ui())
            .unwrap_or_else(|| self.dispatcher.has_pending())
    }

    pub fn register_frame_callback(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> Option<FrameCallbackId> {
        self.inner
            .upgrade()
            .map(|inner| inner.register_frame_callback(Box::new(callback)))
    }

    pub fn cancel_frame_callback(&self, id: FrameCallbackId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_frame_callback(id);
        }
    }

    pub fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.drain_frame_callbacks(frame_time_nanos);
        }
    }

    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.clone())
    }

    pub fn set_needs_frame(&self, value: bool) {
        if let Some(inner) = self.inner.upgrade() {
            *inner.needs_frame.borrow_mut() = value;
        }
    }

    pub(crate) fn take_updates(&self) -> Vec<Command> {
        // FUTURE(no_std): return iterator over static buffer.
        self.inner
            .upgrade()
            .map(|inner| inner.take_updates())
            .unwrap_or_default()
    }

    pub fn has_updates(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_updates())
            .unwrap_or(false)
    }

    pub(crate) fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.register_invalid_scope(id, scope);
        }
    }

    pub(crate) fn mark_scope_recomposed(&self, id: ScopeId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.mark_scope_recomposed(id);
        }
    }

    pub(crate) fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        // FUTURE(no_std): expose draining iterator without Vec allocation.
        self.inner
            .upgrade()
            .map(|inner| inner.take_invalidated_scopes())
            .unwrap_or_default()
    }

    pub fn has_invalid_scopes(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_invalid_scopes())
            .unwrap_or(false)
    }

    pub fn has_frame_callbacks(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_frame_callbacks())
            .unwrap_or(false)
    }

    pub fn assert_ui_thread(&self) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.ui_thread_id,
            "state mutated off the runtime's UI thread"
        );
    }

    pub fn dispatcher(&self) -> UiDispatcher {
        self.dispatcher.clone()
    }
}

impl TaskHandle {
    pub fn cancel(self) {
        self.runtime.cancel_task(self.id);
    }
}

pub(crate) struct FrameCallbackEntry {
    id: FrameCallbackId,
    callback: Option<Box<dyn FnOnce(u64) + 'static>>,
}

struct RuntimeTaskWaker {
    scheduler: Arc<dyn RuntimeScheduler>,
}

impl RuntimeTaskWaker {
    fn new(inner: Weak<RuntimeInner>) -> Self {
        // Extract the Arc<RuntimeScheduler> which IS Send+Sync
        // This way we can wake the runtime without storing the Rc::Weak
        let scheduler = inner
            .upgrade()
            .map(|rc| rc.scheduler.clone())
            .expect("RuntimeInner dropped before waker created");
        Self { scheduler }
    }

    fn into_waker(self) -> Waker {
        futures_task::waker(Arc::new(self))
    }
}

impl futures_task::ArcWake for RuntimeTaskWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.scheduler.schedule_frame();
    }
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static ACTIVE_RUNTIMES: RefCell<Vec<RuntimeHandle>> = const { RefCell::new(Vec::new()) }; // FUTURE(no_std): move to bounded stack storage.
    #[allow(clippy::missing_const_for_thread_local)]
    static LAST_RUNTIME: RefCell<Option<RuntimeHandle>> = const { RefCell::new(None) };
}

fn current_runtime_handle() -> Option<RuntimeHandle> {
    if let Some(handle) = ACTIVE_RUNTIMES.with(|stack| stack.borrow().last().cloned()) {
        return Some(handle);
    }
    LAST_RUNTIME.with(|slot| slot.borrow().clone())
}

pub(crate) fn push_active_runtime(handle: &RuntimeHandle) {
    ACTIVE_RUNTIMES.with(|stack| stack.borrow_mut().push(handle.clone()));
    LAST_RUNTIME.with(|slot| *slot.borrow_mut() = Some(handle.clone()));
}

pub(crate) fn pop_active_runtime() {
    ACTIVE_RUNTIMES.with(|stack| {
        stack.borrow_mut().pop();
    });
}

/// Schedule a new frame render using the most recently active runtime handle.
pub fn schedule_frame() {
    if let Some(handle) = current_runtime_handle() {
        handle.schedule();
        return;
    }
    panic!("no runtime available to schedule frame");
}

/// Schedule an in-place node update using the most recently active runtime.
pub fn schedule_node_update(
    update: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
) {
    let handle = current_runtime_handle().expect("no runtime available to schedule node update");
    let mut update_opt = Some(update);
    handle.enqueue_node_update(Box::new(move |applier: &mut dyn Applier| {
        if let Some(update) = update_opt.take() {
            return update(applier);
        }
        Ok(())
    }));
}
