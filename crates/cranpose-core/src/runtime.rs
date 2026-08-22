use crate::collections::map::HashMap;
use crate::collections::map::HashSet;
use crate::state::{MutationPolicy, NeverEqual};
use crate::MutableStateInner;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;
use std::thread_local;

#[cfg(any(feature = "internal", test))]
use crate::frame_clock::FrameClock;
use crate::platform::{RuntimeScheduler, SchedulerRef};
use crate::{Applier, Command, FrameCallbackId, NodeError, RecomposeScopeInner, ScopeId};

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
}

impl<T: Clone + 'static> ScopeWatchCell for TypedStateCell<T> {
    fn unregister_scope(&self, scope_id: ScopeId) {
        self.inner.unregister_scope(scope_id);
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateArenaDebugStats {
    pub cells_len: usize,
    pub cells_cap: usize,
    pub free_len: usize,
    pub free_cap: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeDebugStats {
    pub node_updates_len: usize,
    pub node_updates_cap: usize,
    pub invalid_scopes_len: usize,
    pub invalid_scopes_cap: usize,
    pub scope_queue_len: usize,
    pub scope_queue_cap: usize,
    pub frame_callbacks_len: usize,
    pub frame_callbacks_cap: usize,
    pub local_tasks_len: usize,
    pub local_tasks_cap: usize,
    pub ui_conts_len: usize,
    pub ui_conts_cap: usize,
    pub tasks_len: usize,
    pub tasks_cap: usize,
    pub external_state_owners_len: usize,
    pub external_state_owners_cap: usize,
    pub ui_dispatcher_pending: usize,
}

#[derive(Default)]
pub(crate) struct StateArena {
    inner: RefCell<StateArenaInner>,
}

impl StateArena {
    pub(crate) fn alloc<T: Clone + 'static>(&self, value: T, runtime: RuntimeHandle) -> StateId {
        self.alloc_with_policy(value, runtime, Arc::new(NeverEqual))
    }

    pub(crate) fn alloc_with_policy<T: Clone + 'static>(
        &self,
        value: T,
        runtime: RuntimeHandle,
        policy: Arc<dyn MutationPolicy<T>>,
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
        let inner = MutableStateInner::new_with_policy(value, runtime.clone(), policy);
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
            }
            cell
        };
        drop(cell);
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
        let watcher_cell = {
            let inner = self.inner.borrow();
            inner
                .cells
                .get(id.slot_index())
                .filter(|slot| slot.generation == id.generation())
                .and_then(|slot| slot.watcher_cell.as_ref())
                .cloned()
        };
        if let Some(watcher_cell) = watcher_cell {
            watcher_cell.unregister_scope(scope_id);
        }
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeId(u32);

impl RuntimeId {
    fn next() -> Self {
        NEXT_RUNTIME_ID.with(|next| {
            let id = next.get();
            next.set(id.wrapping_add(1));
            Self(id)
        })
    }
}

struct UiDispatcherInner {
    scheduler: SchedulerRef,
    tx: mpsc::Sender<UiMessage>,
    pending: AtomicUsize,
}

/// Shared handle to a [`UiDispatcherInner`].
///
/// `UiDispatcherInner` holds a [`SchedulerRef`], so it inherits the same
/// per-target split: native code posts work onto the UI dispatcher from other
/// threads (see `EventSender` and `launchBlocking` in `concurrency.rs`), so
/// the handle has to be an atomically reference-counted `Send + Sync` pointer
/// there; on wasm the dispatcher is only ever touched from the one thread it
/// was created on, and its scheduler cannot be `Send + Sync`, so `Rc` is used
/// instead of paying for synchronisation the target cannot use.
#[cfg(not(target_arch = "wasm32"))]
type UiDispatcherRef = Arc<UiDispatcherInner>;

/// See the native definition of [`UiDispatcherRef`] for why this is `Rc` on wasm.
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

impl<'a> Drop for PendingGuard<'a> {
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

#[derive(Clone)]
pub struct UiDispatcher {
    inner: UiDispatcherRef,
}

impl UiDispatcher {
    fn new(inner: UiDispatcherRef) -> Self {
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
    scheduler: SchedulerRef,
    needs_frame: RefCell<bool>,
    node_updates: RefCell<Vec<Command>>,
    invalid_scopes: RefCell<HashSet<ScopeId>>,
    scope_queue: RefCell<Vec<(ScopeId, Weak<RecomposeScopeInner>)>>,
    frame_callbacks: RefCell<VecDeque<FrameCallbackEntry>>,
    next_frame_callback_id: Cell<u64>,
    last_frame_time_nanos: Cell<Option<u64>>,
    ui_dispatcher: UiDispatcherRef,
    ui_rx: RefCell<mpsc::Receiver<UiMessage>>,
    local_tasks: RefCell<VecDeque<Box<dyn FnOnce() + 'static>>>,
    ui_conts: RefCell<UiContinuationMap>,
    next_cont_id: Cell<u64>,
    ui_thread_id: ThreadId,
    tasks: RefCell<Vec<TaskEntry>>,
    next_task_id: Cell<u64>,
    state_arena: StateArena,
    external_state_owners: RefCell<HashMap<StateId, Rc<StateHandleLease>>>,
    live_recompose_scope_count: Cell<usize>,
    runtime_id: RuntimeId,
}

struct TaskEntry {
    id: u64,
    label: String,
    future: Pin<Box<dyn Future<Output = ()> + 'static>>,
    /// Whether this task can currently make progress.
    ///
    /// Set when the task is spawned and every time its waker fires, cleared
    /// immediately before it is polled. A task that returned `Poll::Pending`
    /// and has not been woken since is *parked*: polling it again would only
    /// return `Poll::Pending` a second time, and — more importantly — it is
    /// not a reason to keep asking the display for frames. See
    /// [`RuntimeInner::has_pending_ui`].
    ///
    /// Shared with the task's [`Waker`], which may be woken from any thread.
    runnable: Arc<AtomicBool>,
    /// The waker handed to this task's future, so a wake can be attributed to
    /// the one task that is ready rather than to all of them.
    waker: Waker,
}

thread_local! {
    static NEXT_TASK_LABEL: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn label_next_ui_task(label: impl Into<String>) {
    NEXT_TASK_LABEL.with(|held| *held.borrow_mut() = Some(label.into()));
}

impl RuntimeInner {
    fn new(scheduler: SchedulerRef) -> Self {
        let (tx, rx) = mpsc::channel();
        let dispatcher = UiDispatcherRef::new(UiDispatcherInner::new(scheduler.clone(), tx));
        Self {
            scheduler,
            needs_frame: RefCell::new(false),
            node_updates: RefCell::new(Vec::new()),
            invalid_scopes: RefCell::new(HashSet::default()),
            scope_queue: RefCell::new(Vec::new()),
            frame_callbacks: RefCell::new(VecDeque::new()),
            next_frame_callback_id: Cell::new(1),
            last_frame_time_nanos: Cell::new(None),
            ui_dispatcher: dispatcher,
            ui_rx: RefCell::new(rx),
            local_tasks: RefCell::new(VecDeque::new()),
            ui_conts: RefCell::new(UiContinuationMap::default()),
            next_cont_id: Cell::new(1),
            ui_thread_id: std::thread::current().id(),
            tasks: RefCell::new(Vec::new()),
            next_task_id: Cell::new(1),
            state_arena: StateArena::default(),
            external_state_owners: RefCell::new(HashMap::default()),
            live_recompose_scope_count: Cell::new(0),
            runtime_id: RuntimeId::next(),
        }
    }

    fn schedule(&self) {
        *self.needs_frame.borrow_mut() = true;
        self.scheduler.schedule_frame();
    }

    fn enqueue_update(&self, command: Command) {
        self.node_updates.borrow_mut().push(command);
        self.schedule(); // Ensure frame is scheduled to process the command
    }

    fn take_updates(&self) -> Vec<Command> {
        let updates = self.node_updates.borrow_mut().drain(..).collect::<Vec<_>>();
        updates
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

    fn requeue_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if self.invalid_scopes.borrow().contains(&id) {
            self.scope_queue.borrow_mut().push((id, scope));
            self.schedule();
        }
    }

    fn mark_scope_recomposed(&self, id: ScopeId) {
        self.invalid_scopes.borrow_mut().remove(&id);
    }

    fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
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
        let label = NEXT_TASK_LABEL
            .with(|held| held.borrow_mut().take())
            .unwrap_or_else(|| "unnamed".to_string());
        let runnable = Arc::new(AtomicBool::new(true));
        let waker = RuntimeTaskWaker::new(self, Arc::clone(&runnable)).into_waker();
        self.tasks.borrow_mut().push(TaskEntry {
            id,
            label,
            future,
            runnable,
            waker,
        });
        self.schedule();
        id
    }

    fn cancel_task(&self, id: u64) {
        let mut tasks = self.tasks.borrow_mut();
        if tasks.iter().any(|entry| entry.id == id) {
            tasks.retain(|entry| entry.id != id);
        }
    }

    /// Whether a spawned task is still registered. A task that completed or was
    /// cancelled is gone from the list.
    fn has_task(&self, id: u64) -> bool {
        // `poll_async_tasks` takes the list while polling, so a task being
        // polled right now is momentarily absent. Only the UI thread polls, and
        // only the UI thread asks, so that window is never observed.
        self.tasks
            .try_borrow()
            .map(|tasks| tasks.iter().any(|entry| entry.id == id))
            .unwrap_or(true)
    }

    fn poll_async_tasks(&self) -> bool {
        let mut tasks_ref = self.tasks.borrow_mut();
        let tasks = std::mem::take(&mut *tasks_ref);
        drop(tasks_ref);
        let mut pending = Vec::with_capacity(tasks.len());
        let mut made_progress = false;
        for mut entry in tasks.into_iter() {
            // Claim the wake before polling, not after: the future may be woken
            // from another thread while it is being polled, and that wake has to
            // survive into the next pass rather than be cleared by this one.
            if !entry.runnable.swap(false, Ordering::AcqRel) {
                pending.push(entry);
                continue;
            }
            let mut cx = Context::from_waker(&entry.waker);
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

        // Draining is the point at which tasks park, so it is also the first
        // moment we can tell that the runtime has gone quiet. Checking here
        // rather than waiting for the next frame-callback drain saves the app
        // one wasted wake every time it settles.
        self.clear_needs_frame_if_idle();
    }

    fn has_pending_ui(&self) -> bool {
        let local_pending = self
            .local_tasks
            .try_borrow()
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true);

        local_pending || self.ui_dispatcher.has_pending() || self.has_runnable_tasks()
    }

    /// Whether any spawned task could make progress if it were polled now.
    ///
    /// Only *runnable* tasks count. A task that is merely alive is not pending
    /// work: an effect awaiting a back gesture, a network reply, or a frame it
    /// has not asked for yet will do nothing if polled, so counting it as
    /// pending pinned `needs_frame` true for the lifetime of the composition.
    /// Since almost every app spawns at least one long-lived effect, that kept
    /// the frame clock running forever and asked the display for 60 frames a
    /// second on a screen where nothing moved. A task awaiting the frame clock
    /// keeps frames coming through `has_frame_callbacks` instead, which is the
    /// honest reason to want one.
    fn has_runnable_tasks(&self) -> bool {
        self.tasks
            .try_borrow()
            .map(|tasks| {
                tasks
                    .iter()
                    .any(|task| task.runnable.load(Ordering::Acquire))
            })
            .unwrap_or(true)
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
        self.ui_conts.borrow_mut().remove(&id);
    }

    fn register_frame_callback(
        &self,
        kind: FrameCallbackKind,
        callback: Box<dyn FnOnce(u64) + 'static>,
    ) -> FrameCallbackId {
        let id = self.next_frame_callback_id.get();
        self.next_frame_callback_id.set(id + 1);
        self.frame_callbacks
            .borrow_mut()
            .push_back(FrameCallbackEntry {
                id,
                kind,
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
        drop(callbacks);
        self.clear_needs_frame_if_idle();
    }

    /// Stops asking the display for frames when nothing is left for one to do.
    ///
    /// These four are the complete set of reasons to want another frame: a
    /// scope to recompose, a queued node update, a registered frame callback,
    /// or UI work that can run. If none of them holds, the next frame would
    /// wake the process, walk an unchanged tree, and present nothing.
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
        self.last_frame_time_nanos.set(Some(frame_time_nanos));
        let mut callbacks = self.frame_callbacks.borrow_mut();
        let mut pending: Vec<Box<dyn FnOnce(u64) + 'static>> = Vec::with_capacity(callbacks.len());
        while let Some(mut entry) = callbacks.pop_front() {
            if let Some(callback) = entry.callback.take() {
                pending.push(callback);
            }
        }
        drop(callbacks);

        // Wrap ALL frame callbacks in a single mutable snapshot so state changes
        // are properly applied to the global snapshot and visible to subsequent reads.
        // Using a single snapshot for all callbacks avoids stack exhaustion from
        // repeated snapshot creation in long-running animation loops.
        if !pending.is_empty() {
            let _ = crate::run_in_mutable_snapshot(|| {
                for callback in pending {
                    callback(frame_time_nanos);
                }
            });
        }

        self.clear_needs_frame_if_idle();
    }

    fn debug_stats(&self) -> RuntimeDebugStats {
        let node_updates = self.node_updates.borrow();
        let invalid_scopes = self.invalid_scopes.borrow();
        let scope_queue = self.scope_queue.borrow();
        let frame_callbacks = self.frame_callbacks.borrow();
        let local_tasks = self.local_tasks.borrow();
        let ui_conts = self.ui_conts.borrow();
        let tasks = self.tasks.borrow();
        let external_state_owners = self.external_state_owners.borrow();

        RuntimeDebugStats {
            node_updates_len: node_updates.len(),
            node_updates_cap: node_updates.capacity(),
            invalid_scopes_len: invalid_scopes.len(),
            invalid_scopes_cap: invalid_scopes.capacity(),
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

#[derive(Clone)]
pub struct Runtime {
    inner: Rc<RuntimeInner>,
}

impl Runtime {
    pub fn new(scheduler: SchedulerRef) -> Self {
        let inner = Rc::new(RuntimeInner::new(scheduler));
        let runtime = Self { inner };
        let handle = runtime.handle();
        register_runtime_handle(&handle);
        LAST_RUNTIME.with(|slot| *slot.borrow_mut() = Some(handle));
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
        // The stored flag is only half the answer. A task woken from another
        // thread cannot touch this runtime's `RefCell`s, so its waker can only
        // set the shared `runnable` flag and ping the scheduler.
        //
        // A merely pending UI continuation does NOT belong here: main's "Avoid
        // frames for idle UI continuations" moved that to an update-only wake
        // (`has_pending_ui` in the app shell), so an idle continuation no
        // longer costs a frame.
        *self.inner.needs_frame.borrow() || self.inner.has_runnable_tasks()
    }

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
        unregister_runtime_handle(self.inner.runtime_id);
        LAST_RUNTIME.with(|slot| {
            let should_clear = slot
                .borrow()
                .as_ref()
                .is_some_and(|handle| handle.id() == self.inner.runtime_id);
            if should_clear {
                *slot.borrow_mut() = None;
            }
        });
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

struct DeferredStateRelease {
    runtime: RuntimeHandle,
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

    pub(crate) fn runtime(&self) -> RuntimeHandle {
        self.runtime.clone()
    }
}

impl Drop for StateHandleLease {
    fn drop(&mut self) {
        defer_state_release(self.runtime.clone(), self.id);
    }
}

impl RuntimeHandle {
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
        policy: Arc<dyn MutationPolicy<T>>,
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
        let lease = self.alloc_state(value);
        if let Some(inner) = self.inner.upgrade() {
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

    fn release_state_immediate(&self, id: StateId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.state_arena.release(id);
        }
    }

    pub fn state_arena_stats(&self) -> (usize, usize) {
        self.try_with_state_arena(StateArena::stats)
            .unwrap_or_default()
    }

    pub fn state_arena_debug_stats(&self) -> StateArenaDebugStats {
        self.try_with_state_arena(StateArena::debug_stats)
            .unwrap_or_default()
    }

    pub fn debug_stats(&self) -> RuntimeDebugStats {
        self.inner
            .upgrade()
            .map(|inner| inner.debug_stats())
            .unwrap_or_default()
    }

    pub fn live_ui_task_labels(&self) -> Vec<(u64, String)> {
        self.inner
            .upgrade()
            .map(|inner| {
                inner
                    .tasks
                    .borrow()
                    .iter()
                    .map(|entry| (entry.id, entry.label.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn unregister_state_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.state_arena.unregister_scope(id, scope_id);
        }
    }

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

    /// Whether the runtime still holds the spawned task `id`.
    pub fn has_task(&self, id: u64) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_task(id))
            .unwrap_or(false)
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
        self.inner.upgrade().map(|inner| {
            inner.register_frame_callback(FrameCallbackKind::Transient, Box::new(callback))
        })
    }

    pub fn register_perpetual_frame_callback(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> Option<FrameCallbackId> {
        self.inner.upgrade().map(|inner| {
            inner.register_frame_callback(FrameCallbackKind::Perpetual, Box::new(callback))
        })
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

    /// Animation-clock time of the most recent frame-callback drain (see
    /// [`Runtime::last_frame_time_nanos`]).
    pub fn last_frame_time_nanos(&self) -> Option<u64> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.last_frame_time_nanos.get())
    }

    #[cfg(any(feature = "internal", test))]
    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.clone())
    }

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

    pub fn has_updates(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_updates())
            .unwrap_or(false)
    }

    pub(crate) fn mark_scope_recomposed(&self, id: ScopeId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.mark_scope_recomposed(id);
        }
    }

    pub(crate) fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.register_invalid_scope(id, scope);
        }
    }

    pub(crate) fn requeue_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.requeue_invalid_scope(id, scope);
        }
    }

    pub(crate) fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
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

    pub(crate) fn increment_live_recompose_scope_count(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.increment_live_recompose_scope_count();
        }
    }

    pub(crate) fn decrement_live_recompose_scope_count(&self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.decrement_live_recompose_scope_count();
        }
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
            .map(|inner| inner.invalid_scopes.borrow().iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn has_frame_callbacks(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_frame_callbacks())
            .unwrap_or(false)
    }

    pub fn has_transient_frame_callbacks(&self) -> bool {
        self.inner
            .upgrade()
            .map(|inner| inner.has_transient_frame_callbacks())
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

    #[doc(hidden)]
    pub fn with_deferred_state_releases<R>(&self, f: impl FnOnce() -> R) -> R {
        let _scope = enter_state_teardown_scope();
        f()
    }
}

impl TaskHandle {
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
    callback: Option<Box<dyn FnOnce(u64) + 'static>>,
}

/// The waker for one spawned UI task.
///
/// Waking marks that task runnable and asks the platform for a frame, which is
/// what eventually reaches [`RuntimeInner::poll_async_tasks`]. The `runnable`
/// flag is the half that matters for idling: without it a wake is
/// indistinguishable from any other, so the runtime cannot tell a task that is
/// ready from one that is parked.
///
/// `runnable` is an `Arc<AtomicBool>` rather than a `Cell` because a task may
/// legitimately be woken from another thread — a JNI callback, a worker
/// finishing, a platform service replying.
#[cfg(not(target_arch = "wasm32"))]
struct RuntimeTaskWaker {
    scheduler: SchedulerRef,
    runnable: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
struct RuntimeTaskWaker {
    runtime_id: RuntimeId,
    runnable: Arc<AtomicBool>,
}

impl RuntimeTaskWaker {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(inner: &RuntimeInner, runnable: Arc<AtomicBool>) -> Self {
        let scheduler = inner.scheduler.clone();
        Self {
            scheduler,
            runnable,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(inner: &RuntimeInner, runnable: Arc<AtomicBool>) -> Self {
        let runtime_id = inner.runtime_id;
        Self {
            runtime_id,
            runnable,
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
        arc_self.scheduler.schedule_frame();
    }

    #[cfg(target_arch = "wasm32")]
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.runnable.store(true, Ordering::Release);
        REGISTERED_RUNTIMES.with(|registry| {
            if let Some(handle) = registry.borrow().get(&arc_self.runtime_id).cloned() {
                handle.schedule();
            }
        });
    }
}

thread_local! {
    static NEXT_RUNTIME_ID: Cell<u32> = const { Cell::new(1) };
    static ACTIVE_RUNTIMES: RefCell<Vec<RuntimeHandle>> = const { RefCell::new(Vec::new()) };
    static LAST_RUNTIME: RefCell<Option<RuntimeHandle>> = const { RefCell::new(None) };
    static REGISTERED_RUNTIMES: RefCell<HashMap<RuntimeId, RuntimeHandle>> = RefCell::new(HashMap::default());
    static STATE_TEARDOWN_DEPTH: Cell<usize> = const { Cell::new(0) };
    static DEFERRED_STATE_RELEASES: RefCell<Vec<DeferredStateRelease>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeThreadLocalDebugStats {
    pub active_runtimes_len: usize,
    pub active_runtimes_cap: usize,
    pub registered_runtimes_len: usize,
    pub registered_runtimes_cap: usize,
    pub deferred_state_releases_len: usize,
    pub deferred_state_releases_cap: usize,
}

/// Gets the current runtime handle from thread-local storage.
///
/// Returns the most recently pushed active runtime, or the last known runtime.
/// Used by fling animation and other components that need access to the runtime.
pub fn current_runtime_handle() -> Option<RuntimeHandle> {
    if let Some(handle) = ACTIVE_RUNTIMES.with(|stack| stack.borrow().last().cloned()) {
        return Some(handle);
    }
    LAST_RUNTIME.with(|slot| slot.borrow().clone())
}

pub(crate) fn runtime_handle_by_id(id: RuntimeId) -> Option<RuntimeHandle> {
    REGISTERED_RUNTIMES.with(|registry| registry.borrow().get(&id).cloned())
}

pub(crate) fn live_recompose_scope_count() -> usize {
    REGISTERED_RUNTIMES.with(|registry| {
        registry
            .borrow()
            .values()
            .map(RuntimeHandle::live_recompose_scope_count)
            .sum()
    })
}

pub fn debug_runtime_thread_local_stats() -> RuntimeThreadLocalDebugStats {
    let (active_runtimes_len, active_runtimes_cap) = ACTIVE_RUNTIMES.with(|stack| {
        let stack = stack.borrow();
        (stack.len(), stack.capacity())
    });
    let (registered_runtimes_len, registered_runtimes_cap) = REGISTERED_RUNTIMES.with(|registry| {
        let registry = registry.borrow();
        (registry.len(), registry.capacity())
    });
    let (deferred_state_releases_len, deferred_state_releases_cap) =
        DEFERRED_STATE_RELEASES.with(|releases| {
            let releases = releases.borrow();
            (releases.len(), releases.capacity())
        });

    RuntimeThreadLocalDebugStats {
        active_runtimes_len,
        active_runtimes_cap,
        registered_runtimes_len,
        registered_runtimes_cap,
        deferred_state_releases_len,
        deferred_state_releases_cap,
    }
}

fn register_runtime_handle(handle: &RuntimeHandle) {
    REGISTERED_RUNTIMES.with(|registry| {
        registry.borrow_mut().insert(handle.id(), handle.clone());
    });
}

fn unregister_runtime_handle(id: RuntimeId) {
    REGISTERED_RUNTIMES.with(|registry| {
        registry.borrow_mut().remove(&id);
    });
}

fn defer_state_release(runtime: RuntimeHandle, id: StateId) {
    let teardown_active = STATE_TEARDOWN_DEPTH.with(|depth| depth.get() > 0);
    if teardown_active {
        DEFERRED_STATE_RELEASES.with(|releases| {
            releases
                .borrow_mut()
                .push(DeferredStateRelease { runtime, id });
        });
    } else {
        runtime.release_state_immediate(id);
    }
}

fn flush_deferred_state_releases() {
    DEFERRED_STATE_RELEASES.with(|releases| {
        let mut releases = releases.borrow_mut();
        while let Some(deferred) = releases.pop() {
            deferred.runtime.release_state_immediate(deferred.id);
        }
    });
}

pub(crate) struct StateTeardownScope;

pub(crate) fn enter_state_teardown_scope() -> StateTeardownScope {
    STATE_TEARDOWN_DEPTH.with(|depth| depth.set(depth.get() + 1));
    StateTeardownScope
}

impl Drop for StateTeardownScope {
    fn drop(&mut self) {
        STATE_TEARDOWN_DEPTH.with(|depth| {
            let next = depth.get().saturating_sub(1);
            depth.set(next);
            if next == 0 {
                flush_deferred_state_releases();
            }
        });
    }
}

pub(crate) fn push_active_runtime(handle: &RuntimeHandle) {
    register_runtime_handle(handle);
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
mod tests {
    use super::*;

    #[test]
    fn state_arena_alloc_skips_free_slot_outside_cell_storage() {
        let runtime = TestRuntime::new();
        let arena = StateArena::default();
        let first = arena.alloc(1_i32, runtime.handle());
        arena.inner.borrow_mut().free.push(u32::MAX);

        let second = arena.alloc(2_i32, runtime.handle());

        assert_eq!(first.slot(), 0);
        assert_eq!(second.slot(), 1);
        assert!(arena.get_typed_opt::<i32>(first).is_some());
        assert!(arena.get_typed_opt::<i32>(second).is_some());
    }

    #[test]
    fn state_arena_alloc_skips_occupied_free_slot() {
        let runtime = TestRuntime::new();
        let arena = StateArena::default();
        let first = arena.alloc(1_i32, runtime.handle());
        arena.inner.borrow_mut().free.push(first.slot());

        let second = arena.alloc(2_i32, runtime.handle());

        assert_ne!(first.slot(), second.slot());
        assert_eq!(second.slot(), 1);
        assert!(arena.get_typed_opt::<i32>(first).is_some());
        assert!(arena.get_typed_opt::<i32>(second).is_some());
    }

    #[test]
    fn state_arena_register_lease_ignores_stale_id() {
        let runtime = TestRuntime::new();
        let arena = StateArena::default();
        let stale_id = StateId::new(99, 0);
        let lease = Rc::new(StateHandleLease {
            id: stale_id,
            runtime: runtime.handle(),
        });

        arena.register_lease(stale_id, &lease);

        assert!(arena.retain_lease(stale_id).is_none());
    }

    #[test]
    fn ui_continuation_type_mismatch_is_ignored_until_matching_payload() {
        let runtime = TestRuntime::new();
        let handle = runtime.handle();
        let received = Rc::new(Cell::new(None));
        let received_for_continuation = Rc::clone(&received);
        let cont_id = handle
            .register_ui_cont(move |value: u32| {
                received_for_continuation.set(Some(value));
            })
            .expect("test runtime is alive");

        handle.dispatcher().post_invoke(cont_id, "wrong payload");
        handle.drain_ui();

        assert_eq!(received.get(), None);
        assert_eq!(handle.debug_stats().ui_conts_len, 1);

        handle.dispatcher().post_invoke(cont_id, 42_u32);
        handle.drain_ui();

        assert_eq!(received.get(), Some(42));
        assert_eq!(handle.debug_stats().ui_conts_len, 0);
    }

    #[test]
    fn ui_dispatcher_failed_send_does_not_leave_pending_work() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let dispatcher = runtime.handle().dispatcher();

        drop(runtime);

        assert!(!dispatcher.has_pending());
        dispatcher.post(|| {});
        assert!(!dispatcher.has_pending());
        dispatcher.post_invoke(404, 12_u32);
        assert!(!dispatcher.has_pending());
    }

    #[test]
    fn pending_guard_does_not_wrap_on_underflow() {
        let counter = AtomicUsize::new(0);

        {
            let _guard = PendingGuard::new(&counter);
        }

        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn pending_guard_decrements_pending_count() {
        let counter = AtomicUsize::new(2);

        {
            let _guard = PendingGuard::new(&counter);
        }

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn schedule_frame_without_runtime_is_ignored() {
        let ok = std::thread::spawn(|| std::panic::catch_unwind(super::schedule_frame).is_ok())
            .join()
            .expect("test thread should join");

        assert!(ok);
    }

    #[test]
    fn schedule_node_update_without_runtime_is_ignored() {
        let ok = std::thread::spawn(|| {
            std::panic::catch_unwind(|| {
                super::schedule_node_update(|_| Ok(()));
            })
            .is_ok()
        })
        .join()
        .expect("test thread should join");

        assert!(ok);
    }
}
