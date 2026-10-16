use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread::ThreadId,
};

#[cfg(any(feature = "internal", test))]
use crate::frame_clock::FrameClock;
use crate::{
    Applier, Command, FrameCallbackId, Key, MutableStateInner, NodeError, RecomposeScope,
    RecomposeScopeInner, ScopeId,
    collections::map::HashMap,
    platform::{RuntimeScheduler, SchedulerRef},
    state::{MutationPolicy, NeverEqual},
    thread_state,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameCallbackKind {
    Transient,
    Perpetual,
}

enum UiMessage {
    Task(Box<dyn FnOnce() + Send + 'static>),
    Invoke { id: u64, value: Box<dyn Any + Send> },
}

type UiContinuation = Box<dyn Fn(Box<dyn Any>) -> bool + 'static>;
type UiContinuationMap = HashMap<u64, UiContinuation>;

struct TypedStateCell<T: Clone + 'static> {
    inner: MutableStateInner<T>,
}

trait ScopeWatchCell {
    fn unregister_scope(&self, scope_id: ScopeId);
    fn forget_scope(&self, scope_id: ScopeId);
}

impl<T: Clone + 'static> ScopeWatchCell for TypedStateCell<T> {
    fn unregister_scope(&self, scope_id: ScopeId) {
        self.inner.unregister_scope(scope_id);
    }

    fn forget_scope(&self, scope_id: ScopeId) {
        self.inner.forget_scope(scope_id);
    }
}

struct StateArenaSlot {
    generation: u32,
    cell: Option<Rc<dyn Any>>,
    watcher_cell: Option<Rc<dyn ScopeWatchCell>>,
    lease: Option<Weak<StateHandleLease>>,
}

#[derive(Default)]
struct StateArenaInner {
    cells: Vec<StateArenaSlot>,
    free: Vec<u32>,
    /// The derived states among the cells, by slot.
    derived: HashMap<u32, Weak<dyn DerivedRefresh>>,
}

/// A derived state that brings its value up to date before a read.
pub(crate) trait DerivedRefresh {
    /// Derives the value again when a state it read changed since the
    /// recomposition that last derived it.
    fn refresh(&self);
}

/// Sizes of a runtime's state arena, for leak and memory checks. A `_len` field counts entries and
/// a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateArenaDebugStats {
    /// State cells, both in use and free.
    pub cells_len: usize,
    /// Capacity reserved for the state cells.
    pub cells_cap: usize,
    /// State cells free for reuse.
    pub free_len: usize,
    /// Capacity reserved for the free list.
    pub free_cap: usize,
}

/// Sizes of a runtime's queues and tables, for leak and memory checks. A `_len` field counts
/// entries and a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeDebugStats {
    /// Node updates queued and not yet applied.
    pub node_updates_len: usize,
    /// Capacity reserved for the node updates.
    pub node_updates_cap: usize,
    /// Recompose scopes that were invalidated and not yet recomposed.
    pub invalid_scopes_len: usize,
    /// Entries in the queue of invalidated scopes.
    pub scope_queue_len: usize,
    /// Capacity reserved for the invalid scope queue.
    pub scope_queue_cap: usize,
    /// Frame callbacks waiting for the next frame.
    pub frame_callbacks_len: usize,
    /// Capacity reserved for the frame callbacks.
    pub frame_callbacks_cap: usize,
    /// UI-thread closures queued by `enqueue_ui_task` and not yet run.
    pub local_tasks_len: usize,
    /// Capacity reserved for the local tasks.
    pub local_tasks_cap: usize,
    /// UI continuations registered and waiting for a value.
    pub ui_conts_len: usize,
    /// Capacity reserved for the UI continuations.
    pub ui_conts_cap: usize,
    /// Futures from `spawn_ui` that have not finished.
    pub tasks_len: usize,
    /// Capacity reserved for the spawned tasks.
    pub tasks_cap: usize,
    /// States kept alive on behalf of the runtime itself, not of a Rust owner.
    pub external_state_owners_len: usize,
    /// Capacity reserved for the external state owners.
    pub external_state_owners_cap: usize,
    /// Messages posted from other threads that the UI thread has not run yet.
    pub ui_dispatcher_pending: usize,
}

#[derive(Default)]
pub(crate) struct StateArena {
    inner: RefCell<StateArenaInner>,
}

impl StateArena {
    pub(crate) fn alloc<T: Clone + 'static>(&self, value: T, runtime: RuntimeHandle) -> StateId {
        self.alloc_with_policy(value, runtime, Rc::new(NeverEqual))
    }

    pub(crate) fn alloc_with_policy<T: Clone + 'static>(
        &self,
        value: T,
        runtime: RuntimeHandle,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> StateId {
        let (slot, generation) = {
            let mut inner = self.inner.borrow_mut();
            loop {
                let Some(slot) = inner.free.pop() else {
                    let slot = inner.cells.len() as u32;
                    inner.cells.push(StateArenaSlot {
                        generation: 0,
                        cell: None,
                        watcher_cell: None,
                        lease: None,
                    });
                    break (slot, 0);
                };

                let Some(entry) = inner.cells.get_mut(slot as usize) else {
                    continue;
                };
                if entry.cell.is_some() {
                    continue;
                }

                entry.watcher_cell = None;
                entry.lease = None;
                entry.generation = entry.generation.wrapping_add(1);
                break (slot, entry.generation);
            }
        };
        let id = StateId::new(slot, generation);
        let inner = MutableStateInner::new_with_policy(value, runtime, policy);
        inner.install_snapshot_observer(id);
        let typed_cell = Rc::new(TypedStateCell { inner });
        let cell: Rc<dyn Any> = typed_cell.clone();
        let watcher_cell: Rc<dyn ScopeWatchCell> = typed_cell;
        let mut arena = self.inner.borrow_mut();
        let slot_entry = &mut arena.cells[slot as usize];
        slot_entry.cell = Some(cell);
        slot_entry.watcher_cell = Some(watcher_cell);
        id
    }

    fn get_cell_opt(&self, id: StateId) -> Option<Rc<dyn Any>> {
        self.inner
            .borrow()
            .cells
            .get(id.slot_index())
            .filter(|cell| cell.generation == id.generation())
            .and_then(|cell| cell.cell.as_ref())
            .cloned()
    }

    fn get_typed<T: Clone + 'static>(&self, id: StateId) -> Rc<TypedStateCell<T>> {
        match self.get_cell_opt(id) {
            None => panic!(
                "state cell missing: slot={}, gen={}, expected={}",
                id.slot(),
                id.generation(),
                std::any::type_name::<T>(),
            ),
            Some(cell) => Rc::downcast::<TypedStateCell<T>>(cell).unwrap_or_else(|_| {
                panic!(
                    "state cell type mismatch: slot={}, gen={}, expected={}",
                    id.slot(),
                    id.generation(),
                    std::any::type_name::<T>(),
                )
            }),
        }
    }

    fn get_typed_opt<T: Clone + 'static>(&self, id: StateId) -> Option<Rc<TypedStateCell<T>>> {
        Rc::downcast::<TypedStateCell<T>>(self.get_cell_opt(id)?).ok()
    }

    pub(crate) fn with_typed<T: Clone + 'static, R>(
        &self,
        id: StateId,
        f: impl FnOnce(&MutableStateInner<T>) -> R,
    ) -> R {
        let cell = self.get_typed::<T>(id);
        f(&cell.inner)
    }

    pub(crate) fn with_typed_opt<T: Clone + 'static, R>(
        &self,
        id: StateId,
        f: impl FnOnce(&MutableStateInner<T>) -> R,
    ) -> Option<R> {
        let cell = self.get_typed_opt::<T>(id)?;
        Some(f(&cell.inner))
    }

    pub(crate) fn release(&self, id: StateId) {
        let cell = {
            let mut inner = self.inner.borrow_mut();
            let Some(slot) = inner.cells.get_mut(id.slot_index()) else {
                return;
            };
            if slot.generation != id.generation() {
                return;
            }
            slot.lease = None;
            slot.watcher_cell = None;
            let cell = slot.cell.take();
            if cell.is_some() {
                inner.free.push(id.slot());
                if !inner.derived.is_empty() {
                    inner.derived.remove(&id.slot());
                }
            }
            cell
        };
        drop(cell);
    }

    /// Makes `refresh` what brings the derived state `id` up to date before
    /// each read, until the state is released.
    pub(crate) fn register_derived(&self, id: StateId, refresh: Weak<dyn DerivedRefresh>) {
        self.inner.borrow_mut().derived.insert(id.slot(), refresh);
    }

    /// Brings the derived state `id` up to date before a read; see
    /// [`DerivedRefresh`].
    #[cold]
    pub(crate) fn refresh_derived(&self, id: StateId) {
        let refresh = {
            let inner = self.inner.borrow();
            inner
                .cells
                .get(id.slot_index())
                .filter(|cell| cell.generation == id.generation())
                .and_then(|_| inner.derived.get(&id.slot()))
                .and_then(Weak::upgrade)
        };
        if let Some(refresh) = refresh {
            refresh.refresh();
        }
    }

    pub(crate) fn stats(&self) -> (usize, usize) {
        let inner = self.inner.borrow();
        (inner.cells.len(), inner.free.len())
    }

    pub(crate) fn debug_stats(&self) -> StateArenaDebugStats {
        let inner = self.inner.borrow();
        StateArenaDebugStats {
            cells_len: inner.cells.len(),
            cells_cap: inner.cells.capacity(),
            free_len: inner.free.len(),
            free_cap: inner.free.capacity(),
        }
    }

    pub(crate) fn unregister_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(watcher_cell) = self.watcher_cell(id) {
            watcher_cell.unregister_scope(scope_id);
        }
    }

    /// Stops the state `id` from recomposing the live scope `scope_id`.
    pub(crate) fn forget_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(watcher_cell) = self.watcher_cell(id) {
            watcher_cell.forget_scope(scope_id);
        }
    }

    fn watcher_cell(&self, id: StateId) -> Option<Rc<dyn ScopeWatchCell>> {
        let inner = self.inner.borrow();
        inner
            .cells
            .get(id.slot_index())
            .filter(|slot| slot.generation == id.generation())
            .and_then(|slot| slot.watcher_cell.as_ref())
            .cloned()
    }

    pub(crate) fn register_lease(&self, id: StateId, lease: &Rc<StateHandleLease>) {
        let mut inner = self.inner.borrow_mut();
        let Some(slot) = inner.cells.get_mut(id.slot_index()) else {
            return;
        };
        if slot.generation != id.generation() {
            return;
        }
        slot.lease = Some(Rc::downgrade(lease));
    }

    pub(crate) fn retain_lease(&self, id: StateId) -> Option<Rc<StateHandleLease>> {
        let inner = self.inner.borrow();
        let slot = inner.cells.get(id.slot_index())?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.lease.as_ref()?.upgrade()
    }
}

/// Names one state cell of a runtime. A generation count makes the id of a freed cell stale.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StateId {
    slot: u32,
    generation: u32,
}

impl StateId {
    const fn new(slot: u32, generation: u32) -> Self {
        Self { slot, generation }
    }

    pub(crate) const fn slot(self) -> u32 {
        self.slot
    }

    pub(crate) const fn slot_index(self) -> usize {
        self.slot as usize
    }

    pub(crate) const fn generation(self) -> u32 {
        self.generation
    }
}

/// Identifies a runtime within its thread.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeId(u32);

impl RuntimeId {
    fn next() -> Self {
        thread_state::runtimes(|state| {
            let id = state.next_runtime_id.get();
            state.next_runtime_id.set(id.wrapping_add(1));
            Self(id)
        })
    }
}

struct UiDispatcherInner {
    scheduler: SchedulerRef,
    tx: mpsc::Sender<UiMessage>,
    pending: AtomicUsize,
}

#[cfg(not(target_arch = "wasm32"))]
type UiDispatcherRef = Arc<UiDispatcherInner>;

#[cfg(target_arch = "wasm32")]
type UiDispatcherRef = Rc<UiDispatcherInner>;

impl UiDispatcherInner {
    fn new(scheduler: SchedulerRef, tx: mpsc::Sender<UiMessage>) -> Self {
        Self {
            scheduler,
            tx,
            pending: AtomicUsize::new(0),
        }
    }

    fn post(&self, task: impl FnOnce() + Send + 'static) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.tx.send(UiMessage::Task(Box::new(task))).is_ok() {
            self.scheduler.schedule_frame();
        } else {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn post_invoke(&self, id: u64, value: Box<dyn Any + Send>) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.tx.send(UiMessage::Invoke { id, value }).is_ok() {
            self.scheduler.schedule_frame();
        } else {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
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

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        let mut current = self.counter.load(Ordering::SeqCst);
        loop {
            if current == 0 {
                return;
            }
            match self.counter.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }
}

/// Posts work from any thread to the UI thread of a runtime.
#[derive(Clone)]
pub struct UiDispatcher {
    inner: UiDispatcherRef,
}

impl UiDispatcher {
    fn new(inner: UiDispatcherRef) -> Self {
        Self { inner }
    }

    /// Queues `task` to run on the UI thread the next time the runtime drains UI work. May be
    /// called from any thread.
    pub fn post(&self, task: impl FnOnce() + Send + 'static) {
        self.inner.post(task);
    }

    /// Queues a call of the continuation registered under `id` with `value`, to run on the UI
    /// thread. May be called from any thread.
    pub fn post_invoke<T>(&self, id: u64, value: T)
    where
        T: Send + 'static,
    {
        self.inner.post_invoke(id, Box::new(value));
    }

    /// Returns `true` if a posted message has not run yet.
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }
}

struct RuntimeInner {
    scheduler: SchedulerRef,
    needs_frame: RefCell<bool>,
    node_updates: RefCell<Vec<Command>>,
    invalid_scope_count: Cell<usize>,
    scope_queue: RefCell<Vec<Weak<RecomposeScopeInner>>>,
    /// The storage of the last pass's scope list, for the next pass.
    spare_scopes: RefCell<Vec<RecomposeScope>>,
    frame_callbacks: RefCell<VecDeque<FrameCallbackEntry>>,
    next_frame_callback_id: Cell<u64>,
    last_frame_time_nanos: Cell<Option<u64>>,
    ui_dispatcher: UiDispatcherRef,
    ui_rx: RefCell<mpsc::Receiver<UiMessage>>,
    local_tasks: RefCell<VecDeque<Box<dyn FnOnce() + 'static>>>,
    ui_conts: RefCell<UiContinuationMap>,
    next_cont_id: Cell<u64>,
    ui_thread_id: ThreadId,
    tasks: RefCell<HashMap<u64, TaskEntry>>,
    task_order: RefCell<Vec<u64>>,
    /// Set by every task wake after the task's own flag, and cleared by the
    /// poll that then looks at the tasks: a poll after no wake skips them.
    task_woken: Arc<AtomicBool>,
    next_task_id: Cell<u64>,
    state_arena: StateArena,
    external_state_owners: RefCell<HashMap<StateId, Rc<StateHandleLease>>>,
    live_recompose_scope_count: Cell<usize>,
    forgotten_movables: RefCell<Vec<Key>>,
    next_movable_content_id: Cell<u64>,
    runtime_id: RuntimeId,
    #[cfg(target_os = "android")]
    rescued: RefCell<Option<Rc<thread_state::RescuedThreadState>>>,
}

struct TaskEntry {
    label: String,
    future: Option<Pin<Box<dyn Future<Output = ()> + 'static>>>,
    runnable: Arc<AtomicBool>,
    waker: Waker,
}

/// Names the next task that `spawn_ui` creates on this thread, for `live_ui_task_labels` and leak
/// reports. A task with no label is called `unnamed`.
pub fn label_next_ui_task(label: impl Into<String>) {
    thread_state::watch_thread_exit();
    let label = label.into();
    thread_state::runtimes(|state| *state.next_task_label.borrow_mut() = Some(label));
}

impl RuntimeInner {
    fn new(scheduler: SchedulerRef) -> Self {
        let (tx, rx) = mpsc::channel();
        let dispatcher = UiDispatcherRef::new(UiDispatcherInner::new(scheduler.clone(), tx));
        Self {
            scheduler,
            needs_frame: RefCell::new(false),
            node_updates: RefCell::new(Vec::new()),
            invalid_scope_count: Cell::new(0),
            scope_queue: RefCell::new(Vec::new()),
            spare_scopes: RefCell::new(Vec::new()),
            frame_callbacks: RefCell::new(VecDeque::new()),
            next_frame_callback_id: Cell::new(1),
            last_frame_time_nanos: Cell::new(None),
            ui_dispatcher: dispatcher,
            ui_rx: RefCell::new(rx),
            local_tasks: RefCell::new(VecDeque::new()),
            ui_conts: RefCell::new(UiContinuationMap::default()),
            next_cont_id: Cell::new(1),
            ui_thread_id: std::thread::current().id(),
            tasks: RefCell::new(HashMap::default()),
            task_order: RefCell::new(Vec::new()),
            task_woken: Arc::new(AtomicBool::new(false)),
            next_task_id: Cell::new(1),
            state_arena: StateArena::default(),
            external_state_owners: RefCell::new(HashMap::default()),
            live_recompose_scope_count: Cell::new(0),
            forgotten_movables: RefCell::new(Vec::new()),
            next_movable_content_id: Cell::new(1),
            runtime_id: RuntimeId::next(),
            #[cfg(target_os = "android")]
            rescued: RefCell::new(None),
        }
    }

    fn schedule(&self) {
        *self.needs_frame.borrow_mut() = true;
        self.scheduler.schedule_frame();
    }

    fn enqueue_update(&self, command: Command) {
        self.node_updates.borrow_mut().push(command);
        self.schedule();
    }

    fn take_updates(&self) -> Vec<Command> {
        self.node_updates.borrow_mut().drain(..).collect::<Vec<_>>()
    }

    fn has_updates(&self) -> bool {
        !self.node_updates.borrow().is_empty() || self.has_invalid_scopes()
    }

    fn register_invalid_scope(&self, scope: Weak<RecomposeScopeInner>) {
        self.invalid_scope_count
            .set(self.invalid_scope_count.get() + 1);
        self.scope_queue.borrow_mut().push(scope);
        self.schedule();
    }

    fn requeue_invalid_scope(&self, scope: &RecomposeScope) {
        if scope.is_enqueued() {
            self.scope_queue.borrow_mut().push(scope.downgrade());
            self.schedule();
        }
    }

    fn mark_scope_recomposed(&self) {
        self.invalid_scope_count
            .set(self.invalid_scope_count.get().saturating_sub(1));
    }

    fn take_invalidated_scopes(&self) -> Option<Vec<RecomposeScope>> {
        // An empty queue keeps its storage for the next invalidation.
        let mut pending = {
            let mut queue = self.scope_queue.borrow_mut();
            if queue.is_empty() {
                return None;
            }
            std::mem::take(&mut *queue)
        };
        let mut scopes = std::mem::take(&mut *self.spare_scopes.borrow_mut());
        scopes.extend(
            pending
                .iter()
                .filter_map(RecomposeScope::upgrade)
                .filter(RecomposeScope::is_enqueued),
        );
        pending.clear();
        let mut queue = self.scope_queue.borrow_mut();
        if queue.is_empty() {
            std::mem::swap(&mut *queue, &mut pending);
        }
        drop(queue);
        if scopes.is_empty() {
            self.recycle_scopes(scopes);
            return None;
        }
        Some(scopes)
    }

    fn recycle_scopes(&self, mut scopes: Vec<RecomposeScope>) {
        // Cleared before the borrow: dropping a scope may run any code.
        scopes.clear();
        let mut spare = self.spare_scopes.borrow_mut();
        if spare.capacity() < scopes.capacity() {
            *spare = scopes;
        }
    }

    fn has_invalid_scopes(&self) -> bool {
        self.invalid_scope_count.get() != 0
    }

    fn queued_invalid_scope_ids(&self) -> Vec<ScopeId> {
        let mut ids: Vec<ScopeId> = self
            .scope_queue
            .borrow()
            .iter()
            .filter_map(RecomposeScope::upgrade)
            .filter(RecomposeScope::is_enqueued)
            .map(|scope| scope.id())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn increment_live_recompose_scope_count(&self) {
        self.live_recompose_scope_count
            .set(self.live_recompose_scope_count.get().saturating_add(1));
    }

    fn decrement_live_recompose_scope_count(&self) {
        self.live_recompose_scope_count
            .set(self.live_recompose_scope_count.get().saturating_sub(1));
    }

    fn live_recompose_scope_count(&self) -> usize {
        self.live_recompose_scope_count.get()
    }

    fn has_frame_callbacks(&self) -> bool {
        !self.frame_callbacks.borrow().is_empty()
    }

    fn has_transient_frame_callbacks(&self) -> bool {
        self.frame_callbacks
            .borrow()
            .iter()
            .any(|entry| entry.kind == FrameCallbackKind::Transient)
    }

    fn enqueue_ui_task(&self, task: Box<dyn FnOnce() + 'static>) {
        self.local_tasks.borrow_mut().push_back(task);
        self.schedule();
    }

    fn spawn_ui_task(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> u64 {
        let id = self.next_task_id.get();
        self.next_task_id.set(id + 1);
        let label = thread_state::runtimes(|state| state.next_task_label.borrow_mut().take())
            .unwrap_or_else(|| "unnamed".to_string());
        let runnable = Arc::new(AtomicBool::new(true));
        self.task_woken.store(true, Ordering::Release);
        let waker = RuntimeTaskWaker::new(self, Arc::clone(&runnable)).into_waker();
        self.tasks.borrow_mut().insert(
            id,
            TaskEntry {
                label,
                future: Some(future),
                runnable,
                waker,
            },
        );
        self.task_order.borrow_mut().push(id);
        self.schedule();
        id
    }

    fn cancel_task(&self, id: u64) {
        let task = self.tasks.borrow_mut().remove(&id);
        self.task_order.borrow_mut().retain(|queued| *queued != id);
        drop(task);
    }

    fn has_task(&self, id: u64) -> bool {
        self.tasks
            .try_borrow()
            .map_or(true, |tasks| tasks.contains_key(&id))
    }

    fn poll_async_tasks(&self) -> bool {
        // Without a wake no task can make progress, and the order stays.
        if !self.task_woken.load(Ordering::Acquire) {
            return false;
        }
        // The tasks still pending move to the front of `order`, which goes
        // back as the task order, after any task spawned while polling.
        let mut order = std::mem::take(&mut *self.task_order.borrow_mut());
        // An empty order is a poll nested in another, which holds the order:
        // the wake flag stays for the polls after it.
        if order.is_empty() || !self.task_woken.swap(false, Ordering::AcqRel) {
            let mut task_order = self.task_order.borrow_mut();
            if task_order.is_empty() {
                *task_order = order;
            } else {
                task_order.extend(order);
            }
            return false;
        }
        let mut pending = 0;
        let mut made_progress = false;
        for index in 0..order.len() {
            let id = order[index];
            let task = {
                let mut tasks = self.tasks.borrow_mut();
                let Some(entry) = tasks.get_mut(&id) else {
                    continue;
                };
                if entry.runnable.swap(false, Ordering::AcqRel) {
                    entry
                        .future
                        .take()
                        .map(|future| (future, entry.waker.clone()))
                } else {
                    None
                }
            };
            let Some((mut future, waker)) = task else {
                order[pending] = id;
                pending += 1;
                continue;
            };
            let mut cx = Context::from_waker(&waker);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    self.cancel_task(id);
                    made_progress = true;
                }
                Poll::Pending => {
                    let mut tasks = self.tasks.borrow_mut();
                    if let Some(entry) = tasks.get_mut(&id) {
                        entry.future = Some(future);
                        order[pending] = id;
                        pending += 1;
                    } else {
                        drop(tasks);
                        drop(future);
                    }
                }
            }
        }
        order.truncate(pending);
        order.retain(|id| self.has_task(*id));
        let mut task_order = self.task_order.borrow_mut();
        if task_order.is_empty() {
            *task_order = order;
        } else {
            task_order.extend(order);
        }
        made_progress
    }

    fn drain_ui(&self) {
        #[cfg(target_os = "emscripten")]
        crate::concurrency::fire_due_alarms();
        loop {
            let mut executed = false;

            // A message counts as pending from before it is sent until it
            // ran, so none waits while none is pending.
            if self.ui_dispatcher.has_pending() {
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

            if self.run_local_tasks() {
                executed = true;
            }

            if self.poll_async_tasks() {
                executed = true;
            }
            // The tasks just polled may have queued work of their own, such
            // as a state observer's change notice for a state they wrote. It
            // belongs to this drain, or a frame that resumed an animation
            // would draw the value it wrote only on the next frame. It does
            // not count as progress: a task writing state on every poll must
            // not keep the drain polling it.
            self.run_local_tasks();

            if !executed {
                break;
            }
        }

        self.clear_needs_frame_if_idle();
    }

    fn run_local_tasks(&self) -> bool {
        let mut ran = false;
        loop {
            let task = self.local_tasks.borrow_mut().pop_front();
            let Some(task) = task else {
                return ran;
            };
            ran = true;
            task();
        }
    }

    fn has_pending_ui(&self) -> bool {
        let local_pending = self
            .local_tasks
            .try_borrow()
            .map_or(true, |tasks| !tasks.is_empty());

        local_pending || self.ui_dispatcher.has_pending() || self.has_runnable_tasks()
    }

    /// A finished task's waker may still wake after the task is gone; with
    /// no task left that wake is stale.
    fn has_runnable_tasks(&self) -> bool {
        self.task_woken.load(Ordering::Acquire)
            && self
                .tasks
                .try_borrow()
                .map_or(true, |tasks| !tasks.is_empty())
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
                let Ok(value) = value.downcast::<T>() else {
                    return false;
                };
                let Some(slot) = callback.borrow_mut().take() else {
                    return true;
                };
                slot(*value);
                true
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
        let callback = { self.ui_conts.borrow_mut().remove(&id) };
        if let Some(callback) = callback {
            let value: Box<dyn Any> = value;
            if !callback(value) {
                self.ui_conts.borrow_mut().insert(id, callback);
            }
        }
    }

    fn cancel_ui_cont(&self, id: u64) {
        let continuation = self.ui_conts.borrow_mut().remove(&id);
        drop(continuation);
    }

    fn register_frame_callback(
        &self,
        kind: FrameCallbackKind,
        callback: FrameCallback,
    ) -> FrameCallbackId {
        let id = self.next_frame_callback_id.get();
        self.next_frame_callback_id.set(id + 1);
        self.frame_callbacks
            .borrow_mut()
            .push_back(FrameCallbackEntry { id, kind, callback });
        self.schedule();
        id
    }

    /// The frame time once the frame callback `id` has run. A waiting
    /// task's entry takes `waker` when the one it holds would wake another
    /// task.
    #[cfg(any(feature = "internal", test))]
    fn frame_waker_time(&self, id: FrameCallbackId, waker: &Waker) -> Option<u64> {
        let mut callbacks = self.frame_callbacks.borrow_mut();
        let Some(entry) = callbacks.iter_mut().find(|entry| entry.id == id) else {
            return self.last_frame_time_nanos.get();
        };
        if let FrameCallback::Wake(held) = &mut entry.callback
            && !held.will_wake(waker)
        {
            held.clone_from(waker);
        }
        None
    }

    fn cancel_frame_callback(&self, id: FrameCallbackId) {
        let removed = {
            let mut callbacks = self.frame_callbacks.borrow_mut();
            callbacks
                .iter()
                .position(|entry| entry.id == id)
                .and_then(|index| callbacks.remove(index))
        };
        drop(removed);
        self.clear_needs_frame_if_idle();
    }

    fn clear_needs_frame_if_idle(&self) {
        if !self.has_invalid_scopes()
            && !self.has_updates()
            && !self.has_frame_callbacks()
            && !self.has_pending_ui()
        {
            *self.needs_frame.borrow_mut() = false;
        }
    }

    fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        if self
            .last_frame_time_nanos
            .get()
            .is_some_and(|previous| frame_time_nanos <= previous)
        {
            return;
        }
        self.last_frame_time_nanos.set(Some(frame_time_nanos));
        let next_frame_id = self.next_frame_callback_id.get();
        let drain = || {
            loop {
                let entry = {
                    let mut callbacks = self.frame_callbacks.borrow_mut();
                    if callbacks
                        .front()
                        .is_some_and(|entry| entry.id < next_frame_id)
                    {
                        callbacks.pop_front()
                    } else {
                        None
                    }
                };
                let Some(entry) = entry else {
                    break;
                };
                match entry.callback {
                    FrameCallback::Run(callback) => callback(frame_time_nanos),
                    #[cfg(any(feature = "internal", test))]
                    FrameCallback::Wake(waker) => waker.wake(),
                }
            }
        };
        // Callbacks write state in one snapshot. Waking a task writes none:
        // a frame of only waiting tasks opens no snapshot.
        let runs_callbacks = self
            .frame_callbacks
            .borrow()
            .iter()
            .any(|entry| matches!(entry.callback, FrameCallback::Run(_)));
        if runs_callbacks {
            let _ = crate::run_in_mutable_snapshot(drain);
        } else {
            drain();
        }

        self.clear_needs_frame_if_idle();
    }

    fn debug_stats(&self) -> RuntimeDebugStats {
        let node_updates = self.node_updates.borrow();
        let scope_queue = self.scope_queue.borrow();
        let frame_callbacks = self.frame_callbacks.borrow();
        let local_tasks = self.local_tasks.borrow();
        let ui_conts = self.ui_conts.borrow();
        let tasks = self.tasks.borrow();
        let external_state_owners = self.external_state_owners.borrow();

        RuntimeDebugStats {
            node_updates_len: node_updates.len(),
            node_updates_cap: node_updates.capacity(),
            invalid_scopes_len: self.invalid_scope_count.get(),
            scope_queue_len: scope_queue.len(),
            scope_queue_cap: scope_queue.capacity(),
            frame_callbacks_len: frame_callbacks.len(),
            frame_callbacks_cap: frame_callbacks.capacity(),
            local_tasks_len: local_tasks.len(),
            local_tasks_cap: local_tasks.capacity(),
            ui_conts_len: ui_conts.len(),
            ui_conts_cap: ui_conts.capacity(),
            tasks_len: tasks.len(),
            tasks_cap: tasks.capacity(),
            external_state_owners_len: external_state_owners.len(),
            external_state_owners_cap: external_state_owners.capacity(),
            ui_dispatcher_pending: self.ui_dispatcher.pending.load(Ordering::SeqCst),
        }
    }
}

/// Owns the state cells, UI tasks, frame callbacks and invalidated scopes of one composition.
/// Clones share the same runtime.
#[derive(Clone)]
pub struct Runtime {
    inner: Rc<RuntimeInner>,
}

impl Runtime {
    /// Creates a runtime whose UI thread is the current thread. `scheduler` is asked for a frame
    /// whenever the runtime has work.
    pub fn new(scheduler: SchedulerRef) -> Self {
        thread_state::watch_thread_exit();
        let inner = Rc::new(RuntimeInner::new(scheduler));
        let runtime = Self { inner };
        let handle = runtime.handle();
        thread_state::runtimes(|state| {
            state
                .registered_runtimes
                .borrow_mut()
                .insert(handle.id(), handle.clone());
            *state.last_runtime.borrow_mut() = Some(handle);
        });
        runtime
    }

    #[cfg(target_os = "android")]
    pub(crate) fn restore_thread_state(&self) {
        if let Some(rescued) = self.inner.rescued.take() {
            thread_state::restore(&rescued);
        }
    }

    /// Returns a handle that reaches this runtime without keeping it alive.
    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle {
            inner: Rc::downgrade(&self.inner),
            dispatcher: UiDispatcher::new(self.inner.ui_dispatcher.clone()),
            ui_thread_id: self.inner.ui_thread_id,
            id: self.inner.runtime_id,
        }
    }

    /// Returns `true` if node updates are queued or any recompose scope is invalid.
    pub fn has_updates(&self) -> bool {
        self.inner.has_updates()
    }

    /// Returns `true` if the runtime has asked for a frame or has a task ready to run.
    pub fn needs_frame(&self) -> bool {
        *self.inner.needs_frame.borrow() || self.inner.has_runnable_tasks()
    }

    /// Sets or clears the request for a frame.
    pub fn set_needs_frame(&self, value: bool) {
        *self.inner.needs_frame.borrow_mut() = value;
    }

    /// Animation-clock time of the most recent frame-callback drain. This is
    /// the same clock `Animatable`s advance on (virtual under robot exact
    /// captures), so per-frame integrators derive dt from it instead of wall
    /// time.
    pub fn last_frame_time_nanos(&self) -> Option<u64> {
        self.inner.last_frame_time_nanos.get()
    }

    /// Returns a clock for the frames of this runtime.
    #[cfg(any(feature = "internal", test))]
    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.handle())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if Rc::strong_count(&self.inner) != 1 {
            return;
        }
        let id = self.inner.runtime_id;
        thread_state::try_runtimes(|state| {
            state.registered_runtimes.borrow_mut().remove(&id);
            let mut last = state.last_runtime.borrow_mut();
            if last.as_ref().is_some_and(|handle| handle.id() == id) {
                *last = None;
            }
        });
        thread_state::release_when_idle();
    }
}

/// A scheduler that ignores frame requests, for compositions that the caller drives itself.
#[derive(Default)]
pub struct DefaultScheduler;

impl RuntimeScheduler for DefaultScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestScheduler;

#[cfg(test)]
impl RuntimeScheduler for TestScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
pub(crate) struct TestRuntime {
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
    pub(crate) fn new() -> Self {
        Self {
            runtime: Runtime::new(Arc::new(TestScheduler)),
        }
    }

    pub(crate) fn handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }
}

/// A cheap handle to a [`Runtime`]. It does not keep the runtime alive, and its calls do nothing
/// once the runtime is dropped.
#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Weak<RuntimeInner>,
    dispatcher: UiDispatcher,
    ui_thread_id: ThreadId,
    id: RuntimeId,
}

/// What a recompose scope keeps of its runtime: one word, where a
/// [`RuntimeHandle`] takes four, on every scope of a composition. Each call
/// does nothing once the runtime is gone, as the handle's would.
pub(crate) struct ScopeRuntime(Weak<RuntimeInner>);

impl ScopeRuntime {
    pub(crate) fn mark_scope_recomposed(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.mark_scope_recomposed();
        }
    }

    pub(crate) fn register_invalid_scope(&self, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.0.upgrade() {
            inner.register_invalid_scope(scope);
        }
    }

    pub(crate) fn unregister_state_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.state_arena.unregister_scope(id, scope_id);
        }
    }

    pub(crate) fn forget_state_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.state_arena.forget_scope(id, scope_id);
        }
    }

    pub(crate) fn increment_live_recompose_scope_count(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.increment_live_recompose_scope_count();
        }
    }

    pub(crate) fn decrement_live_recompose_scope_count(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.decrement_live_recompose_scope_count();
        }
    }
}

/// Controls a future spawned with `RuntimeHandle::spawn_ui`.
pub struct TaskHandle {
    id: u64,
    runtime: RuntimeHandle,
}

pub(crate) struct DeferredStateRelease {
    runtime: Weak<RuntimeInner>,
    id: StateId,
}

pub(crate) struct StateHandleLease {
    id: StateId,
    runtime: RuntimeHandle,
}

impl StateHandleLease {
    pub(crate) fn id(&self) -> StateId {
        self.id
    }

    pub(crate) fn runtime_id(&self) -> RuntimeId {
        self.runtime.id()
    }
}

impl Drop for StateHandleLease {
    fn drop(&mut self) {
        defer_state_release(&self.runtime, self.id);
    }
}

struct StateOwnerFrame;

impl Drop for StateOwnerFrame {
    fn drop(&mut self) {
        thread_state::runtimes(|state| state.state_owners.borrow_mut().pop());
    }
}

/// Runs `build` with every state it creates owned by the caller, and returns
/// those states beside its value.
///
/// This is what makes the Jetpack Compose shape safe here:
///
/// ```rust,ignore
/// remember(|| Holder {
///     count: mutableStateOf(0),
/// })
/// ```
///
/// Kotlin leaves the state to the garbage collector, which frees it with the
/// object that holds it. Nothing collects here, so a state with no owner has
/// to be kept by the runtime for as long as the runtime lives, and a holder
/// built once per screen would pile up cells nobody can reach. Handing the
/// states to whoever is building the value puts them back on the object's
/// lifetime: the slot drops the value, the value drops the states.
pub(crate) fn collecting_states<T>(build: impl FnOnce() -> T) -> (T, Vec<Rc<StateHandleLease>>) {
    thread_state::runtimes(|state| state.state_owners.borrow_mut().push(Vec::new()));
    let frame = StateOwnerFrame;
    let value = build();
    let states = thread_state::runtimes(|state| {
        state
            .state_owners
            .borrow_mut()
            .last_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    });
    drop(frame);
    (value, states)
}

fn hand_to_current_owner(lease: &Rc<StateHandleLease>) -> bool {
    thread_state::runtimes(|state| match state.state_owners.borrow_mut().last_mut() {
        Some(owner) => {
            owner.push(Rc::clone(lease));
            true
        }
        None => false,
    })
}

impl RuntimeHandle {
    /// Returns the id of the runtime.
    pub fn id(&self) -> RuntimeId {
        self.id
    }

    pub(crate) fn alloc_state<T: Clone + 'static>(&self, value: T) -> Rc<StateHandleLease> {
        let id = self.with_state_arena(|arena| arena.alloc(value, self.clone()));
        let lease = Rc::new(StateHandleLease {
            id,
            runtime: self.clone(),
        });
        self.with_state_arena(|arena| arena.register_lease(id, &lease));
        lease
    }

    pub(crate) fn alloc_state_with_policy<T: Clone + 'static>(
        &self,
        value: T,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> Rc<StateHandleLease> {
        let id =
            self.with_state_arena(|arena| arena.alloc_with_policy(value, self.clone(), policy));
        let lease = Rc::new(StateHandleLease {
            id,
            runtime: self.clone(),
        });
        self.with_state_arena(|arena| arena.register_lease(id, &lease));
        lease
    }

    pub(crate) fn alloc_persistent_state<T: Clone + 'static>(
        &self,
        value: T,
    ) -> crate::MutableState<T> {
        self.hand_out(self.alloc_state(value))
    }

    pub(crate) fn alloc_persistent_state_with_policy<T: Clone + 'static>(
        &self,
        value: T,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> crate::MutableState<T> {
        self.hand_out(self.alloc_state_with_policy(value, policy))
    }

    fn hand_out<T: Clone + 'static>(&self, lease: Rc<StateHandleLease>) -> crate::MutableState<T> {
        if !hand_to_current_owner(&lease)
            && let Some(inner) = self.inner.upgrade()
        {
            inner
                .external_state_owners
                .borrow_mut()
                .insert(lease.id(), Rc::clone(&lease));
        }
        crate::MutableState::from_lease(&lease)
    }

    pub(crate) fn retain_state_lease(&self, id: StateId) -> Option<Rc<StateHandleLease>> {
        self.with_state_arena(|arena| arena.retain_lease(id))
    }

    pub(crate) fn with_state_arena<R>(&self, f: impl FnOnce(&StateArena) -> R) -> R {
        self.try_with_state_arena(f)
            .unwrap_or_else(|| panic!("runtime dropped"))
    }

    pub(crate) fn try_with_state_arena<R>(&self, f: impl FnOnce(&StateArena) -> R) -> Option<R> {
        self.inner.upgrade().map(|inner| f(&inner.state_arena))
    }

    /// Returns the number of state cells and, of those, the number free for reuse.
    pub fn state_arena_stats(&self) -> (usize, usize) {
        self.try_with_state_arena(StateArena::stats)
            .unwrap_or_default()
    }

    /// Returns the sizes of the runtime's state arena, for leak and memory checks.
    pub fn state_arena_debug_stats(&self) -> StateArenaDebugStats {
        self.try_with_state_arena(StateArena::debug_stats)
            .unwrap_or_default()
    }

    /// Returns the sizes of the runtime's queues and tables, for leak and memory checks.
    pub fn debug_stats(&self) -> RuntimeDebugStats {
        self.inner
            .upgrade()
            .map(|inner| inner.debug_stats())
            .unwrap_or_default()
    }

    /// Returns the id and label of every spawned UI task that has not finished.
    pub fn live_ui_task_labels(&self) -> Vec<(u64, String)> {
        self.inner
            .upgrade()
            .map(|inner| {
                inner
                    .tasks
                    .borrow()
                    .iter()
                    .map(|(id, entry)| (*id, entry.label.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Marks the runtime as needing a frame and asks its scheduler for one.
    pub fn schedule(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.schedule();
        }
    }

    pub(crate) fn enqueue_node_update(&self, command: Command) {
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

    /// Spawns `fut` on the UI thread, where `drain_ui` polls it whenever it is woken. Returns
    /// `None` if the runtime is gone.
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

    /// Cancels the spawned UI task `id` and drops its future.
    pub fn cancel_task(&self, id: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_task(id);
        }
    }

    /// Whether the runtime still holds the spawned task `id`.
    pub fn has_task(&self, id: u64) -> bool {
        self.inner.upgrade().is_some_and(|inner| inner.has_task(id))
    }

    /// Enqueues work from any thread to run on the UI thread.
    ///
    /// The closure must be `Send` because it may cross threads before executing
    /// on the runtime thread. Use this when posting from background work.
    pub fn post_ui(&self, task: impl FnOnce() + Send + 'static) {
        self.dispatcher.post(task);
    }

    /// Registers `f` to run once on the UI thread when `UiDispatcher::post_invoke` sends a `T` for
    /// the returned id. Returns `None` if the runtime is gone.
    pub fn register_ui_cont<T: 'static>(&self, f: impl FnOnce(T) + 'static) -> Option<u64> {
        self.inner.upgrade().map(|inner| inner.register_ui_cont(f))
    }

    /// Removes the continuation `id` without running it.
    pub fn cancel_ui_cont(&self, id: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_ui_cont(id);
        }
    }

    /// Runs the work posted to the UI thread, including woken tasks, until none is left.
    pub fn drain_ui(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.drain_ui();
        }
    }

    /// Returns `true` if posted UI work has not run yet.
    pub fn has_pending_ui(&self) -> bool {
        self.inner.upgrade().map_or_else(
            || self.dispatcher.has_pending(),
            |inner| inner.has_pending_ui(),
        )
    }

    /// Registers `callback` to run once on the next frame with the frame time in nanoseconds.
    /// Returns `None` if the runtime is gone.
    pub fn register_frame_callback(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> Option<FrameCallbackId> {
        self.inner.upgrade().map(|inner| {
            inner.register_frame_callback(
                FrameCallbackKind::Transient,
                FrameCallback::Run(Box::new(callback)),
            )
        })
    }

    /// Registers `callback` to run once on the next frame, for a long-lived loop that registers
    /// again after every frame. It does not count in `has_transient_frame_callbacks`.
    pub fn register_perpetual_frame_callback(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> Option<FrameCallbackId> {
        self.inner.upgrade().map(|inner| {
            inner.register_frame_callback(
                FrameCallbackKind::Perpetual,
                FrameCallback::Run(Box::new(callback)),
            )
        })
    }

    /// Wakes the task of `waker` at the next frame, whose time it then reads
    /// with [`Self::frame_waker_time`].
    #[cfg(any(feature = "internal", test))]
    pub(crate) fn register_frame_waker(
        &self,
        kind: FrameCallbackKind,
        waker: Waker,
    ) -> Option<FrameCallbackId> {
        self.inner
            .upgrade()
            .map(|inner| inner.register_frame_callback(kind, FrameCallback::Wake(waker)))
    }

    /// The frame time once the frame waker `id` has run; until then its
    /// entry wakes `waker`.
    #[cfg(any(feature = "internal", test))]
    pub(crate) fn frame_waker_time(&self, id: FrameCallbackId, waker: &Waker) -> Option<u64> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.frame_waker_time(id, waker))
    }

    /// Cancels the frame callback `id` if it has not run.
    pub fn cancel_frame_callback(&self, id: FrameCallbackId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel_frame_callback(id);
        }
    }

    /// Delivers callbacks registered before this display frame. Timestamps use
    /// one monotonic nanosecond epoch; duplicate or older timestamps are ignored.
    /// A callback registered during delivery waits for a newer frame timestamp.
    pub fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        if let Some(inner) = self.inner.upgrade() {
            inner.drain_frame_callbacks(frame_time_nanos);
        }
    }

    /// Animation-clock time of the most recent frame-callback drain (see
    /// [`Runtime::last_frame_time_nanos`]).
    pub fn last_frame_time_nanos(&self) -> Option<u64> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.last_frame_time_nanos.get())
    }

    /// Returns a clock for the frames of this runtime.
    #[cfg(any(feature = "internal", test))]
    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.clone())
    }

    /// Sets or clears the request for a frame.
    pub fn set_needs_frame(&self, value: bool) {
        if let Some(inner) = self.inner.upgrade() {
            *inner.needs_frame.borrow_mut() = value;
        }
    }

    pub(crate) fn take_updates(&self) -> Vec<Command> {
        self.inner
            .upgrade()
            .map(|inner| inner.take_updates())
            .unwrap_or_default()
    }

    /// Returns `true` if node updates are queued or any recompose scope is invalid.
    pub fn has_updates(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.has_updates())
    }

    /// The link a recompose scope keeps to this runtime.
    pub(crate) fn scope_runtime(&self) -> ScopeRuntime {
        ScopeRuntime(Weak::clone(&self.inner))
    }

    pub(crate) fn requeue_invalid_scope(&self, scope: &RecomposeScope) {
        if let Some(inner) = self.inner.upgrade() {
            inner.requeue_invalid_scope(scope);
        }
    }

    pub(crate) fn take_invalidated_scopes(&self) -> Option<Vec<RecomposeScope>> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.take_invalidated_scopes())
    }

    /// Gives back a scope list from [`Self::take_invalidated_scopes`], for
    /// the next pass to reuse its storage.
    pub(crate) fn recycle_scopes(&self, scopes: Vec<RecomposeScope>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.recycle_scopes(scopes);
        }
    }

    /// Releases the retained state of the movable content with identity
    /// `id` at the composition's next opportunity. See
    /// [`crate::forget_movable`].
    pub fn forget_movable(&self, id: Key) {
        if let Some(inner) = self.inner.upgrade() {
            inner.forgotten_movables.borrow_mut().push(id);
            inner.schedule();
        }
    }

    pub(crate) fn take_forgotten_movables(&self) -> Vec<Key> {
        self.inner
            .upgrade()
            .map(|inner| std::mem::take(&mut *inner.forgotten_movables.borrow_mut()))
            .unwrap_or_default()
    }

    /// An identity for a piece of movable content, unique within this
    /// runtime. Owned by the runtime instance rather than a process-wide
    /// counter, so two compositions in one process cannot collide and a
    /// test cannot be made to pass by the order it happens to run in.
    pub(crate) fn next_movable_content_id(&self) -> Key {
        let Some(inner) = self.inner.upgrade() else {
            log::error!("movable content asked a runtime that is gone for an identity");
            return 0;
        };
        let id = inner.next_movable_content_id.get();
        inner.next_movable_content_id.set(id.wrapping_add(1).max(1));
        id
    }

    /// Returns `true` if a recompose scope has been invalidated and not yet recomposed.
    pub fn has_invalid_scopes(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.has_invalid_scopes())
    }

    fn live_recompose_scope_count(&self) -> usize {
        self.inner
            .upgrade()
            .map(|inner| inner.live_recompose_scope_count())
            .unwrap_or_default()
    }

    #[doc(hidden)]
    pub fn debug_invalid_scope_ids(&self) -> Vec<usize> {
        self.inner
            .upgrade()
            .map(|inner| inner.queued_invalid_scope_ids())
            .unwrap_or_default()
    }

    /// Returns `true` if any frame callback, transient or perpetual, is waiting.
    pub fn has_frame_callbacks(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.has_frame_callbacks())
    }

    /// Returns `true` if a callback from `register_frame_callback` is waiting. Perpetual callbacks
    /// do not count.
    pub fn has_transient_frame_callbacks(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.has_transient_frame_callbacks())
    }

    /// Panics in debug builds when called off the runtime's UI thread.
    pub fn assert_ui_thread(&self) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.ui_thread_id,
            "state mutated off the runtime's UI thread"
        );
    }

    /// Returns a dispatcher that posts work to the UI thread from any thread.
    pub fn dispatcher(&self) -> UiDispatcher {
        self.dispatcher.clone()
    }

    #[doc(hidden)]
    pub fn with_deferred_state_releases<R>(&self, f: impl FnOnce() -> R) -> R {
        let _scope = enter_state_teardown_scope();
        f()
    }
}

impl TaskHandle {
    /// Cancels the task if it has not finished.
    pub fn cancel(&self) {
        self.runtime.cancel_task(self.id);
    }

    /// Whether the spawned future has finished or been cancelled.
    pub fn is_finished(&self) -> bool {
        !self.runtime.has_task(self.id)
    }
}

pub(crate) struct FrameCallbackEntry {
    id: FrameCallbackId,
    kind: FrameCallbackKind,
    callback: FrameCallback,
}

enum FrameCallback {
    /// Runs with the frame time.
    Run(Box<dyn FnOnce(u64) + 'static>),
    /// Wakes a task awaiting the frame.
    #[cfg(any(feature = "internal", test))]
    Wake(Waker),
}

#[cfg(not(target_arch = "wasm32"))]
struct RuntimeTaskWaker {
    scheduler: SchedulerRef,
    runnable: Arc<AtomicBool>,
    woken: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
struct RuntimeTaskWaker {
    runtime_id: RuntimeId,
    runnable: Arc<AtomicBool>,
    woken: Arc<AtomicBool>,
}

impl RuntimeTaskWaker {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(inner: &RuntimeInner, runnable: Arc<AtomicBool>) -> Self {
        let scheduler = inner.scheduler.clone();
        Self {
            scheduler,
            runnable,
            woken: Arc::clone(&inner.task_woken),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(inner: &RuntimeInner, runnable: Arc<AtomicBool>) -> Self {
        let runtime_id = inner.runtime_id;
        Self {
            runtime_id,
            runnable,
            woken: Arc::clone(&inner.task_woken),
        }
    }

    fn into_waker(self) -> Waker {
        futures_task::waker(Arc::new(self))
    }
}

impl futures_task::ArcWake for RuntimeTaskWaker {
    #[cfg(not(target_arch = "wasm32"))]
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.runnable.store(true, Ordering::Release);
        arc_self.woken.store(true, Ordering::Release);
        arc_self.scheduler.schedule_frame();
    }

    #[cfg(target_arch = "wasm32")]
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.runnable.store(true, Ordering::Release);
        arc_self.woken.store(true, Ordering::Release);
        if let Some(handle) = runtime_handle_by_id(arc_self.runtime_id) {
            handle.schedule();
        }
    }
}

/// Sizes of the runtime registries of one thread, for leak and memory checks. A `_len` field counts
/// entries and a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeThreadLocalDebugStats {
    /// Runtimes currently installed as active on this thread.
    pub active_runtimes_len: usize,
    /// Capacity reserved for the active runtimes.
    pub active_runtimes_cap: usize,
    /// Runtimes registered on this thread.
    pub registered_runtimes_len: usize,
    /// Capacity reserved for the registered runtimes.
    pub registered_runtimes_cap: usize,
    /// State releases waiting for the end of a teardown scope.
    pub deferred_state_releases_len: usize,
    /// Capacity reserved for the deferred state releases.
    pub deferred_state_releases_cap: usize,
}

/// Gets the current runtime handle from thread-local storage.
///
/// Returns the most recently pushed active runtime, or the last known runtime.
/// Used by fling animation and other components that need access to the runtime.
pub fn current_runtime_handle() -> Option<RuntimeHandle> {
    thread_state::try_runtimes(|state| {
        if let Some(handle) = state.active_runtimes.borrow().last() {
            return Some(handle.clone());
        }
        state.last_runtime.borrow().clone()
    })
    .flatten()
}

pub(crate) fn runtime_handle_by_id(id: RuntimeId) -> Option<RuntimeHandle> {
    with_registered_runtime(id, |handle| Some(handle.clone()))
}

/// Runs `f` on this thread's handle of the runtime `id`: the active
/// runtime's, which most lookups name, without hashing the id, or else the
/// registered one.
fn with_registered_runtime<R>(
    id: RuntimeId,
    f: impl FnOnce(&RuntimeHandle) -> Option<R>,
) -> Option<R> {
    thread_state::try_runtimes(|state| {
        if let Some(handle) = state.active_runtimes.borrow().last()
            && handle.id == id
        {
            return f(handle);
        }
        state.registered_runtimes.borrow().get(&id).and_then(f)
    })
    .flatten()
}

#[cfg(target_os = "android")]
pub(crate) fn share_rescue<'a>(
    handles: impl Iterator<Item = &'a RuntimeHandle>,
) -> Option<Rc<thread_state::RescuedThreadState>> {
    let live: Vec<Rc<RuntimeInner>> = handles
        .filter_map(|handle| handle.inner.upgrade())
        .collect();
    let shared = live
        .iter()
        .find_map(|inner| inner.rescued.borrow().clone())
        .or_else(|| (!live.is_empty()).then(Rc::default))?;
    for inner in &live {
        *inner.rescued.borrow_mut() = Some(Rc::clone(&shared));
    }
    Some(shared)
}

pub(crate) fn with_state_arena_by_id<R>(
    id: RuntimeId,
    f: impl FnOnce(&StateArena) -> R,
) -> Option<R> {
    let inner = with_registered_runtime(id, |handle| handle.inner.upgrade())?;
    Some(f(&inner.state_arena))
}

pub(crate) fn live_recompose_scope_count() -> usize {
    thread_state::runtimes(|state| {
        state
            .registered_runtimes
            .borrow()
            .values()
            .map(RuntimeHandle::live_recompose_scope_count)
            .sum()
    })
}

/// Returns the sizes of the runtime registries of the current thread, for leak and memory checks.
pub fn debug_runtime_thread_local_stats() -> RuntimeThreadLocalDebugStats {
    thread_state::runtimes(|state| {
        let active = state.active_runtimes.borrow();
        let registered = state.registered_runtimes.borrow();
        let deferred = state.deferred_state_releases.borrow();
        RuntimeThreadLocalDebugStats {
            active_runtimes_len: active.len(),
            active_runtimes_cap: active.capacity(),
            registered_runtimes_len: registered.len(),
            registered_runtimes_cap: registered.capacity(),
            deferred_state_releases_len: deferred.len(),
            deferred_state_releases_cap: deferred.capacity(),
        }
    })
}

fn defer_state_release(runtime: &RuntimeHandle, id: StateId) {
    let deferred = thread_state::try_runtimes(|state| {
        if state.state_teardown_depth.get() == 0 {
            return false;
        }
        state
            .deferred_state_releases
            .borrow_mut()
            .push(DeferredStateRelease {
                runtime: Weak::clone(&runtime.inner),
                id,
            });
        true
    });
    if deferred != Some(true) {
        release_state(&runtime.inner, id);
    }
}

fn release_state(runtime: &Weak<RuntimeInner>, id: StateId) {
    if let Some(inner) = runtime.upgrade() {
        inner.state_arena.release(id);
    }
}

pub(crate) struct StateTeardownScope;

pub(crate) fn enter_state_teardown_scope() -> StateTeardownScope {
    thread_state::try_runtimes(|state| {
        let depth = &state.state_teardown_depth;
        depth.set(depth.get() + 1);
    });
    StateTeardownScope
}

impl Drop for StateTeardownScope {
    fn drop(&mut self) {
        let finished = thread_state::try_runtimes(|state| {
            let depth = &state.state_teardown_depth;
            let next = depth.get().saturating_sub(1);
            depth.set(next);
            if next == 0 {
                let mut releases = state.deferred_state_releases.borrow_mut();
                while let Some(deferred) = releases.pop() {
                    release_state(&deferred.runtime, deferred.id);
                }
            }
            next == 0
        });
        if finished == Some(true) {
            thread_state::release_when_idle();
        }
    }
}

pub(crate) fn push_active_runtime(handle: &RuntimeHandle) {
    thread_state::runtimes(|state| {
        // A runtime's handles are alike: the ones the thread holds stay.
        state
            .registered_runtimes
            .borrow_mut()
            .entry(handle.id())
            .or_insert_with(|| handle.clone());
        state.active_runtimes.borrow_mut().push(handle.clone());
        let mut last = state.last_runtime.borrow_mut();
        if last.as_ref().is_none_or(|last| last.id != handle.id) {
            *last = Some(handle.clone());
        }
    });
}

pub(crate) fn pop_active_runtime() {
    thread_state::runtimes(|state| {
        state.active_runtimes.borrow_mut().pop();
    });
}

/// Schedule a new frame render using the most recently active runtime handle.
pub fn schedule_frame() {
    if let Some(handle) = current_runtime_handle() {
        handle.schedule();
        return;
    }
    log::debug!(
        target: "cranpose::runtime",
        "ignoring frame request without an active runtime",
    );
}

/// Schedule an in-place node update using the most recently active runtime.
pub fn schedule_node_update(
    update: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
) {
    if let Some(handle) = current_runtime_handle() {
        handle.enqueue_node_update(Command::callback(update));
    } else {
        drop(update);
        log::debug!(
            target: "cranpose::runtime",
            "ignoring node update request without an active runtime",
        );
    }
}

#[cfg(test)]
#[path = "tests/runtime_tests.rs"]
mod tests;
