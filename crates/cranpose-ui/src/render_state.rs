#[cfg(test)]
use std::sync::OnceLock;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

use cranpose_core::{NodeId, SnapshotStateObserver, current_runtime_handle};

pub(crate) type ModifierChainTraceCallback =
    dyn Fn(&[crate::modifier::ModifierChainInspectorNode]) + Send + Sync + 'static;

struct RenderState {
    layout_repasses: Mutex<LayoutRepassManager>,
    measure_repasses: Mutex<LayoutRepassManager>,
    draw_repasses: Mutex<DrawRepassManager>,
    modifier_slice_repasses: Mutex<LayoutRepassManager>,
    geometry_scene_nodes: Mutex<LayoutRepassManager>,
    render_invalidated: AtomicBool,
    pointer_invalidated: AtomicBool,
    focus_invalidated: AtomicBool,
    layout_invalidated: AtomicBool,
    density_bits: AtomicU32,
    font_scale: Mutex<crate::font_scale::FontScaleCurve>,
}

#[doc(hidden)]
pub struct AppContext {
    id: AppContextId,
    self_weak: RefCell<Weak<AppContext>>,
    state: RenderState,
    draw_observer: SnapshotStateObserver,
    text: crate::text::measure::TextService,
    layout_frame_arena: RefCell<crate::layout::FrameLayoutArena>,
    layout_cache_epoch: AtomicU64,
    last_fling_velocity_bits: AtomicU32,
    scroll_motion_contexts: crate::scroll::ScrollMotionContextStore,
    layout_node_registry: crate::widgets::nodes::layout_node::LayoutNodeRegistryState,
    pointer_dispatch: crate::pointer_dispatch::PointerDispatchState,
    focus_dispatch: crate::focus_dispatch::FocusInvalidationState,
    semantics_dispatch: crate::semantics_dispatch::SemanticsInvalidationState,
    cursor_animation: crate::cursor_animation::CursorAnimationState,
    text_field_focus: crate::text_field_focus::TextFieldFocusState,
    text_input_session: crate::text_input_session::PlatformTextInputState,
    clipboard_session: crate::clipboard_session::ClipboardSessionState,
    pointer_input_tasks: crate::modifier::pointer_input::PointerInputTaskRegistry,
    modifier_chain_trace: RefCell<Option<Arc<ModifierChainTraceCallback>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AppContextId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawObservationScope {
    node_id: NodeId,
    command_index: usize,
}

impl DrawObservationScope {
    pub(crate) fn new(node_id: NodeId, command_index: usize) -> Self {
        Self {
            node_id,
            command_index,
        }
    }
}

fn new_draw_observer() -> SnapshotStateObserver {
    let observer = SnapshotStateObserver::new(|callback| {
        if let Some(runtime) = current_runtime_handle() {
            runtime.enqueue_ui_task(callback);
        } else {
            callback();
        }
    });
    observer.start();
    observer
}

thread_local! {
    static CURRENT_DRAW_NODE: std::cell::Cell<Option<NodeId>> = const { std::cell::Cell::new(None) };
}

struct CurrentDrawNodeGuard {
    previous: Option<NodeId>,
}

impl Drop for CurrentDrawNodeGuard {
    fn drop(&mut self) {
        CURRENT_DRAW_NODE.with(|current| current.set(self.previous));
    }
}

pub(crate) fn observe_draw_reads<R>(scope: DrawObservationScope, block: impl FnOnce() -> R) -> R {
    let context = require_current_app_context("draw observer access");
    let context_id = context.id;
    let _guard = CurrentDrawNodeGuard {
        previous: CURRENT_DRAW_NODE.with(|current| current.replace(Some(scope.node_id))),
    };
    context.draw_observer.observe_reads(
        scope,
        move |scope| {
            schedule_draw_repass_for_app_context(context_id, scope.node_id);
        },
        block,
    )
}

/// The draw-phase animation contract: a draw closure that advances its own
/// spring or clock (a Cell no observation can see) must schedule the next
/// frame's re-record of ITS OWN node — a bare render invalidation only
/// re-presents the retained scene, which would freeze the animation
/// mid-flight. Callable only from inside a recording draw closure; anywhere
/// else it degrades to a plain frame request.
pub fn request_current_draw_redraw() {
    if let Some(node_id) = CURRENT_DRAW_NODE.with(std::cell::Cell::get) {
        schedule_draw_repass(node_id);
    }
    request_render_invalidation();
}

pub(crate) fn clear_draw_observations_for_node(node_id: NodeId) {
    with_draw_observer(|observer| {
        observer.clear_if(|scope| {
            scope
                .downcast_ref::<DrawObservationScope>()
                .is_some_and(|scope| scope.node_id == node_id)
        });
    });
}

pub fn prune_draw_observations_to_nodes(retained: &HashSet<NodeId>) {
    with_draw_observer(|observer| {
        observer.clear_if(|scope| {
            scope
                .downcast_ref::<DrawObservationScope>()
                .is_some_and(|scope| !retained.contains(&scope.node_id))
        });
    });
}

impl RenderState {
    fn new_with_density(density: f32) -> Self {
        Self {
            layout_repasses: Mutex::new(LayoutRepassManager::new()),
            measure_repasses: Mutex::new(LayoutRepassManager::new()),
            draw_repasses: Mutex::new(DrawRepassManager::new()),
            modifier_slice_repasses: Mutex::new(LayoutRepassManager::new()),
            geometry_scene_nodes: Mutex::new(LayoutRepassManager::new()),
            render_invalidated: AtomicBool::new(false),
            pointer_invalidated: AtomicBool::new(false),
            focus_invalidated: AtomicBool::new(false),
            layout_invalidated: AtomicBool::new(false),
            density_bits: AtomicU32::new(normalize_density(density).to_bits()),
            font_scale: Mutex::new(crate::font_scale::FontScaleCurve::linear(1.0)),
        }
    }
}

std::thread_local! {
    static NEXT_APP_CONTEXT_ID: Cell<u64> = const { Cell::new(1) };
    static CURRENT_APP_CONTEXT: RefCell<Vec<Weak<AppContext>>> = const { RefCell::new(Vec::new()) };
    static APP_CONTEXTS: RefCell<HashMap<AppContextId, Weak<AppContext>>> = RefCell::new(HashMap::new());
}

fn next_app_context_id() -> AppContextId {
    NEXT_APP_CONTEXT_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        AppContextId(id)
    })
}

#[doc(hidden)]
pub struct AppContextScope;

impl Drop for AppContextScope {
    fn drop(&mut self) {
        CURRENT_APP_CONTEXT.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

impl AppContext {
    pub fn new() -> Rc<Self> {
        Self::new_with_density(1.0)
    }

    pub fn new_with_density(density: f32) -> Rc<Self> {
        let context = Rc::new(Self {
            id: next_app_context_id(),
            self_weak: RefCell::new(Weak::new()),
            state: RenderState::new_with_density(density),
            draw_observer: new_draw_observer(),
            text: crate::text::measure::TextService::new(),
            layout_frame_arena: RefCell::new(crate::layout::FrameLayoutArena::default()),
            layout_cache_epoch: AtomicU64::new(1),
            last_fling_velocity_bits: AtomicU32::new(0.0f32.to_bits()),
            scroll_motion_contexts: crate::scroll::ScrollMotionContextStore::new(),
            layout_node_registry: crate::widgets::nodes::layout_node::LayoutNodeRegistryState::new(
            ),
            pointer_dispatch: crate::pointer_dispatch::PointerDispatchState::new(),
            focus_dispatch: crate::focus_dispatch::FocusInvalidationState::new(),
            semantics_dispatch: crate::semantics_dispatch::SemanticsInvalidationState::new(),
            cursor_animation: crate::cursor_animation::CursorAnimationState::new(),
            text_field_focus: crate::text_field_focus::TextFieldFocusState::new(),
            text_input_session: crate::text_input_session::PlatformTextInputState::new(),
            clipboard_session: crate::clipboard_session::ClipboardSessionState::new(),
            pointer_input_tasks: crate::modifier::pointer_input::PointerInputTaskRegistry::new(),
            modifier_chain_trace: RefCell::new(None),
        });
        *context.self_weak.borrow_mut() = Rc::downgrade(&context);
        APP_CONTEXTS.with(|contexts| {
            contexts
                .borrow_mut()
                .insert(context.id, Rc::downgrade(&context));
        });
        context
    }

    pub fn enter<R>(self: &Rc<Self>, block: impl FnOnce() -> R) -> R {
        let _scope = self.enter_scope();
        block()
    }

    #[doc(hidden)]
    pub fn enter_scope(self: &Rc<Self>) -> AppContextScope {
        CURRENT_APP_CONTEXT.with(|stack| {
            stack.borrow_mut().push(Rc::downgrade(self));
        });
        AppContextScope
    }

    pub fn set_text_measurer<M: crate::text::TextMeasurer>(&self, measurer: M) {
        self.set_text_measurer_rc(Rc::new(measurer));
    }

    pub fn set_text_measurer_rc(&self, measurer: Rc<dyn crate::text::TextMeasurer>) {
        self.text.set_measurer(measurer);
        self.layout_cache_epoch.fetch_add(1, Ordering::Relaxed);
        self.state.layout_invalidated.store(true, Ordering::Relaxed);
        self.state.render_invalidated.store(true, Ordering::Relaxed);
    }

    #[doc(hidden)]
    pub fn downgrade(&self) -> Weak<Self> {
        self.self_weak.borrow().clone()
    }
}

impl Drop for AppContext {
    fn drop(&mut self) {
        let id = self.id;
        let _ = APP_CONTEXTS.try_with(|contexts| {
            contexts.borrow_mut().remove(&id);
        });
    }
}

fn app_context_by_id(id: AppContextId) -> Option<Rc<AppContext>> {
    APP_CONTEXTS
        .try_with(|contexts| {
            let context = contexts.borrow().get(&id).cloned()?;
            let Some(context) = context.upgrade() else {
                contexts.borrow_mut().remove(&id);
                return None;
            };
            Some(context)
        })
        .ok()
        .flatten()
}

#[cfg(test)]
fn app_context_registry_entry_count() -> usize {
    APP_CONTEXTS
        .try_with(|contexts| contexts.borrow().len())
        .unwrap_or_default()
}

fn with_app_context_by_id<R>(id: AppContextId, f: impl FnOnce(&Rc<AppContext>) -> R) -> Option<R> {
    app_context_by_id(id).map(|context| f(&context))
}

pub(crate) fn current_app_context_id() -> AppContextId {
    require_current_app_context("app context identity access").id
}

pub(crate) fn with_layout_node_registry_by_app_context<R>(
    id: AppContextId,
    f: impl FnOnce(&crate::widgets::nodes::layout_node::LayoutNodeRegistryState) -> R,
) -> Option<R> {
    with_app_context_by_id(id, |context| f(&context.layout_node_registry))
}

pub(crate) fn enter_app_context_by_id<R>(id: AppContextId, f: impl FnOnce() -> R) -> Option<R> {
    with_app_context_by_id(id, |context| context.enter(f))
}

pub(crate) fn current_app_context() -> Option<Rc<AppContext>> {
    CURRENT_APP_CONTEXT
        .try_with(|stack| {
            let mut stack = stack.borrow_mut();
            loop {
                let context = stack.last()?;
                if let Some(context) = context.upgrade() {
                    return Some(context);
                }
                stack.pop();
            }
        })
        .ok()
        .flatten()
}

#[doc(hidden)]
pub fn has_current_app_context() -> bool {
    current_app_context().is_some()
}

fn require_current_app_context(operation: &str) -> Rc<AppContext> {
    if let Some(context) = current_app_context() {
        return context;
    }
    require_current_app_context_without_scope(operation)
}

fn require_current_app_context_without_scope(operation: &str) -> Rc<AppContext> {
    panic!("{operation} requires an active AppContext")
}

fn with_render_state<R>(f: impl FnOnce(&RenderState) -> R) -> R {
    let context = require_current_app_context("render state access");
    f(&context.state)
}

fn normalize_density(density: f32) -> f32 {
    if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    }
}

fn normalize_font_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale.clamp(MIN_FONT_SCALE, MAX_FONT_SCALE)
    } else {
        1.0
    }
}

/// Smallest system font scale honoured; below this, text stops being readable
/// as text.
pub const MIN_FONT_SCALE: f32 = 0.5;
/// Largest system font scale honoured. Android's own accessibility slider tops
/// out at 2.0.
pub const MAX_FONT_SCALE: f32 = 3.0;

pub(crate) fn with_text_measurer<R>(f: impl FnOnce(&dyn crate::text::TextMeasurer) -> R) -> R {
    let context = require_current_app_context("text measurer access");
    context.text.with_measurer(f)
}

pub(crate) fn with_text_service<R>(f: impl FnOnce(&crate::text::measure::TextService) -> R) -> R {
    let context = require_current_app_context("text service access");
    f(&context.text)
}

pub(crate) fn set_current_text_measurer(measurer: Rc<dyn crate::text::TextMeasurer>) {
    let Some(context) = current_app_context() else {
        panic!("set_text_measurer requires an active AppContext");
    };
    context.set_text_measurer_rc(measurer);
}

pub(crate) fn set_modifier_chain_trace(callback: Arc<ModifierChainTraceCallback>) -> AppContextId {
    let context = require_current_app_context("modifier chain trace installation");
    *context.modifier_chain_trace.borrow_mut() = Some(callback);
    context.id
}

pub(crate) fn clear_modifier_chain_trace(context_id: AppContextId) {
    let _ = with_app_context_by_id(context_id, |context| {
        *context.modifier_chain_trace.borrow_mut() = None;
    });
}

pub(crate) fn emit_modifier_chain_trace(nodes: &[crate::modifier::ModifierChainInspectorNode]) {
    let Some(context) = current_app_context() else {
        return;
    };
    let callback = context.modifier_chain_trace.borrow().clone();
    if let Some(callback) = callback {
        callback(nodes);
    }
}

pub(crate) fn take_layout_frame_arena() -> crate::layout::FrameLayoutArena {
    let context = require_current_app_context("layout frame arena access");

    std::mem::take(&mut *context.layout_frame_arena.borrow_mut())
}

pub(crate) fn replace_layout_frame_arena(arena: crate::layout::FrameLayoutArena) {
    let context = require_current_app_context("layout frame arena access");
    *context.layout_frame_arena.borrow_mut() = arena;
}

pub(crate) fn invalidate_layout_cache_epoch() {
    let context = require_current_app_context("layout cache epoch access");
    context.layout_cache_epoch.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn next_layout_cache_epoch() -> u64 {
    let context = require_current_app_context("layout cache epoch access");
    context.layout_cache_epoch.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn current_layout_cache_epoch() -> u64 {
    let context = require_current_app_context("layout cache epoch access");
    context.layout_cache_epoch.load(Ordering::Relaxed)
}

pub(crate) fn record_last_fling_velocity(velocity: f32) {
    if let Some(context) = current_app_context() {
        context
            .last_fling_velocity_bits
            .store(velocity.to_bits(), Ordering::Relaxed);
    }
}

#[doc(hidden)]
pub fn debug_last_fling_velocity() -> f32 {
    let context = require_current_app_context("fling velocity diagnostics access");
    f32::from_bits(context.last_fling_velocity_bits.load(Ordering::Relaxed))
}

#[doc(hidden)]
pub fn debug_reset_last_fling_velocity() {
    let context = require_current_app_context("fling velocity diagnostics access");
    context
        .last_fling_velocity_bits
        .store(0.0f32.to_bits(), Ordering::Relaxed);
}

pub(crate) fn with_scroll_motion_context_store<R>(
    f: impl FnOnce(&crate::scroll::ScrollMotionContextStore) -> R,
) -> R {
    let context = require_current_app_context("scroll motion context access");
    f(&context.scroll_motion_contexts)
}

#[doc(hidden)]
pub fn clear_transient_scroll_motion_contexts() {
    let Some(context) = current_app_context() else {
        return;
    };
    context.scroll_motion_contexts.clear_transient_after_frame();
}

#[cfg(test)]
pub(crate) fn layout_frame_arena_placement_scratch_count() -> usize {
    let context = require_current_app_context("layout frame arena access");

    context
        .layout_frame_arena
        .borrow()
        .available_placement_scratch_count()
}

pub(crate) fn with_layout_node_registry<R>(
    f: impl FnOnce(&crate::widgets::nodes::layout_node::LayoutNodeRegistryState) -> R,
) -> R {
    let context = require_current_app_context("layout node registry access");
    f(&context.layout_node_registry)
}

pub(crate) fn with_pointer_dispatch<R>(
    f: impl FnOnce(&crate::pointer_dispatch::PointerDispatchState) -> R,
) -> R {
    let context = require_current_app_context("pointer dispatch access");
    f(&context.pointer_dispatch)
}

pub(crate) fn with_focus_dispatch<R>(
    f: impl FnOnce(&crate::focus_dispatch::FocusInvalidationState) -> R,
) -> R {
    let context = require_current_app_context("focus dispatch access");
    f(&context.focus_dispatch)
}

pub(crate) fn with_semantics_dispatch<R>(
    f: impl FnOnce(&crate::semantics_dispatch::SemanticsInvalidationState) -> R,
) -> R {
    let context = require_current_app_context("semantics dispatch access");
    f(&context.semantics_dispatch)
}

pub(crate) fn with_semantics_dispatch_by_app_context(
    id: AppContextId,
    f: impl FnOnce(&crate::semantics_dispatch::SemanticsInvalidationState),
) {
    with_app_context_by_id(id, |context| f(&context.semantics_dispatch));
}

pub(crate) fn current_app_context_id_opt() -> Option<AppContextId> {
    current_app_context().map(|context| context.id)
}

pub(crate) fn with_cursor_animation<R>(
    f: impl FnOnce(&crate::cursor_animation::CursorAnimationState) -> R,
) -> R {
    let context = require_current_app_context("cursor animation access");
    f(&context.cursor_animation)
}

pub(crate) fn with_text_field_focus<R>(
    f: impl FnOnce(&crate::text_field_focus::TextFieldFocusState) -> R,
) -> R {
    let context = require_current_app_context("text field focus access");
    f(&context.text_field_focus)
}

pub(crate) fn with_text_input_session<R>(
    f: impl FnOnce(&crate::text_input_session::PlatformTextInputState) -> R,
) -> R {
    let context = require_current_app_context("platform text input session access");
    f(&context.text_input_session)
}

pub(crate) fn with_clipboard_session<R>(
    f: impl FnOnce(&crate::clipboard_session::ClipboardSessionState) -> R,
) -> R {
    let context = require_current_app_context("clipboard session access");
    f(&context.clipboard_session)
}

pub(crate) fn register_pointer_input_task(
    task_id: u64,
    task: Rc<crate::modifier::pointer_input::PointerInputTaskInner>,
) -> crate::modifier::pointer_input::PointerInputTaskOwner {
    let context = require_current_app_context("pointer input task registration");
    context.pointer_input_tasks.insert(task_id, task);
    crate::modifier::pointer_input::PointerInputTaskOwner::App(context.id)
}

pub(crate) fn remove_pointer_input_task(
    owner: crate::modifier::pointer_input::PointerInputTaskOwner,
    task_id: u64,
) {
    match owner {
        crate::modifier::pointer_input::PointerInputTaskOwner::App(context_id) => {
            let _ = with_app_context_by_id(context_id, |context| {
                context.pointer_input_tasks.remove(task_id);
            });
        }
    }
}

pub(crate) fn request_pointer_input_task_poll(
    owner: crate::modifier::pointer_input::PointerInputTaskOwner,
    task_id: u64,
) {
    match owner {
        crate::modifier::pointer_input::PointerInputTaskOwner::App(context_id) => {
            let _ = with_app_context_by_id(context_id, |context| {
                context.enter(|| {
                    context.pointer_input_tasks.request_poll(task_id, owner);
                });
            });
        }
    }
}

fn with_draw_observer<R>(f: impl FnOnce(&SnapshotStateObserver) -> R) -> R {
    let context = require_current_app_context("draw observer access");
    f(&context.draw_observer)
}

struct LayoutRepassManager {
    dirty_nodes: HashSet<NodeId>,
}

impl LayoutRepassManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
        }
    }

    fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        self.dirty_nodes.drain().collect()
    }

    fn dirty_nodes_snapshot(&self) -> Vec<NodeId> {
        let mut nodes = self.dirty_nodes.iter().copied().collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes
    }
}

struct DrawRepassManager {
    dirty_nodes: HashSet<NodeId>,
}

impl DrawRepassManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
        }
    }

    fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        self.dirty_nodes.drain().collect()
    }
}

fn lock_repass_manager<T>(manager: &Mutex<T>) -> MutexGuard<'_, T> {
    manager
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Schedules a layout repass for a specific node.
///
/// **This is the preferred way to invalidate layout for local changes** (e.g., scroll, single-node mutations).
///
/// The app shell will call `take_layout_repass_nodes()` and bubble dirty flags up the tree
/// via `bubble_layout_dirty`. This gives you **O(subtree) performance** - only the affected
/// subtree is remeasured, and layout caches for other parts of the app remain valid.
///
/// # Implementation Note
///
/// This sets the `LAYOUT_INVALIDATED` flag to signal the app shell there's work to do,
/// but the flag alone does NOT trigger global cache invalidation. The app shell checks
/// `take_layout_repass_nodes()` first and processes scoped repasses. Global cache invalidation
/// only happens if the flag is set AND there are no scoped repasses (a rare fallback case).
///
/// # For Global Invalidation
///
/// For rare global events (window resize, global scale changes), use `request_layout_invalidation()` instead.
#[track_caller]
pub fn schedule_layout_repass(node_id: NodeId) {
    if layout_repass_schedule_diagnostics_enabled_for(node_id) {
        let caller = std::panic::Location::caller();
        log::warn!(
            "[layout-repass-schedule] node={} caller={}:{}:{}",
            node_id,
            caller.file(),
            caller.line(),
            caller.column()
        );
    }
    with_render_state(|state| {
        lock_repass_manager(&state.layout_repasses).schedule_repass(node_id);
        state.layout_invalidated.store(true, Ordering::Relaxed);
    });
    request_render_invalidation();
}

#[derive(Clone, Copy)]
enum LayoutRepassScheduleDiag {
    Disabled,
    All,
    Node(NodeId),
}

fn layout_repass_schedule_diagnostics_enabled_for(node_id: NodeId) -> bool {
    static MODE: std::sync::OnceLock<LayoutRepassScheduleDiag> = std::sync::OnceLock::new();
    match *MODE.get_or_init(|| {
        let Some(value) = std::env::var_os("CRANPOSE_LAYOUT_REPASS_SCHEDULE_DIAG") else {
            return LayoutRepassScheduleDiag::Disabled;
        };
        if value == "all" {
            return LayoutRepassScheduleDiag::All;
        }
        value
            .to_string_lossy()
            .parse::<NodeId>()
            .map(LayoutRepassScheduleDiag::Node)
            .unwrap_or(LayoutRepassScheduleDiag::Disabled)
    }) {
        LayoutRepassScheduleDiag::Disabled => false,
        LayoutRepassScheduleDiag::All => true,
        LayoutRepassScheduleDiag::Node(target) => target == node_id,
    }
}

pub(crate) fn schedule_modifier_slices_repass(node_id: NodeId) {
    with_render_state(|state| {
        lock_repass_manager(&state.modifier_slice_repasses).schedule_repass(node_id);
    });
    schedule_draw_repass(node_id);
}

/// Schedules a draw-only repass for a specific node.
///
/// This ensures draw/pointer data stays in sync when modifier updates do not
/// require a layout pass (e.g., draw-only modifier changes).
pub fn schedule_draw_repass(node_id: NodeId) {
    let context = require_current_app_context("render state access");
    schedule_draw_repass_in_context(&context, node_id);
}

fn schedule_draw_repass_for_app_context(context_id: AppContextId, node_id: NodeId) {
    let _ = with_app_context_by_id(context_id, |context| {
        schedule_draw_repass_in_context(context, node_id);
    });
}

fn schedule_draw_repass_in_context(context: &AppContext, node_id: NodeId) {
    lock_repass_manager(&context.state.draw_repasses).schedule_repass(node_id);
    context
        .state
        .render_invalidated
        .store(true, Ordering::Relaxed);
}

/// Returns true if any draw repasses are pending.
pub fn has_pending_draw_repasses() -> bool {
    with_render_state(|state| lock_repass_manager(&state.draw_repasses).has_pending_repass())
}

/// Takes all pending draw repass node IDs.
pub fn take_draw_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.draw_repasses).take_dirty_nodes())
}

/// Returns true if any layout repasses are pending.
pub fn has_pending_layout_repasses() -> bool {
    with_render_state(|state| lock_repass_manager(&state.layout_repasses).has_pending_repass())
}

/// Returns a stable snapshot of pending layout repass node IDs without consuming them.
pub fn pending_layout_repass_nodes_snapshot() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.layout_repasses).dirty_nodes_snapshot())
}

/// Takes all pending layout repass node IDs.
///
/// The caller should iterate over these and call `bubble_layout_dirty` for each.
pub fn take_layout_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.layout_repasses).take_dirty_nodes())
}

/// Schedules a scoped re-*measure* of `node_id` on the next frame.
///
/// Like [`schedule_layout_repass`], but processing bubbles *measure* dirtiness
/// (not just layout/placement) up the tree, so the node and its ancestors are
/// re-measured. Use this when a node's own measured size changes off a frame
/// callback (e.g. a row collapsing after a swipe dismiss): a plain layout
/// repass would leave the node's `needs_measure` flag unset, and an enclosing
/// `LazyColumn` would reuse its cached, full-height item slot.
pub fn schedule_measure_repass(node_id: NodeId) {
    with_render_state(|state| {
        lock_repass_manager(&state.measure_repasses).schedule_repass(node_id);
        state.layout_invalidated.store(true, Ordering::Relaxed);
    });
    request_render_invalidation();
}

/// Returns true if any measure repasses are pending.
pub fn has_pending_measure_repasses() -> bool {
    with_render_state(|state| lock_repass_manager(&state.measure_repasses).has_pending_repass())
}

/// Returns a stable snapshot of pending measure repass node IDs without
/// consuming them.
///
/// The layout pass takes these ids to bubble measure dirtiness; the scene phase
/// needs the same ids *before* that happens, to scope its graph update to the
/// subtree that moved. Without the snapshot a measure repass reaches the scene
/// phase as "something changed, but nothing says where", which is
/// indistinguishable from a full invalidation.
pub fn pending_measure_repass_nodes_snapshot() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.measure_repasses).dirty_nodes_snapshot())
}

/// Takes all pending measure repass node IDs.
///
/// The caller should iterate over these and call `bubble_measure_dirty` for each.
pub fn take_measure_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.measure_repasses).take_dirty_nodes())
}

pub(crate) fn take_modifier_slice_repass_nodes() -> Vec<NodeId> {
    with_render_state(|state| {
        lock_repass_manager(&state.modifier_slice_repasses).take_dirty_nodes()
    })
}

pub(crate) fn record_geometry_scene_node(node_id: NodeId) {
    with_render_state(|state| {
        lock_repass_manager(&state.geometry_scene_nodes).schedule_repass(node_id);
    });
}

/// Takes the nodes whose geometry the last layout pass actually changed.
///
/// The scene phase merges these into its scoped update scope. Consuming them
/// is mandatory whenever layout ran: geometry recorded by one pass is
/// meaningless to the next.
pub fn take_geometry_scene_nodes() -> Vec<NodeId> {
    with_render_state(|state| lock_repass_manager(&state.geometry_scene_nodes).take_dirty_nodes())
}

/// Returns the current density scale factor (logical px per dp).
pub fn current_density() -> f32 {
    with_render_state(|state| f32::from_bits(state.density_bits.load(Ordering::Relaxed)))
}

/// Updates the current density scale factor.
///
/// This triggers a global layout invalidation when the value changes because
/// density impacts layout, text measurement, and input thresholds.
pub fn set_density(density: f32) {
    let normalized = normalize_density(density);
    let new_bits = normalized.to_bits();
    with_render_state(|state| {
        let old_bits = state.density_bits.swap(new_bits, Ordering::Relaxed);
        if old_bits != new_bits {
            state.layout_invalidated.store(true, Ordering::Relaxed);
        }
    });
}

/// Returns the system font scale — the multiplier the user chose in the
/// platform's font-size setting, `1.0` when they left it alone.
///
/// This is the setting `Sp` is defined against, so text follows it while
/// everything measured in `Dp` does not. A platform that does not report one
/// leaves it at `1.0`.
///
/// It is the number to *report*, not the number to multiply by: what a size in
/// `Sp` comes to is [`scale_sp`], and on Android 14 and up the two are not the
/// same arithmetic. See [`crate::font_scale`].
pub fn current_font_scale() -> f32 {
    current_font_scale_curve().scale()
}

/// Returns the conversion the platform performs for a size in `Sp`.
///
/// The setting is not a multiplier on every platform — see
/// [`crate::font_scale`] — so this, and not [`current_font_scale`], is what a
/// size in `Sp` is resolved through. The scalar remains the thing to *report*.
pub fn current_font_scale_curve() -> crate::font_scale::FontScaleCurve {
    with_render_state(|state| *lock_font_scale(&state.font_scale))
}

/// A size in scale-independent pixels, in dp, through the running app's curve.
pub fn scale_sp(sp: f32) -> f32 {
    current_font_scale_curve().sp_to_dp(sp)
}

/// Updates the system font scale, taking it as a plain multiplier.
///
/// Hosts call this when the platform reports the setting, and again whenever it
/// changes while the app is running — on Android that is a configuration
/// change, which arrives without the process restarting. Like density it
/// invalidates layout, because every `Sp` size on screen has just changed.
///
/// A host whose platform converts `Sp` through a table of its own calls
/// [`set_font_scale_curve`] instead.
pub fn set_font_scale(scale: f32) {
    set_font_scale_curve(crate::font_scale::FontScaleCurve::linear(scale));
}

/// Updates the system font scale and the conversion behind it.
///
/// Hosts that can read the platform's real `Sp` conversion call this instead of
/// [`set_font_scale`], which is the same thing with no table behind it. A curve
/// whose scale is not a value a platform could report is refused the same way a
/// bare scalar is, and refusing it drops the table with it: knots sampled at a
/// scale that was rejected describe a conversion the app is not going to use.
pub fn set_font_scale_curve(curve: crate::font_scale::FontScaleCurve) {
    let normalized = normalize_font_scale(curve.scale());
    let curve = if (normalized - curve.scale()).abs() <= f32::EPSILON {
        curve
    } else {
        crate::font_scale::FontScaleCurve::linear(normalized)
    };
    with_render_state(|state| {
        let mut current = lock_font_scale(&state.font_scale);
        if *current != curve {
            *current = curve;
            state.layout_invalidated.store(true, Ordering::Relaxed);
        }
    });
}

fn lock_font_scale(
    slot: &Mutex<crate::font_scale::FontScaleCurve>,
) -> MutexGuard<'_, crate::font_scale::FontScaleCurve> {
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Requests that the renderer rebuild the current scene.
pub fn request_render_invalidation() {
    with_render_state(|state| state.render_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a render invalidation was pending and clears the flag.
pub fn take_render_invalidation() -> bool {
    with_render_state(|state| state.render_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a render invalidation is pending without clearing it.
pub fn peek_render_invalidation() -> bool {
    with_render_state(|state| state.render_invalidated.load(Ordering::Relaxed))
}

/// Requests a new pointer-input pass without touching layout or draw dirties.
pub fn request_pointer_invalidation() {
    with_render_state(|state| state.pointer_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a pointer invalidation was pending and clears the flag.
pub fn take_pointer_invalidation() -> bool {
    with_render_state(|state| state.pointer_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a pointer invalidation is pending without clearing it.
pub fn peek_pointer_invalidation() -> bool {
    with_render_state(|state| state.pointer_invalidated.load(Ordering::Relaxed))
}

/// Requests a focus recomposition without affecting layout/draw dirties.
pub fn request_focus_invalidation() {
    with_render_state(|state| state.focus_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a focus invalidation was pending and clears the flag.
pub fn take_focus_invalidation() -> bool {
    with_render_state(|state| state.focus_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a focus invalidation is pending without clearing it.
pub fn peek_focus_invalidation() -> bool {
    with_render_state(|state| state.focus_invalidated.load(Ordering::Relaxed))
}

/// Requests a **global** layout re-run.
///
/// # ⚠️ WARNING: Extremely Expensive - O(entire app size)
///
/// This triggers internal cache invalidation that forces **every node** in the app
/// to re-measure, even if nothing changed. This is a performance footgun!
///
/// ## Valid Use Cases (rare!)
///
/// Only use this for **true global changes** that affect layout computation everywhere:
/// - Window/viewport resize
/// - Global font scale or density changes
/// - System-wide theme changes that affect layout
/// - Debug toggles that change layout behavior globally
///
/// ## For Local Changes - DO NOT USE THIS
///
/// **If you're invalidating layout for scroll, a single widget update, or any local change,
/// you MUST use the scoped repass mechanism instead:**
///
/// ```text
/// cranpose_ui::schedule_layout_repass(node_id);
/// ```
///
/// Scoped repasses give you O(subtree) performance instead of O(app), and they don't
/// invalidate caches across the entire app.
pub fn request_layout_invalidation() {
    with_render_state(|state| state.layout_invalidated.store(true, Ordering::Relaxed));
}

/// Returns true if a layout invalidation was pending and clears the flag.
pub fn take_layout_invalidation() -> bool {
    with_render_state(|state| state.layout_invalidated.swap(false, Ordering::Relaxed))
}

/// Returns true if a layout invalidation is pending without clearing it.
pub fn peek_layout_invalidation() -> bool {
    with_render_state(|state| state.layout_invalidated.load(Ordering::Relaxed))
}

#[cfg(any(test, feature = "test-helpers"))]
#[doc(hidden)]
pub fn reset_render_state_for_tests() {
    let _ = take_draw_repass_nodes();
    let _ = take_layout_repass_nodes();
    let _ = take_modifier_slice_repass_nodes();
    let _ = take_render_invalidation();
    let _ = take_pointer_invalidation();
    let _ = take_focus_invalidation();
    let _ = take_layout_invalidation();
    debug_reset_last_fling_velocity();
    set_density(1.0);
    set_font_scale(1.0);
    let _ = take_layout_invalidation();
}

#[cfg(test)]
pub(crate) struct TestAppContextScope {
    _scope: AppContextScope,
    _context: Rc<AppContext>,
}

#[cfg(test)]
pub(crate) fn app_context_test_scope() -> TestAppContextScope {
    let context = AppContext::new();
    let scope = context.enter_scope();
    context.enter(reset_render_state_for_tests);
    TestAppContextScope {
        _scope: scope,
        _context: context,
    }
}

#[cfg(test)]
pub(crate) struct RenderStateTestGuard {
    _app_scope: TestAppContextScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn render_state_test_guard() -> RenderStateTestGuard {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = match TEST_LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    RenderStateTestGuard {
        _app_scope: app_context_test_scope(),
        _lock: lock,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};

    use super::*;
    use crate::text::{AnnotatedString, TextLayoutResult, TextMeasurer, TextMetrics, TextStyle};

    #[test]
    fn a_draw_closure_can_name_its_own_node_for_the_next_frame() {
        let _scope = app_context_test_scope();
        let _ = take_draw_repass_nodes();
        let _ = take_render_invalidation();

        observe_draw_reads(DrawObservationScope::new(33, 0), || {
            request_current_draw_redraw();
        });
        assert_eq!(
            take_draw_repass_nodes(),
            vec![33],
            "the recording node must be scheduled for re-record"
        );
        assert!(
            take_render_invalidation(),
            "the next frame must be requested"
        );

        request_current_draw_redraw();
        assert!(take_draw_repass_nodes().is_empty());
        assert!(take_render_invalidation());
    }

    struct TestTextMeasurer;

    impl TextMeasurer for TestTextMeasurer {
        fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
            TextMetrics {
                width: text.text.len() as f32,
                height: 1.0,
                line_height: 1.0,
                line_count: 1,
            }
        }

        fn get_offset_for_position(
            &self,
            text: &AnnotatedString,
            _style: &TextStyle,
            x: f32,
            _y: f32,
        ) -> usize {
            x.round().max(0.0) as usize % text.text.len().max(1)
        }

        fn get_cursor_x_for_offset(
            &self,
            _text: &AnnotatedString,
            _style: &TextStyle,
            offset: usize,
        ) -> f32 {
            offset as f32
        }

        fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
            TextLayoutResult::monospaced(&text.text, 1.0, 1.0)
        }
    }

    #[test]
    fn app_context_ids_do_not_use_process_global_counter() {
        let source = include_str!("render_state.rs");
        assert!(!source.contains(concat!("NEXT_", "APP_CONTEXT_ID: Atomic")));
    }

    #[test]
    fn app_context_ids_are_unique_within_thread_registry() {
        let first = AppContext::new();
        let second = AppContext::new();

        assert_ne!(first.id, second.id);
        assert!(app_context_by_id(first.id).is_some());
        assert!(app_context_by_id(second.id).is_some());
    }

    #[test]
    fn set_text_measurer_requires_active_app_context() {
        let result = std::panic::catch_unwind(|| {
            crate::text::set_text_measurer(TestTextMeasurer);
        });
        assert!(result.is_err());

        let context = AppContext::new();
        context.enter(|| {
            crate::text::set_text_measurer(TestTextMeasurer);
        });
    }

    #[test]
    fn the_font_scale_starts_at_one_and_invalidates_layout_when_it_moves() {
        let context = AppContext::new();
        context.enter(|| {
            assert_eq!(current_font_scale(), 1.0);
            let _ = take_layout_invalidation();

            set_font_scale(1.3);
            assert_eq!(current_font_scale(), 1.3);
            assert!(
                take_layout_invalidation(),
                "every Sp on screen just changed size"
            );

            set_font_scale(1.3);
            assert!(!take_layout_invalidation());
        });
    }

    #[test]
    fn a_font_scale_no_platform_reports_is_refused() {
        let context = AppContext::new();
        context.enter(|| {
            for nonsense in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                set_font_scale(1.0);
                set_font_scale(nonsense);
                assert_eq!(current_font_scale(), 1.0, "{nonsense} was let through");
            }
            set_font_scale(99.0);
            assert_eq!(current_font_scale(), MAX_FONT_SCALE);
            set_font_scale(0.01);
            assert_eq!(current_font_scale(), MIN_FONT_SCALE);
        });
    }

    #[test]
    fn the_font_scale_is_per_app_context() {
        let first = AppContext::new();
        let second = AppContext::new();
        first.enter(|| set_font_scale(1.5));
        first.enter(|| assert_eq!(current_font_scale(), 1.5));
        second.enter(|| assert_eq!(current_font_scale(), 1.0));
    }

    #[test]
    fn invalidation_flags_are_shared_across_threads() {
        let state = Arc::new(RenderState::new_with_density(1.0));
        let (tx, rx) = mpsc::channel();
        let worker_state = Arc::clone(&state);

        let handle = std::thread::spawn(move || {
            worker_state
                .render_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .pointer_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .focus_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .layout_invalidated
                .store(true, Ordering::Relaxed);
            worker_state
                .density_bits
                .store(f32::to_bits(2.0), Ordering::Relaxed);
            tx.send(()).expect("signal invalidation setup");

            f32::from_bits(worker_state.density_bits.load(Ordering::Relaxed))
        });

        rx.recv().expect("wait for worker invalidation setup");
        assert!(state.render_invalidated.load(Ordering::Relaxed));
        assert!(state.pointer_invalidated.load(Ordering::Relaxed));
        assert!(state.focus_invalidated.load(Ordering::Relaxed));
        assert!(state.layout_invalidated.load(Ordering::Relaxed));
        assert_eq!(
            f32::from_bits(state.density_bits.load(Ordering::Relaxed)),
            2.0
        );
        assert!(state.render_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.pointer_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.focus_invalidated.swap(false, Ordering::Relaxed));
        assert!(state.layout_invalidated.swap(false, Ordering::Relaxed));

        let density = handle.join().expect("worker invalidation snapshot");
        assert_eq!(density, 2.0);
        assert!(!state.render_invalidated.load(Ordering::Relaxed));
        assert!(!state.pointer_invalidated.load(Ordering::Relaxed));
        assert!(!state.focus_invalidated.load(Ordering::Relaxed));
        assert!(!state.layout_invalidated.load(Ordering::Relaxed));
    }

    #[test]
    fn app_contexts_keep_density_and_invalidations_isolated() {
        let first = AppContext::new_with_density(1.0);
        let second = AppContext::new_with_density(1.0);

        first.enter(|| {
            set_density(2.0);
            request_render_invalidation();
            request_pointer_invalidation();
            schedule_layout_repass(11);
            schedule_draw_repass(12);
        });

        second.enter(|| {
            assert_eq!(current_density(), 1.0);
            assert!(!peek_render_invalidation());
            assert!(!peek_pointer_invalidation());
            assert!(!peek_layout_invalidation());
            assert!(!has_pending_layout_repasses());
            assert!(!has_pending_draw_repasses());
        });

        first.enter(|| {
            assert_eq!(current_density(), 2.0);
            assert!(peek_render_invalidation());
            assert!(peek_pointer_invalidation());
            assert!(peek_layout_invalidation());
            assert!(has_pending_layout_repasses());
            assert!(has_pending_draw_repasses());
            assert_eq!(take_layout_repass_nodes(), vec![11]);
            assert_eq!(take_draw_repass_nodes(), vec![12]);
            assert!(take_render_invalidation());
            assert!(take_pointer_invalidation());
            assert!(take_layout_invalidation());
        });
    }

    #[test]
    fn app_contexts_keep_fling_velocity_diagnostics_isolated() {
        let first = AppContext::new_with_density(1.0);
        let second = AppContext::new_with_density(1.0);

        first.enter(|| {
            record_last_fling_velocity(1200.0);
            assert_eq!(debug_last_fling_velocity(), 1200.0);
        });

        second.enter(|| {
            assert_eq!(debug_last_fling_velocity(), 0.0);
            record_last_fling_velocity(-450.0);
            assert_eq!(debug_last_fling_velocity(), -450.0);
        });

        first.enter(|| {
            assert_eq!(debug_last_fling_velocity(), 1200.0);
            debug_reset_last_fling_velocity();
            assert_eq!(debug_last_fling_velocity(), 0.0);
        });

        second.enter(|| {
            assert_eq!(debug_last_fling_velocity(), -450.0);
        });
    }

    #[test]
    fn app_context_new_uses_independent_density() {
        let outer = AppContext::new_with_density(2.0);
        let context = AppContext::new();
        context.enter(|| {
            assert_eq!(current_density(), 1.0);
        });
        outer.enter(|| {
            assert_eq!(current_density(), 2.0);
        });
    }

    #[test]
    fn runtime_state_access_requires_explicit_app_context_even_in_tests() {
        let result = std::panic::catch_unwind(|| {
            request_render_invalidation();
        });
        assert!(result.is_err());
    }

    #[test]
    fn app_contexts_keep_layout_frame_arenas_isolated() {
        let first = AppContext::new_with_density(1.0);
        let second = AppContext::new_with_density(1.0);

        first.enter(|| {
            assert_eq!(layout_frame_arena_placement_scratch_count(), 0);
            let mut arena = take_layout_frame_arena();
            arena.seed_placement_scratch_for_test();
            replace_layout_frame_arena(arena);
            assert_eq!(layout_frame_arena_placement_scratch_count(), 1);
        });

        second.enter(|| {
            assert_eq!(layout_frame_arena_placement_scratch_count(), 0);
        });

        first.enter(|| {
            assert_eq!(layout_frame_arena_placement_scratch_count(), 1);
        });
    }

    #[test]
    fn current_app_context_scope_does_not_extend_context_lifetime() {
        let weak = {
            let context = AppContext::new_with_density(1.0);
            let weak = Rc::downgrade(&context);
            context.enter(|| {
                assert!(current_app_context().is_some());
            });
            weak
        };

        assert!(weak.upgrade().is_none());
        assert!(current_app_context().is_none());
    }

    #[test]
    fn dropped_app_context_unregisters_from_thread_lookup_registry() {
        let start_count = app_context_registry_entry_count();

        let id = {
            let context = AppContext::new_with_density(1.0);
            let id = context.id;
            assert!(app_context_by_id(id).is_some());
            id
        };

        assert_eq!(
            app_context_registry_entry_count(),
            start_count,
            "dropped AppContexts must remove their weak registry entry"
        );
        assert!(app_context_by_id(id).is_none());
    }
}
