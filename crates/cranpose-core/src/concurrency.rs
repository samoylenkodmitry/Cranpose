//! Composition-scoped structured concurrency.
//!
//! `LaunchedEffect` covers work that starts because a key changed. This covers
//! the rest: work started from an event handler, timed work, work that feeds a
//! piece of state, and blocking work that must not run on the UI thread.
//! Everything here is owned by the composition — a scope cancels its tasks when
//! it leaves, and a timer stops when nothing is waiting on it — so an
//! application never keeps its own task list or its own "is this still alive"
//! flag.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex, PoisonError};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use web_time::Instant;

use crate::{
    hooks::{mutableStateOfNeverEqual, remember},
    runtime::{RuntimeHandle, TaskHandle, current_runtime_handle},
    state::{MutableState, State},
};

/// Spawns `future` on the current runtime's UI task queue.
///
/// Framework-internal: application code launches through a
/// [`CoroutineScope`] so the work is cancelled with its
/// composition. Returns `None` when there is no runtime on this thread.
pub fn spawn_ui_task(future: impl Future<Output = ()> + 'static) -> Option<TaskHandle> {
    current_runtime_handle().and_then(|runtime| runtime.spawn_ui(future))
}

/// A cancellation scope for work launched outside the composition pass.
///
/// Tasks launched through a scope are cancelled when the scope leaves the
/// composition, so a click handler can start an asynchronous job without the
/// job outliving the screen that started it.
#[derive(Clone)]
pub struct CoroutineScope {
    inner: Rc<ScopeInner>,
}

struct ScopeInner {
    runtime: Option<RuntimeHandle>,
    tasks: RefCell<Vec<TaskHandle>>,
    closed: Cell<bool>,
}

impl Drop for ScopeInner {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().drain(..) {
            task.cancel();
        }
    }
}

struct CompositionScopeOwner(CoroutineScope);

impl Drop for CompositionScopeOwner {
    fn drop(&mut self) {
        self.0.inner.closed.set(true);
        self.0.cancel();
    }
}

impl CoroutineScope {
    /// Launches `future`, keeping it alive until it finishes or the scope is
    /// cancelled.
    pub fn launch(&self, future: impl Future<Output = ()> + 'static) {
        if self.inner.closed.get() {
            return;
        }
        let Some(runtime) = self.inner.runtime.clone() else {
            log::warn!("cranpose: a coroutine scope with no runtime dropped its work");
            return;
        };
        self.inner
            .tasks
            .borrow_mut()
            .retain(|task| !task.is_finished());
        if let Some(handle) = runtime.spawn_ui(future) {
            self.inner.tasks.borrow_mut().push(handle);
        }
    }

    /// Cancels every task this scope launched.
    pub fn cancel(&self) {
        let tasks = std::mem::take(&mut *self.inner.tasks.borrow_mut());
        for task in tasks {
            task.cancel();
        }
    }

    #[cfg(test)]
    pub(crate) fn probe_identity(&self) -> usize {
        Rc::as_ptr(&self.inner) as *const () as usize
    }
}

/// Remembers a [`CoroutineScope`] bound to this position in the composition.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberCoroutineScope() -> CoroutineScope {
    remember(|| {
        CompositionScopeOwner(CoroutineScope {
            inner: Rc::new(ScopeInner {
                runtime: current_runtime_handle(),
                tasks: RefCell::new(Vec::new()),
                closed: Cell::new(false),
            }),
        })
    })
    .with(|owner| owner.0.clone())
}

/// Resolves after `duration` has elapsed.
///
/// The wait is served by the framework's timer, which posts the wake-up onto
/// the runtime's UI queue. Nothing spins and no frames are requested while a
/// delay is pending, so a one-minute timer costs nothing for a minute.
pub fn delay(duration: Duration) -> Delay {
    Delay {
        deadline: Instant::now() + duration,
        armed: false,
        fired: Arc::new(AtomicBool::new(false)),
    }
}

/// The future returned by [`delay`].
pub struct Delay {
    deadline: Instant,
    armed: bool,
    fired: Arc<AtomicBool>,
}

impl Future for Delay {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.fired.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        let this = self.get_mut();
        if !this.armed {
            this.armed = true;
            timer().arm(
                this.deadline,
                context.waker().clone(),
                Arc::clone(&this.fired),
            );
        }
        Poll::Pending
    }
}

/// Runs `tick` every `period` until the returned future is dropped.
///
/// The first tick happens after one full period, matching a repeating timer
/// rather than a leading-edge one.
pub async fn interval(period: Duration, mut tick: impl FnMut()) {
    loop {
        delay(period).await;
        tick();
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Alarm {
    deadline: Instant,
    waker: Waker,
    fired: Arc<AtomicBool>,
}

struct Timer {
    #[cfg(not(target_arch = "wasm32"))]
    alarms: Mutex<Vec<Alarm>>,
    #[cfg(not(target_arch = "wasm32"))]
    wake: Condvar,
}

fn timer() -> &'static Timer {
    static TIMER: OnceLock<&'static Timer> = OnceLock::new();
    TIMER.get_or_init(|| {
        let timer: &'static Timer = Box::leak(Box::new(Timer::new()));
        timer.start();
        timer
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl Timer {
    fn new() -> Self {
        Self {
            alarms: Mutex::new(Vec::new()),
            wake: Condvar::new(),
        }
    }

    fn start(&'static self) {
        std::thread::Builder::new()
            .name("cranpose-timer".to_string())
            .spawn(move || self.run())
            .expect("the timer thread starts");
    }

    fn run(&self) {
        let mut alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let now = Instant::now();
            let mut due = Vec::new();
            let mut next: Option<Duration> = None;
            alarms.retain(|alarm| {
                if alarm.deadline <= now {
                    due.push((alarm.waker.clone(), Arc::clone(&alarm.fired)));
                    false
                } else {
                    let remaining = alarm.deadline - now;
                    next = Some(next.map_or(remaining, |current| current.min(remaining)));
                    true
                }
            });

            if !due.is_empty() {
                drop(alarms);
                for (waker, fired) in due {
                    fired.store(true, Ordering::Release);
                    waker.wake();
                }
                alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
                continue;
            }

            alarms = match next {
                Some(timeout) => {
                    self.wake
                        .wait_timeout(alarms, timeout)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .wake
                    .wait(alarms)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        let mut alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
        alarms.push(Alarm {
            deadline,
            waker,
            fired,
        });
        self.wake.notify_one();
    }
}

#[cfg(target_arch = "wasm32")]
impl Timer {
    fn new() -> Self {
        Self {}
    }

    fn start(&'static self) {}

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        let millis = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
            fired.store(true, Ordering::Release);
            waker.wake();
        });
        let scheduled = web_sys::window().and_then(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    millis,
                )
                .ok()
        });
        if scheduled.is_none() {
            log::warn!("cranpose: no window timer is available; the delay resolves immediately");
        }
    }
}

/// The producing half of an [`EventStream`].
///
/// A service that receives events from outside the composition — a platform
/// callback, a worker thread, a socket — publishes through a channel, and every
/// pending collector is woken. This is the shape that replaces
/// "register an observer, then drain a queue" everywhere in the framework.
pub struct EventChannel<T: 'static> {
    shared: Rc<ChannelShared<T>>,
}

struct ChannelShared<T: 'static> {
    ready: RefCell<std::collections::VecDeque<T>>,
    closed: std::cell::Cell<bool>,
    delivered: std::cell::Cell<usize>,
    wakers: RefCell<Vec<Waker>>,
}

impl<T: 'static> ChannelShared<T> {
    fn wake_all(&self) {
        for waker in self.wakers.borrow_mut().drain(..) {
            waker.wake();
        }
    }
}

impl<T: 'static> Default for EventChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> EventChannel<T> {
    /// Creates an open channel.
    pub fn new() -> Self {
        Self {
            shared: Rc::new(ChannelShared {
                ready: RefCell::new(std::collections::VecDeque::new()),
                closed: std::cell::Cell::new(false),
                delivered: std::cell::Cell::new(0),
                wakers: RefCell::new(Vec::new()),
            }),
        }
    }

    /// The consuming half, handed to collectors.
    pub fn stream(&self) -> EventStream<T> {
        EventStream {
            shared: Rc::clone(&self.shared),
        }
    }

    /// Publishes one event and wakes every pending collector.
    pub fn send(&self, event: T) {
        if self.shared.closed.get() {
            return;
        }
        self.shared.ready.borrow_mut().push_back(event);
        self.shared.wake_all();
    }

    /// Ends the stream. Collectors drain what is queued and then finish.
    pub fn close(&self) {
        if self.shared.closed.get() {
            return;
        }
        self.shared.closed.set(true);
        self.shared.wake_all();
    }

    /// Whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.shared.closed.get()
    }

    /// How many events are queued but not yet taken.
    pub fn pending(&self) -> usize {
        self.shared.ready.borrow().len()
    }
}

/// The consuming half of an [`EventChannel`].
///
/// Collectors take events one at a time; an event goes to exactly one
/// collector, so two collectors share the stream rather than each seeing every
/// event.
pub struct EventStream<T: 'static> {
    shared: Rc<ChannelShared<T>>,
}

impl<T: 'static> Clone for EventStream<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Rc::clone(&self.shared),
        }
    }
}

impl<T: 'static> EventStream<T> {
    /// Resolves with the next event, or `None` once the stream is closed and
    /// drained.
    pub fn next(&self) -> EventStreamNext<T> {
        EventStreamNext {
            shared: Rc::clone(&self.shared),
        }
    }

    /// How many events this stream has handed out.
    pub fn delivered(&self) -> usize {
        self.shared.delivered.get()
    }
}

/// The future returned by [`EventStream::next`].
pub struct EventStreamNext<T: 'static> {
    shared: Rc<ChannelShared<T>>,
}

impl<T: 'static> Future for EventStreamNext<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<T>> {
        if let Some(event) = self.shared.ready.borrow_mut().pop_front() {
            self.shared.delivered.set(self.shared.delivered.get() + 1);
            return Poll::Ready(Some(event));
        }
        if self.shared.closed.get() {
            return Poll::Ready(None);
        }
        self.shared
            .wakers
            .borrow_mut()
            .push(context.waker().clone());
        Poll::Pending
    }
}

/// Collects `stream` for as long as this call stays in the composition,
/// handing each event to `on_event`.
///
/// `key` re-starts the collection when it changes, exactly like
/// `LaunchedEffect`.
#[expect(non_snake_case)]
#[track_caller]
pub fn CollectEvents<T, K>(stream: EventStream<T>, key: K, on_event: impl FnMut(T) + 'static)
where
    T: 'static,
    K: PartialEq + 'static,
{
    crate::__launched_effect_async_impl(
        crate::caller_location_key(),
        std::panic::Location::caller().into(),
        key,
        move |_scope| {
            let mut on_event = on_event;
            Box::pin(async move {
                while let Some(event) = stream.next().await {
                    on_event(event);
                }
            })
        },
    );
}

/// Collects `stream` into state, starting at `initial`.
///
/// The composition reads the latest value the stream produced, and recomposes
/// when a new one arrives.
#[expect(non_snake_case)]
#[track_caller]
pub fn collectAsState<T, K>(stream: EventStream<T>, key: K, initial: T) -> State<T>
where
    T: Clone + 'static,
    K: PartialEq + 'static,
{
    let state = remember(|| mutableStateOfNeverEqual(initial)).with(|state| *state);
    let sink = state;
    CollectEvents(stream, key, move |event| sink.set(event));
    state.as_state()
}

/// A `Send` publishing handle for a composition-scoped [`EventStream`].
///
/// Platform services publish events from whatever thread they run on — a JNI
/// callback, a worker, a socket reader. The sender hops each event onto the UI
/// thread through the runtime's dispatcher and pushes it into the stream the
/// composition is collecting, so no service and no application ever writes that
/// hop again.
pub struct EventSender<T: Send + 'static> {
    #[cfg(not(target_arch = "wasm32"))]
    dispatcher: crate::runtime::UiDispatcher,
    bridge: u64,
    _events: std::marker::PhantomData<fn(T)>,
}

impl<T: Send + 'static> Clone for EventSender<T> {
    fn clone(&self) -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            dispatcher: self.dispatcher.clone(),
            bridge: self.bridge,
            _events: std::marker::PhantomData,
        }
    }
}

impl<T: Send + 'static> EventSender<T> {
    /// Publishes `event` to the composition that owns this bridge.
    pub fn send(&self, event: T) {
        let bridge = self.bridge;
        #[cfg(not(target_arch = "wasm32"))]
        self.dispatcher
            .post(move || deliver_bridged::<T>(bridge, event));
        #[cfg(target_arch = "wasm32")]
        deliver_bridged::<T>(bridge, event);
    }
}

thread_local! {
    static BRIDGES: RefCell<std::collections::HashMap<u64, Rc<dyn std::any::Any>>> =
        RefCell::new(std::collections::HashMap::new());
}

static NEXT_BRIDGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn deliver_bridged<T: Send + 'static>(bridge: u64, event: T) {
    let channel = BRIDGES.with(|bridges| bridges.borrow().get(&bridge).cloned());
    let Some(channel) = channel else {
        log::debug!("event bridge {bridge} is gone, one event dropped");
        return;
    };
    if let Ok(channel) = channel.downcast::<EventChannel<T>>() {
        channel.send(event);
    }
}

struct Bridge<T: Send + 'static> {
    id: u64,
    channel: Rc<EventChannel<T>>,
}

impl<T: Send + 'static> Bridge<T> {
    fn new() -> Self {
        let id = NEXT_BRIDGE.fetch_add(1, Ordering::Relaxed);
        let channel = Rc::new(EventChannel::<T>::new());
        BRIDGES.with(|bridges| {
            bridges
                .borrow_mut()
                .insert(id, Rc::clone(&channel) as Rc<dyn std::any::Any>)
        });
        Self { id, channel }
    }
}

impl<T: Send + 'static> Drop for Bridge<T> {
    fn drop(&mut self) {
        BRIDGES.with(|bridges| bridges.borrow_mut().remove(&self.id));
        self.channel.close();
    }
}

/// Turns a platform subscription into a composition-scoped [`EventStream`].
///
/// `subscribe` receives a `Send` [`EventSender`] and returns whatever
/// registration handle the service uses; that handle is dropped — unsubscribing
/// the service — when `key` changes or the composition leaves. This is the one
/// place the framework bridges "a service publishes from another thread" to
/// "a composition collects".
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberEventStream<T, K, R, S>(key: K, subscribe: S) -> EventStream<T>
where
    T: Send + 'static,
    K: PartialEq + 'static,
    R: 'static,
    S: FnOnce(EventSender<T>) -> R + 'static,
{
    let bridge = remember(Bridge::<T>::new);
    let (id, stream) = bridge.with(|bridge| (bridge.id, bridge.channel.stream()));
    #[cfg(not(target_arch = "wasm32"))]
    let dispatcher = current_runtime_handle().map(|runtime| runtime.dispatcher());

    crate::__disposable_effect_impl(crate::caller_location_key(), key, move |scope| {
        #[cfg(not(target_arch = "wasm32"))]
        let Some(dispatcher) = dispatcher else {
            log::warn!("cranpose: an event stream was remembered without a runtime");
            return scope.on_dispose(|| {});
        };
        let registration = subscribe(EventSender {
            #[cfg(not(target_arch = "wasm32"))]
            dispatcher,
            bridge: id,
            _events: std::marker::PhantomData,
        });
        scope.on_dispose(move || drop(registration))
    });

    stream
}

/// Runs `work` off the UI thread and resolves with its result on the UI thread.
///
/// This is the escape hatch for genuinely blocking work — parsing a large file,
/// a synchronous provider call — that must not stall composition. On the web
/// there is one thread, so `work` runs inline; callers keep the unit of work
/// small enough that this is honest on every target.
#[expect(non_snake_case)]
pub async fn withBlocking<T, F>(work: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let slot: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));
        let wakers: Arc<Mutex<Vec<Waker>>> = Arc::new(Mutex::new(Vec::new()));

        let worker_slot = Arc::clone(&slot);
        let worker_done = Arc::clone(&done);
        let worker_wakers = Arc::clone(&wakers);
        BlockingPool::get().submit(Box::new(move || {
            let value = work();
            *worker_slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
            worker_done.store(true, Ordering::Release);
            for waker in worker_wakers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
            {
                waker.wake();
            }
        }));

        BlockingWork { slot, done, wakers }.await
    }
    #[cfg(target_arch = "wasm32")]
    {
        work()
    }
}

/// Runs `work` off the UI thread and hands its result to `on_ui` on the UI
/// thread.
///
/// The callback shape of [`withBlocking`], for the code that is not already in
/// a coroutine: an event handler, a button, anything that wants to start some
/// blocking work and carry on. Both share the same pool, so an application
/// that uses one, the other, or both never spends more than one set of threads
/// on blocking work.
///
/// Without a runtime — a unit test, a tool — `work` runs inline and `on_ui`
/// follows it, so a caller behaves the same either way.
///
/// ```rust,ignore
/// launchBlocking(
///     move || std::fs::read(path),
///     move |bytes| document.set(bytes.ok()),
/// );
/// ```
#[expect(non_snake_case)]
pub fn launchBlocking<T>(work: impl FnOnce() -> T + Send + 'static, on_ui: impl FnOnce(T) + 'static)
where
    T: Send + 'static,
{
    let Some(runtime) = current_runtime_handle() else {
        on_ui(work());
        return;
    };
    let Some(continuation) = runtime.register_ui_cont(on_ui) else {
        return;
    };
    let dispatcher = runtime.dispatcher();
    #[cfg(not(target_arch = "wasm32"))]
    BlockingPool::get().submit(Box::new(move || {
        dispatcher.post_invoke(continuation, work());
    }));
    #[cfg(target_arch = "wasm32")]
    dispatcher.post_invoke(continuation, work());
}

#[cfg(not(target_arch = "wasm32"))]
struct BlockingPool {
    sender: std::sync::mpsc::Sender<BlockingJob>,
    receiver: Arc<Mutex<std::sync::mpsc::Receiver<BlockingJob>>>,
    state: Arc<Mutex<PoolState>>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Default)]
struct PoolState {
    alive: usize,
    outstanding: usize,
}

#[cfg(not(target_arch = "wasm32"))]
type BlockingJob = Box<dyn FnOnce() + Send + 'static>;

#[cfg(not(target_arch = "wasm32"))]
const MAX_BLOCKING_WORKERS: usize = 64;

#[cfg(not(target_arch = "wasm32"))]
const _: () = assert!(MAX_BLOCKING_WORKERS > 0 && MAX_BLOCKING_WORKERS <= 256);

#[cfg(not(target_arch = "wasm32"))]
impl BlockingPool {
    fn get() -> &'static BlockingPool {
        static POOL: OnceLock<BlockingPool> = OnceLock::new();
        POOL.get_or_init(BlockingPool::new)
    }

    fn new() -> BlockingPool {
        let (sender, receiver) = std::sync::mpsc::channel();
        BlockingPool {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            state: Arc::new(Mutex::new(PoolState::default())),
        }
    }

    fn submit(&self, job: BlockingJob) {
        if self.take_slot() {
            self.start_worker();
        }
        if let Err(returned) = self.sender.send(job) {
            self.release_slot();
            (returned.0)();
        }
    }

    fn take_slot(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.outstanding += 1;
        let grow = state.alive < state.outstanding && state.alive < MAX_BLOCKING_WORKERS;
        if grow {
            state.alive += 1;
        }
        grow
    }

    fn release_slot(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.outstanding = state.outstanding.saturating_sub(1);
    }

    fn start_worker(&self) {
        let receiver = Arc::clone(&self.receiver);
        let counters = Arc::clone(&self.state);
        let started = std::thread::Builder::new()
            .name("cranpose-blocking".to_string())
            .spawn(move || {
                loop {
                    let job = {
                        let queue = receiver.lock().unwrap_or_else(PoisonError::into_inner);
                        queue.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    job();
                    let mut counters = counters.lock().unwrap_or_else(PoisonError::into_inner);
                    counters.outstanding = counters.outstanding.saturating_sub(1);
                }
            });
        if started.is_err() {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.alive -= 1;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct BlockingWork<T> {
    slot: Arc<Mutex<Option<T>>>,
    done: Arc<AtomicBool>,
    wakers: Arc<Mutex<Vec<Waker>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T> Future for BlockingWork<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        if self.done.load(Ordering::Acquire)
            && let Some(value) = self
                .slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            return Poll::Ready(value);
        }
        self.wakers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(context.waker().clone());
        if self.done.load(Ordering::Acquire)
            && let Some(value) = self
                .slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            return Poll::Ready(value);
        }
        Poll::Pending
    }
}

/// Runs `producer` when `key` changes and exposes what it publishes as state.
///
/// The Compose `produceState` contract: the producer receives a handle it uses
/// to publish values, and is cancelled when the key changes or the composition
/// leaves.
#[expect(non_snake_case)]
#[track_caller]
pub fn produceState<T, K, F>(initial: T, key: K, producer: F) -> State<T>
where
    T: Clone + 'static,
    K: PartialEq + 'static,
    F: FnOnce(ProduceScope<T>) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
{
    let state = remember(|| mutableStateOfNeverEqual(initial)).with(|state| *state);
    let handle = ProduceScope { state };
    crate::__launched_effect_async_impl(
        crate::caller_location_key(),
        std::panic::Location::caller().into(),
        key,
        move |_scope| producer(handle),
    );
    state.as_state()
}

/// The publishing half handed to a [`produceState`] producer.
pub struct ProduceScope<T: Clone + 'static> {
    state: MutableState<T>,
}

impl<T: Clone + 'static> ProduceScope<T> {
    /// Publishes `value` to the produced state.
    pub fn set(&self, value: T) {
        self.state.set(value);
    }
}

#[cfg(test)]
#[path = "tests/concurrency_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/concurrency_stream_tests.rs"]
mod stream_tests;

#[cfg(test)]
#[path = "tests/concurrency_timer_race_tests.rs"]
mod timer_race_tests;
