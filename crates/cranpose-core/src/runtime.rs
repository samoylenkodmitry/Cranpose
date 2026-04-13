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
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;
use std::thread_local;

#[cfg(any(feature = "internal", test))]
use crate::frame_clock::FrameClock;
use crate::platform::RuntimeScheduler;
use crate::{Applier, Command, FrameCallbackId, NodeError, RecomposeScopeInner, ScopeId};

enum UiMessage {
    Task(Box<dyn FnOnce() + Send + 'static>),
    Invoke { id: u64, value: Box<dyn Any + Send> },
}

type UiContinuation = Box<dyn Fn(Box<dyn Any>) + 'static>;
type UiContinuationMap = HashMap<u64, UiContinuation>;

struct TypedStateCell<T: Clone + 'static> {
    inner: MutableStateInner<T>,
}

trait PrunableCell {
    fn prune_dead_watchers(&self);
}

impl<T: Clone + 'static> PrunableCell for TypedStateCell<T> {
    fn prune_dead_watchers(&self) {
        self.inner.prune_dead_watchers();
    }
}

#[allow(dead_code)]
struct RawStateCell<T: 'static> {
    value: T,
}

struct StateArenaSlot {
    generation: u32,
    cell: Option<Rc<dyn Any>>,
    prunable_cell: Option<Rc<dyn PrunableCell>>,
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
            match inner.free.pop() {
                Some(slot) => {
                    let entry = inner
                        .cells
                        .get_mut(slot as usize)
                        .expect("state slot missing");
                    debug_assert!(entry.cell.is_none(), "reused state slot must be empty");
                    entry.generation = entry.generation.wrapping_add(1);
                    (slot, entry.generation)
                }
                None => {
                    let slot = inner.cells.len() as u32;
                    inner.cells.push(StateArenaSlot {
                        generation: 0,
                        cell: None,
                        prunable_cell: None,
                        lease: None,
                    });
                    (slot, 0)
                }
            }
        };
        let id = StateId::new(slot, generation);
        let inner = MutableStateInner::new_with_policy(value, runtime.clone(), policy);
        inner.install_snapshot_observer(id);
        let typed_cell = Rc::new(TypedStateCell { inner });
        let cell: Rc<dyn Any> = typed_cell.clone();
        let prunable_cell: Rc<dyn PrunableCell> = typed_cell;
        let mut arena = self.inner.borrow_mut();
        let slot_entry = &mut arena.cells[slot as usize];
        slot_entry.cell = Some(cell);
        slot_entry.prunable_cell = Some(prunable_cell);
        id
    }

    #[allow(dead_code)]
    pub(crate) fn alloc_raw<T: 'static>(&self, value: T) -> StateId {
        let (slot, generation) = {
            let mut inner = self.inner.borrow_mut();
            match inner.free.pop() {
                Some(slot) => {
                    let entry = inner
                        .cells
                        .get_mut(slot as usize)
                        .expect("state slot missing");
                    debug_assert!(entry.cell.is_none(), "reused state slot must be empty");
                    entry.generation = entry.generation.wrapping_add(1);
                    (slot, entry.generation)
                }
                None => {
                    let slot = inner.cells.len() as u32;
                    inner.cells.push(StateArenaSlot {
                        generation: 0,
                        cell: None,
                        prunable_cell: None,
                        lease: None,
                    });
                    (slot, 0)
                }
            }
        };
        let id = StateId::new(slot, generation);
        let cell: Rc<dyn Any> = Rc::new(RawStateCell { value });
        let mut arena = self.inner.borrow_mut();
        let slot_entry = &mut arena.cells[slot as usize];
        slot_entry.cell = Some(cell);
        slot_entry.prunable_cell = None;
        id
    }

    fn get_cell(&self, id: StateId) -> Rc<dyn Any> {
        self.get_cell_opt(id).expect("state cell missing")
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

    #[allow(dead_code)]
    pub(crate) fn with_raw<T: 'static, R>(&self, id: StateId, f: impl FnOnce(&T) -> R) -> R {
        let cell = Rc::downcast::<RawStateCell<T>>(self.get_cell(id))
            .expect("raw state cell type mismatch");
        f(&cell.value)
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
            slot.prunable_cell = None;
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

    pub(crate) fn prune_dead_watchers(&self) {
        let inner = self.inner.borrow();
        for slot in &inner.cells {
            if let Some(prunable_cell) = slot.prunable_cell.as_ref() {
                prunable_cell.prune_dead_watchers();
            }
        }
    }

    pub(crate) fn register_lease(&self, id: StateId, lease: &Rc<StateHandleLease>) {
        let mut inner = self.inner.borrow_mut();
        let Some(slot) = inner.cells.get_mut(id.slot_index()) else {
            panic!("state slot missing");
        };
        assert_eq!(
            slot.generation,
            id.generation(),
            "state generation mismatch"
        );
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
    external_state_owners: RefCell<HashMap<StateId, Rc<StateHandleLease>>>,
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
            external_state_owners: RefCell::new(HashMap::default()),
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
        self.schedule(); // Ensure frame is scheduled to process the command
    }

    fn take_updates(&self) -> Vec<Command> {
        // FUTURE(no_std): return stack-allocated smallvec.
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

        if !self.has_invalid_scopes()
            && !self.has_updates()
            && !self.has_frame_callbacks()
            && !self.has_pending_ui()
        {
            *self.needs_frame.borrow_mut() = false;
        }
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
    inner: Rc<RuntimeInner>, // FUTURE(no_std): replace Rc with arena-managed runtime storage.
}

impl Runtime {
    pub fn new(scheduler: Arc<dyn RuntimeScheduler>) -> Self {
        let inner = Rc::new(RuntimeInner::new(scheduler));
        RuntimeInner::init_task_waker(&inner);
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
        *self.inner.needs_frame.borrow() || self.inner.ui_dispatcher.has_pending()
    }

    pub fn set_needs_frame(&self, value: bool) {
        *self.inner.needs_frame.borrow_mut() = value;
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
        self.inner
            .upgrade()
            .map(|inner| f(&inner.state_arena))
            .unwrap_or_else(|| panic!("runtime dropped"))
    }

    #[allow(dead_code)]
    pub(crate) fn alloc_value<T: 'static>(&self, value: T) -> StateId {
        self.with_state_arena(|arena| arena.alloc_raw(value))
    }

    #[allow(dead_code)]
    pub(crate) fn with_value<T: 'static, R>(&self, id: StateId, f: impl FnOnce(&T) -> R) -> R {
        self.with_state_arena(|arena| arena.with_raw::<T, R>(id, f))
    }

    fn release_state_immediate(&self, id: StateId) {
        if let Some(inner) = self.inner.upgrade() {
            inner.state_arena.release(id);
        }
    }

    pub fn state_arena_stats(&self) -> (usize, usize) {
        self.with_state_arena(StateArena::stats)
    }

    pub fn state_arena_debug_stats(&self) -> StateArenaDebugStats {
        self.with_state_arena(StateArena::debug_stats)
    }

    pub fn debug_stats(&self) -> RuntimeDebugStats {
        self.inner
            .upgrade()
            .map(|inner| inner.debug_stats())
            .unwrap_or_default()
    }

    pub(crate) fn prune_dead_state_watchers(&self) {
        self.with_state_arena(StateArena::prune_dead_watchers);
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
    static ACTIVE_RUNTIMES: RefCell<Vec<RuntimeHandle>> = const { RefCell::new(Vec::new()) }; // FUTURE(no_std): move to bounded stack storage.
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
    panic!("no runtime available to schedule frame");
}

/// Schedule an in-place node update using the most recently active runtime.
pub fn schedule_node_update(
    update: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
) {
    let handle = current_runtime_handle().expect("no runtime available to schedule node update");
    handle.enqueue_node_update(Command::callback(update));
}
