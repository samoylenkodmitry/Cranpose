use super::{inspector_metadata, Modifier, PointerEvent};
use compose_foundation::{
    impl_pointer_input_node, DelegatableNode, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState, PointerInputNode,
};
use compose_ui_graphics::Size;
use futures_task::{waker, ArcWake};
use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::collections::{hash_map::DefaultHasher, HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

impl Modifier {
    pub fn pointer_input<K, F, Fut>(self, key: K, handler: F) -> Self
    where
        K: Hash + 'static,
        F: Fn(PointerInputScope) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let element =
            PointerInputElement::new(vec![KeyToken::new(&key)], pointer_input_handler(handler));
        let key_count = element.key_count();
        let handler_id = element.handler_id();
        self.then(
            Self::with_element(element).with_inspector_metadata(inspector_metadata(
                "pointerInput",
                move |info| {
                    info.add_property("keyCount", key_count.to_string());
                    info.add_property("handlerId", handler_id.to_string());
                },
            )),
        )
    }
}

fn pointer_input_handler<F, Fut>(handler: F) -> PointerInputHandler
where
    F: Fn(PointerInputScope) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    Rc::new(move |scope| Box::pin(handler(scope.clone())))
}

type PointerInputFuture = Pin<Box<dyn Future<Output = ()>>>;
type PointerInputHandler = Rc<dyn Fn(PointerInputScope) -> PointerInputFuture>;

thread_local! {
    static POINTER_INPUT_TASKS: RefCell<HashMap<u64, Rc<PointerInputTaskInner>>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
struct PointerInputElement {
    keys: Vec<KeyToken>,
    handler: PointerInputHandler,
    handler_id: u64,
}

impl PointerInputElement {
    fn new(keys: Vec<KeyToken>, handler: PointerInputHandler) -> Self {
        static NEXT_HANDLER_ID: AtomicU64 = AtomicU64::new(1);
        let handler_id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            keys,
            handler,
            handler_id,
        }
    }

    fn key_count(&self) -> usize {
        self.keys.len()
    }

    fn handler_id(&self) -> u64 {
        self.handler_id
    }
}

impl fmt::Debug for PointerInputElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerInputElement")
            .field("keys", &self.keys)
            .field("handler", &Rc::as_ptr(&self.handler))
            .field("handler_id", &self.handler_id)
            .finish()
    }
}

impl PartialEq for PointerInputElement {
    fn eq(&self, other: &Self) -> bool {
        // Only compare keys, not handler_id. In Compose, elements are equal if their
        // keys match, even if the handler closure is recreated on recomposition.
        // This ensures nodes are reused instead of being dropped and recreated.
        self.keys == other.keys
    }
}

impl Eq for PointerInputElement {}

impl Hash for PointerInputElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Only hash keys, not handler_id. This ensures stable hashing across
        // recompositions when the closure is recreated but keys remain the same.
        self.keys.hash(state);
    }
}

impl ModifierNodeElement for PointerInputElement {
    type Node = SuspendingPointerInputNode;

    fn create(&self) -> Self::Node {
        SuspendingPointerInputNode::new(self.keys.clone(), self.handler.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.update(self.keys.clone(), self.handler.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

#[derive(Clone)]
pub struct PointerInputScope {
    state: Rc<PointerInputScopeState>,
}

impl PointerInputScope {
    fn new(state: Rc<PointerInputScopeState>) -> Self {
        Self { state }
    }

    pub fn size(&self) -> Size {
        self.state.size.get()
    }

    pub async fn await_pointer_event_scope<R, F, Fut>(&self, block: F) -> R
    where
        F: FnOnce(AwaitPointerEventScope) -> Fut,
        Fut: Future<Output = R>,
    {
        let scope = AwaitPointerEventScope {
            state: self.state.clone(),
        };
        block(scope).await
    }
}

#[derive(Clone)]
pub struct AwaitPointerEventScope {
    state: Rc<PointerInputScopeState>,
}

impl AwaitPointerEventScope {
    pub fn size(&self) -> Size {
        self.state.size.get()
    }

    pub async fn await_pointer_event(&self) -> PointerEvent {
        NextPointerEvent {
            state: self.state.clone(),
        }
        .await
    }

    pub async fn with_timeout_or_null<R, F, Fut>(&self, _time_millis: u64, block: F) -> Option<R>
    where
        F: FnOnce(&AwaitPointerEventScope) -> Fut,
        Fut: Future<Output = R>,
    {
        Some(block(self).await)
    }

    pub async fn with_timeout<R, F, Fut>(&self, _time_millis: u64, block: F) -> R
    where
        F: FnOnce(&AwaitPointerEventScope) -> Fut,
        Fut: Future<Output = R>,
    {
        block(self).await
    }
}

struct NextPointerEvent {
    state: Rc<PointerInputScopeState>,
}

impl Future for NextPointerEvent {
    type Output = PointerEvent;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.state.poll_event(cx)
    }
}

struct PointerInputScopeState {
    events: RefCell<VecDeque<PointerEvent>>,
    waiting: RefCell<Option<Waker>>,
    size: Cell<Size>,
}

impl PointerInputScopeState {
    fn new() -> Self {
        Self {
            events: RefCell::new(VecDeque::new()),
            waiting: RefCell::new(None),
            size: Cell::new(Size {
                width: 0.0,
                height: 0.0,
            }),
        }
    }

    fn push_event(&self, event: PointerEvent) {
        self.events.borrow_mut().push_back(event);
        let waker = {
            let mut waiting = self.waiting.borrow_mut();
            waiting.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn poll_event(&self, cx: &mut Context<'_>) -> Poll<PointerEvent> {
        if let Some(event) = self.events.borrow_mut().pop_front() {
            Poll::Ready(event)
        } else {
            self.waiting.replace(Some(cx.waker().clone()));
            Poll::Pending
        }
    }
}

struct PointerEventDispatcher {
    state: Rc<RefCell<Option<Rc<PointerInputScopeState>>>>,
    handler: Rc<dyn Fn(PointerEvent)>,
}

impl PointerEventDispatcher {
    fn new() -> Self {
        let state = Rc::new(RefCell::new(None::<Rc<PointerInputScopeState>>));
        let state_for_handler = state.clone();
        let handler = Rc::new(move |event: PointerEvent| {
            if let Some(inner) = state_for_handler.borrow().as_ref() {
                inner.push_event(event);
            }
        });
        Self { state, handler }
    }

    fn handler(&self) -> Rc<dyn Fn(PointerEvent)> {
        self.handler.clone()
    }

    fn set_state(&self, state: Option<Rc<PointerInputScopeState>>) {
        *self.state.borrow_mut() = state;
    }
}

struct PointerInputTask {
    id: u64,
    inner: Rc<PointerInputTaskInner>,
}

impl PointerInputTask {
    fn new(future: PointerInputFuture) -> Self {
        static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
        let inner = Rc::new(PointerInputTaskInner::new(future));
        POINTER_INPUT_TASKS.with(|registry| {
            registry.borrow_mut().insert(id, inner.clone());
        });
        Self { id, inner }
    }

    fn poll(&self) {
        self.inner.poll(self.id);
    }

    fn cancel(self) {
        self.inner.cancel();
        POINTER_INPUT_TASKS.with(|registry| {
            registry.borrow_mut().remove(&self.id);
        });
    }
}

impl Drop for PointerInputTask {
    fn drop(&mut self) {
        // Don't remove from registry here! The registry holds a strong Rc<PointerInputTaskInner>,
        // so the inner will stay alive even if this PointerInputTask wrapper is dropped.
        // This is intentional - tasks created by temporary modifier chains (used for slice collection)
        // will have their PointerInputTask dropped, but the inner task needs to stay alive in the
        // registry so that wakers can still find and wake it.
        // Tasks are only removed from the registry when explicitly cancelled via cancel().
    }
}

struct PointerInputTaskInner {
    future: RefCell<Option<PointerInputFuture>>,
    is_polling: Cell<bool>,
    needs_poll: Cell<bool>,
}

impl PointerInputTaskInner {
    fn new(future: PointerInputFuture) -> Self {
        Self {
            future: RefCell::new(Some(future)),
            is_polling: Cell::new(false),
            needs_poll: Cell::new(false),
        }
    }

    fn cancel(&self) {
        self.future.borrow_mut().take();
    }

    fn request_poll(&self, task_id: u64) {
        if self.is_polling.get() {
            self.needs_poll.set(true);
        } else {
            self.poll(task_id);
        }
    }

    fn poll(&self, task_id: u64) {
        if self.is_polling.replace(true) {
            self.needs_poll.set(true);
            return;
        }
        loop {
            self.needs_poll.set(false);
            let waker = waker(Arc::new(PointerInputTaskWaker { task_id }));
            let mut cx = Context::from_waker(&waker);
            let mut future_slot = self.future.borrow_mut();
            if let Some(future) = future_slot.as_mut() {
                let poll_result = future.as_mut().poll(&mut cx);
                if poll_result.is_ready() {
                    future_slot.take();
                }
            }
            if !self.needs_poll.get() {
                break;
            }
        }
        self.is_polling.set(false);
    }
}

struct PointerInputTaskWaker {
    task_id: u64,
}

impl ArcWake for PointerInputTaskWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        POINTER_INPUT_TASKS.with(|registry| {
            if let Some(task) = registry.borrow().get(&arc_self.task_id).cloned() {
                task.request_poll(arc_self.task_id);
            }
        });
    }
}

pub struct SuspendingPointerInputNode {
    keys: Vec<KeyToken>,
    handler: PointerInputHandler,
    dispatcher: PointerEventDispatcher,
    task: Option<PointerInputTask>,
    state: NodeState,
}

impl SuspendingPointerInputNode {
    fn new(keys: Vec<KeyToken>, handler: PointerInputHandler) -> Self {
        Self {
            keys,
            handler,
            dispatcher: PointerEventDispatcher::new(),
            task: None,
            state: NodeState::new(),
        }
    }

    fn update(&mut self, keys: Vec<KeyToken>, handler: PointerInputHandler) {
        // Only restart if keys changed - not if handler Rc pointer changed.
        // In Compose, closures are recreated every composition but the task should
        // continue running as long as the keys are the same. This matches Jetpack
        // Compose behavior where rememberUpdatedState keeps the task alive.
        let should_restart = self.keys != keys;
        self.keys = keys;
        self.handler = handler; // Update handler even if not restarting
        if should_restart {
            self.restart();
        }
    }

    fn restart(&mut self) {
        self.cancel();
        self.start();
    }

    fn start(&mut self) {
        let state = Rc::new(PointerInputScopeState::new());
        self.dispatcher.set_state(Some(state.clone()));
        let scope = PointerInputScope::new(state);
        let future = (self.handler)(scope);
        let task = PointerInputTask::new(future);
        task.poll();
        self.task = Some(task);
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel();
        }
        self.dispatcher.set_state(None);
    }
}

impl Drop for SuspendingPointerInputNode {
    fn drop(&mut self) {
        // Cleanup happens automatically when task field is dropped
    }
}

impl ModifierNode for SuspendingPointerInputNode {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
        self.start();
    }

    fn on_detach(&mut self) {
        self.cancel();
    }

    fn on_reset(&mut self) {
        // Don't restart on reset - only restart when keys/handler actually change
        // (which is handled by update() method). Restarting here would kill the
        // active task and lose its registered waker, preventing events from being delivered.
    }

    // Capability-driven implementation using helper macro
    impl_pointer_input_node!();
}

impl DelegatableNode for SuspendingPointerInputNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl PointerInputNode for SuspendingPointerInputNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(self.dispatcher.handler())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct KeyToken {
    type_id: TypeId,
    hash: u64,
}

impl KeyToken {
    fn new<T: Hash + 'static>(value: &T) -> Self {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        Self {
            type_id: TypeId::of::<T>(),
            hash: hasher.finish(),
        }
    }
}
