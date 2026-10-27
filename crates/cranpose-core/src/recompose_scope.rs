use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

use smallvec::SmallVec;

#[cfg(feature = "inspection")]
use crate::source_trace;
use crate::{
    AnchorId, Composer, LocalStackSnapshot, NodeId, RetentionMode, RuntimeHandle, ScopeId,
    SlotsHost, StateId, collections::map::HashSet, composer, runtime, thread_state,
};

// Whether a scope's body ran to its end or was skipped.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyRun {
    Ran,
    Skipped,
}

impl From<bool> for BodyRun {
    fn from(ran: bool) -> Self {
        if ran { Self::Ran } else { Self::Skipped }
    }
}

/// A composable's body. The scope passes itself when it runs, so the
/// callback needs neither a second box nor a reference back to its scope; a
/// capture-free body records how to name its type, to be kept as is.
struct RecomposeCallback {
    body: Box<dyn FnMut(&Composer) + 'static>,
    stateless_body: Option<fn() -> TypeId>,
}

fn scope_activity_epoch() -> u64 {
    thread_state::cells(|cells| cells.scope_activity_epoch.get())
}

fn note_scope_activity_change() {
    thread_state::cells(|cells| {
        let epoch = &cells.scope_activity_epoch;
        epoch.set(epoch.get() + 1);
    });
}

/// Runs `read` on what `cell` holds and puts it back: a scope's links are
/// read this way rather than through a `RefCell`, whose borrow flag would
/// take a word in every scope of a composition. `read` must not reach the
/// same cell again.
fn read_cell<T: Default, R>(cell: &Cell<T>, read: impl FnOnce(&T) -> R) -> R {
    let value = cell.take();
    let result = read(&value);
    cell.set(value);
    result
}

/// Like [`read_cell`], for a change to what `cell` holds.
fn update_cell<T: Default, R>(cell: &Cell<T>, update: impl FnOnce(&mut T) -> R) -> R {
    let mut value = cell.take();
    let result = update(&mut value);
    cell.set(value);
    result
}

/// How a scope is tied to the scope its lifetime follows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    /// The scope it composes under, its recomposition ancestor.
    Parent,
    /// For the root of a slot table composed for another scope's content,
    /// that scope, which is no recomposition ancestor.
    Lifetime,
}

/// The scope a scope's lifetime follows, and how it is tied to it.
type OwnerLink = Cell<Option<(OwnerKind, Weak<RecomposeScopeInner>)>>;

/// The node id a scope's `parent_hint` holds for none: ids index the
/// applier's nodes, which never reach it.
const NO_PARENT_HINT: NodeId = NodeId(usize::MAX);

pub(crate) struct RecomposeScopeInner {
    runtime: runtime::ScopeRuntime,
    invalid: Cell<bool>,
    enqueued: Cell<bool>,
    active: Cell<bool>,
    active_epoch: Cell<u64>,
    deactivations: Cell<u64>,
    composed_once: Cell<bool>,
    pending_recompose: Cell<bool>,
    /// The recompose callback is running: it is out of `recompose` meanwhile,
    /// and the scope still recomposes what reads in it.
    recomposing: Cell<bool>,
    force_reuse: Cell<bool>,
    force_recompose: Cell<bool>,
    derivation: Cell<bool>,
    retention_mode: Cell<RetentionMode>,
    /// [`NO_PARENT_HINT`] for none.
    parent_hint: Cell<NodeId>,
    group_anchor: Cell<AnchorId>,
    recompose: Cell<Option<RecomposeCallback>>,
    owner: OwnerLink,
    local_stack: Cell<LocalStackSnapshot>,
    #[cfg(feature = "inspection")]
    source_trace: RefCell<Rc<[source_trace::SourceLocation]>>,
    #[cfg(all(feature = "inspection", debug_assertions))]
    pub(crate) recompositions: std::cell::OnceCell<source_trace::RecompositionCounter>,
    slots_storage_key: Cell<usize>,
    slots_runtime_state: Cell<Option<Weak<composer::ComposerRuntimeState>>>,
    /// The states the scope is subscribed to: what its last body read, in
    /// read order.
    state_subscriptions: RefCell<StateIds>,
    /// What the body running now read so far, once it left the order of
    /// `state_subscriptions`.
    run_reads: RefCell<StateIds>,
    /// How many subscriptions the body running now read in order, or
    /// [`DIVERGED`] or [`NOT_READING`].
    read_cursor: Cell<u32>,
    invalidation_sources: Cell<StateIds>,
    unknown_invalidation_source: Cell<bool>,
}

type StateIds = SmallVec<[StateId; 2]>;

/// What [`RecomposeScope::note_read`] made of a read.
pub(crate) enum ScopeRead {
    /// The read repeats the last body's, so the state watches the scope.
    Watched,
    /// The read went into the running body's reads; the state may not watch
    /// the scope yet.
    Collected,
    /// No body of the scope is running.
    NotReading,
}

/// A scope's `read_cursor` while no body of it runs.
const NOT_READING: u32 = u32::MAX;
/// A scope's `read_cursor` once the running body left the order of the reads
/// before it.
const DIVERGED: u32 = u32::MAX - 1;

enum ScopeOwner {
    Root,
    Live(Rc<RecomposeScopeInner>),
    Dropped,
}

impl RecomposeScopeInner {
    fn owner(&self) -> ScopeOwner {
        read_cell(&self.owner, |owner| {
            owner.as_ref().map_or(ScopeOwner::Root, |(_, owner)| {
                owner
                    .upgrade()
                    .map_or(ScopeOwner::Dropped, ScopeOwner::Live)
            })
        })
    }

    fn is_effectively_active(&self, epoch: u64) -> bool {
        if self.active_epoch.get() == epoch {
            return true;
        }
        if !self.active.get() {
            return false;
        }
        let active = match self.owner() {
            ScopeOwner::Root => true,
            ScopeOwner::Dropped => false,
            ScopeOwner::Live(owner) => owner.is_effectively_active(epoch),
        };
        if active {
            self.active_epoch.set(epoch);
        }
        active
    }

    fn new(runtime: runtime::ScopeRuntime) -> Self {
        runtime.increment_live_recompose_scope_count();
        Self {
            runtime,
            invalid: Cell::new(false),
            enqueued: Cell::new(false),
            active: Cell::new(true),
            active_epoch: Cell::new(0),
            deactivations: Cell::new(0),
            composed_once: Cell::new(false),
            pending_recompose: Cell::new(false),
            recomposing: Cell::new(false),
            force_reuse: Cell::new(false),
            force_recompose: Cell::new(false),
            derivation: Cell::new(false),
            retention_mode: Cell::new(RetentionMode::DisposeWhenInactive),
            parent_hint: Cell::new(NO_PARENT_HINT),
            group_anchor: Cell::new(AnchorId::INVALID),
            recompose: Cell::new(None),
            owner: Cell::new(None),
            local_stack: Cell::new(None),
            #[cfg(feature = "inspection")]
            source_trace: RefCell::new(Rc::from([])),
            #[cfg(all(feature = "inspection", debug_assertions))]
            recompositions: std::cell::OnceCell::new(),
            slots_storage_key: Cell::new(0),
            slots_runtime_state: Cell::new(None),
            state_subscriptions: RefCell::new(StateIds::new()),
            run_reads: RefCell::new(StateIds::new()),
            read_cursor: Cell::new(NOT_READING),
            invalidation_sources: Cell::new(StateIds::new()),
            unknown_invalidation_source: Cell::new(false),
        }
    }

    fn id(&self) -> ScopeId {
        std::ptr::from_ref(self).addr()
    }
}

fn push_unique_state_id(ids: &mut StateIds, state_id: StateId) {
    if !ids.contains(&state_id) {
        ids.push(state_id);
    }
}

impl Drop for RecomposeScopeInner {
    fn drop(&mut self) {
        note_scope_activity_change();
        let id = self.id();
        self.runtime.decrement_live_recompose_scope_count();
        let subscriptions = std::mem::take(self.state_subscriptions.get_mut());
        let run_reads = std::mem::take(self.run_reads.get_mut());
        for state_id in subscriptions.into_iter().chain(run_reads) {
            self.runtime.unregister_state_scope(state_id, id);
        }
        #[cfg(debug_assertions)]
        {
            thread_state::try_runtimes(|state| {
                state.scope_labels.borrow_mut().remove(&id);
                state.scope_invalidation_sources.borrow_mut().remove(&id);
            });
        }
        if self.enqueued.replace(false) {
            self.runtime.mark_scope_recomposed();
        }
    }
}

/// Size of the registry of live recompose scopes, for leak checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecomposeScopeRegistryDebugStats {
    /// Scopes registered.
    pub len: usize,
    /// Capacity reserved for the registry.
    pub capacity: usize,
}

/// The unit of recomposition. State read inside a scope invalidates it, and a recomposition runs
/// only the content of the invalid scopes. Clones refer to the same scope.
#[derive(Clone)]
pub struct RecomposeScope {
    pub(crate) inner: Rc<RecomposeScopeInner>,
}

impl PartialEq for RecomposeScope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RecomposeScope {}

impl Hash for RecomposeScope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl RecomposeScope {
    /// Marks this scope as one that only recomputes a derived state: running
    /// it changes nothing a composition shows unless the value it writes
    /// invalidates the scopes that read it.
    pub(crate) fn mark_derivation(&self) {
        self.inner.derivation.set(true);
    }

    pub(crate) fn is_derivation(&self) -> bool {
        self.inner.derivation.get()
    }

    pub(crate) fn new(runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RecomposeScopeInner::new(runtime.scope_runtime())),
        }
    }

    pub(crate) fn downgrade(&self) -> Weak<RecomposeScopeInner> {
        Rc::downgrade(&self.inner)
    }

    pub(crate) fn upgrade(weak: &Weak<RecomposeScopeInner>) -> Option<Self> {
        weak.upgrade().map(|inner| Self { inner })
    }

    /// Returns an id that is unique among the live scopes.
    pub fn id(&self) -> ScopeId {
        self.inner.id()
    }

    /// Returns `true` if state that the scope read has changed since it last recomposed.
    pub fn is_invalid(&self) -> bool {
        self.inner.invalid.get()
    }

    pub(crate) fn is_enqueued(&self) -> bool {
        self.inner.enqueued.get()
    }

    /// Returns `false` between `deactivate` and `reactivate`.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
    }

    /// Total deactivations along this scope's owner chain. A retained slot
    /// composition records this at compose time; a later mismatch means some
    /// enclosing composition was deactivated (its owned effects cancelled)
    /// since the slot last composed, so the retained content must recompose
    /// once to restart them — no flag on the slot's own scopes carries that
    /// trace, because deactivation walks stop at slot-host boundaries.
    pub fn owner_chain_deactivation_epoch(&self) -> u64 {
        let mut total = self.inner.deactivations.get();
        let mut owner = self.inner.owner();
        while let ScopeOwner::Live(scope) = owner {
            total = total.wrapping_add(scope.deactivations.get());
            owner = scope.owner();
        }
        total
    }

    pub(crate) fn is_effectively_active(&self) -> bool {
        self.inner.is_effectively_active(scope_activity_epoch())
    }

    pub(crate) fn record_state_subscription(&self, state_id: StateId) {
        push_unique_state_id(&mut self.inner.state_subscriptions.borrow_mut(), state_id);
    }

    /// Starts collecting the states a body of the scope reads.
    pub(crate) fn begin_reads(&self) {
        self.inner.read_cursor.set(0);
    }

    /// Notes that the body running now read `state_id`. While the body reads
    /// what the last one read, in the same order, a read only moves the
    /// cursor along the subscriptions, and the state is known to watch the
    /// scope already; the first read off that order copies the reads so far
    /// into `run_reads`.
    #[inline]
    pub(crate) fn note_read(&self, state_id: StateId) -> ScopeRead {
        let cursor = self.inner.read_cursor.get();
        if cursor == NOT_READING {
            return ScopeRead::NotReading;
        }
        if cursor == DIVERGED {
            let mut reads = self.inner.run_reads.borrow_mut();
            if reads.last() != Some(&state_id) {
                reads.push(state_id);
            }
            return ScopeRead::Collected;
        }
        let at = cursor as usize;
        let subscribed = self.inner.state_subscriptions.borrow();
        if subscribed.get(at) == Some(&state_id) {
            self.inner.read_cursor.set(cursor + 1);
            return ScopeRead::Watched;
        }
        if at > 0 && subscribed[at - 1] == state_id {
            return ScopeRead::Watched;
        }
        let mut reads = self.inner.run_reads.borrow_mut();
        reads.clear();
        reads.extend_from_slice(&subscribed[..at]);
        reads.push(state_id);
        self.inner.read_cursor.set(DIVERGED);
        ScopeRead::Collected
    }

    // Keeps the subscriptions the scope has: its body runs this pass without
    // the reads that made them, as a skipped body does.
    pub(crate) fn keep_reads(&self) {
        self.inner.read_cursor.set(NOT_READING);
    }

    /// Ends collecting reads. A body that ran makes its reads the scope's
    /// subscriptions and drops the states it did not read again, as Compose
    /// does; a skipped or unfinished one keeps the subscriptions it had.
    #[inline]
    pub(crate) fn end_reads(&self, run: BodyRun) {
        let ran = run == BodyRun::Ran;
        let cursor = self.inner.read_cursor.replace(NOT_READING);
        // A skipped body that read in order keeps the subscriptions as they
        // are.
        if cursor == NOT_READING || (!ran && cursor != DIVERGED) {
            return;
        }
        self.settle_reads(cursor, ran);
    }

    /// The part of [`Self::end_reads`] for a body that ran or left the
    /// order of its reads: `cursor` is where the read cursor stood.
    #[inline(never)]
    fn settle_reads(&self, cursor: u32, ran: bool) {
        let id = self.id();
        let mut subscribed = self.inner.state_subscriptions.borrow_mut();
        if cursor != DIVERGED {
            // The body read a prefix of the subscriptions, in order.
            let read = cursor as usize;
            if ran && read < subscribed.len() {
                for &state_id in &subscribed[read..] {
                    if !subscribed[..read].contains(&state_id) {
                        self.inner.runtime.forget_state_scope(state_id, id);
                    }
                }
                subscribed.truncate(read);
            }
            return;
        }
        let mut reads = self.inner.run_reads.borrow_mut();
        if ran {
            for &state_id in subscribed.iter() {
                if !reads.contains(&state_id) {
                    self.inner.runtime.forget_state_scope(state_id, id);
                }
            }
            std::mem::swap(&mut *reads, &mut *subscribed);
            reads.clear();
        } else {
            for state_id in reads.drain(..) {
                push_unique_state_id(&mut subscribed, state_id);
            }
        }
    }

    fn record_unknown_invalidation_source(&self) {
        self.inner.unknown_invalidation_source.set(true);
        update_cell(&self.inner.invalidation_sources, StateIds::clear);
    }

    fn record_state_invalidation_source(&self, state_id: StateId) {
        if !self.inner.unknown_invalidation_source.get() {
            update_cell(&self.inner.invalidation_sources, |ids| {
                push_unique_state_id(ids, state_id);
            });
        }
    }

    fn enqueue_invalidation(&self) {
        self.inner.invalid.set(true);
        if !self.is_effectively_active() {
            return;
        }
        if !self.inner.enqueued.replace(true) {
            self.inner.runtime.register_invalid_scope(self.downgrade());
        }
    }

    pub(crate) fn invalidate(&self) {
        self.record_unknown_invalidation_source();
        self.enqueue_invalidation();
    }

    pub(crate) fn invalidate_from_state(&self, state_id: StateId) {
        self.record_state_invalidation_source(state_id);
        self.enqueue_invalidation();
    }

    pub(crate) fn mark_recomposed(&self) {
        // An invalidation notes its source and sets `invalid` together, so a
        // scope nothing invalidated has no source to forget.
        if self.inner.invalid.replace(false) {
            self.inner.unknown_invalidation_source.set(false);
            update_cell(&self.inner.invalidation_sources, StateIds::clear);
        }
        self.inner.force_reuse.set(false);
        self.inner.force_recompose.set(false);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
        }
        let pending = self.inner.pending_recompose.replace(false);
        if pending {
            if self.inner.active.get() {
                self.invalidate();
            } else {
                self.inner.invalid.set(true);
            }
        }
    }

    /// Registers `callback` as the code that recomposes the scope when state
    /// its body read changes. A stateless body the scope reruns already
    /// stays as it is.
    pub(crate) fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        let body: fn() -> TypeId = TypeId::of::<F>;
        let stateless = size_of::<F>() == 0;
        if stateless && self.reruns_stateless(body) {
            return;
        }
        self.set_boxed_recompose(Box::new(callback), stateless.then_some(body));
    }

    #[inline(never)]
    pub(crate) fn set_boxed_recompose(
        &self,
        body: Box<dyn FnMut(&Composer) + 'static>,
        stateless_body: Option<fn() -> TypeId>,
    ) {
        #[cfg(feature = "inspection")]
        self.inner
            .source_trace
            .replace(source_trace::current_source_trace());
        self.inner.recompose.set(Some(RecomposeCallback {
            body,
            stateless_body,
        }));
    }

    /// Whether the scope reruns the stateless body whose type `body` names.
    /// The same function names the same type; a copy of it in another
    /// codegen unit is told by the type itself.
    fn reruns_stateless(&self, body: fn() -> TypeId) -> bool {
        read_cell(&self.inner.recompose, |callback| {
            callback
                .as_ref()
                .and_then(|callback| callback.stateless_body)
                .is_some_and(|current| std::ptr::fn_addr_eq(current, body) || current() == body())
        })
    }

    pub(crate) fn run_recompose(&self, composer: &Composer) -> bool {
        #[cfg(feature = "inspection")]
        let _source_context = source_trace::restore_source_trace(&self.inner.source_trace.borrow());
        let Some(mut callback) = self.inner.recompose.take() else {
            return false;
        };
        self.begin_reads();
        self.inner.recomposing.set(true);
        (callback.body)(composer);
        self.inner.recomposing.set(false);
        self.end_reads(BodyRun::Ran);
        // The body may have stored a new callback for the scope while it ran.
        update_cell(&self.inner.recompose, |slot| {
            if slot.is_none() {
                *slot = Some(callback);
            }
        });
        true
    }

    pub(crate) fn has_recompose_callback(&self) -> bool {
        self.inner.recomposing.get() || read_cell(&self.inner.recompose, Option::is_some)
    }

    /// How many scopes this one composes under.
    pub(crate) fn depth(&self) -> usize {
        let mut depth = 0;
        let mut current = self.parent_scope();
        while let Some(scope) = current {
            depth += 1;
            current = scope.parent_scope();
        }
        depth
    }

    pub(crate) fn snapshot_locals(&self, stack: &LocalStackSnapshot) {
        update_cell(&self.inner.local_stack, |locals| {
            let unchanged = match (&*locals, stack) {
                (Some(current), Some(stack)) => Rc::ptr_eq(current, stack),
                (current, stack) => current.is_none() && stack.is_none(),
            };
            if !unchanged {
                locals.clone_from(stack);
            }
        });
    }

    pub(crate) fn local_stack(&self) -> LocalStackSnapshot {
        read_cell(&self.inner.local_stack, Clone::clone)
    }

    pub(crate) fn set_parent_hint(&self, parent: Option<NodeId>) {
        self.inner.parent_hint.set(parent.unwrap_or(NO_PARENT_HINT));
    }

    /// Ties the scope to the scope its lifetime follows, or to none.
    fn set_owner(&self, kind: OwnerKind, owner: Option<&Self>) {
        update_cell(&self.inner.owner, |link| {
            let current = link
                .as_ref()
                .map(|(kind, owner)| (*kind, Weak::as_ptr(owner)));
            if current != owner.map(|owner| (kind, Rc::as_ptr(&owner.inner))) {
                *link = owner.map(|owner| (kind, owner.downgrade()));
                note_scope_activity_change();
            }
        });
    }

    /// Makes `parent` the scope this one composes under, or makes it a root.
    pub(crate) fn set_parent_scope(&self, parent: Option<&Self>) {
        self.set_owner(OwnerKind::Parent, parent);
    }

    fn owner_of_kind(&self, kind: OwnerKind) -> Option<Self> {
        read_cell(&self.inner.owner, |owner| {
            owner
                .as_ref()
                .filter(|(owner_kind, _)| *owner_kind == kind)
                .and_then(|(_, owner)| Self::upgrade(owner))
        })
    }

    pub(crate) fn parent_scope(&self) -> Option<Self> {
        self.owner_of_kind(OwnerKind::Parent)
    }

    /// Makes this scope a root whose lifetime follows `owner`, or a plain
    /// root.
    pub(crate) fn set_lifetime_owner_scope(&self, owner: Option<&Self>) {
        self.set_owner(OwnerKind::Lifetime, owner);
    }

    #[cfg(test)]
    pub(crate) fn lifetime_owner_scope(&self) -> Option<Self> {
        self.owner_of_kind(OwnerKind::Lifetime)
    }

    pub(crate) fn callback_promotion_target(&self) -> Option<Self> {
        let mut current = self.parent_scope();
        while let Some(scope) = current {
            if scope.has_recompose_callback() {
                return Some(scope);
            }
            current = scope.parent_scope();
        }
        None
    }

    pub(crate) fn parent_hint(&self) -> Option<NodeId> {
        Some(self.inner.parent_hint.get()).filter(|&hint| hint != NO_PARENT_HINT)
    }

    pub(crate) fn group_anchor(&self) -> AnchorId {
        self.inner.group_anchor.get()
    }

    pub(crate) fn set_group_anchor(&self, anchor: AnchorId) {
        self.inner.group_anchor.set(anchor);
    }

    pub(crate) fn set_slots_host(&self, host: &SlotsHost) {
        let storage_key = host.storage_key();
        update_cell(&self.inner.slots_runtime_state, |runtime_state| {
            if self.inner.slots_storage_key.get() == storage_key
                && runtime_state.as_ref().map(Weak::as_ptr) == host.runtime_state_ptr()
            {
                return;
            }
            self.inner.slots_storage_key.set(storage_key);
            *runtime_state = host.runtime_state().map(|state| Rc::downgrade(&state));
        });
    }

    pub(crate) fn slots_storage_key(&self) -> Option<usize> {
        let key = self.inner.slots_storage_key.get();
        (key != 0).then_some(key)
    }

    /// What decides the slot host a recomposition of this scope runs in: its
    /// table's storage key and the runtime state that table belongs to.
    pub(crate) fn slots_host_identity(&self) -> (usize, *const composer::ComposerRuntimeState) {
        let state = read_cell(&self.inner.slots_runtime_state, |state| {
            state.as_ref().map_or(std::ptr::null(), Weak::as_ptr)
        });
        (self.inner.slots_storage_key.get(), state)
    }

    pub(crate) fn slots_runtime_state(&self) -> Option<Rc<composer::ComposerRuntimeState>> {
        read_cell(&self.inner.slots_runtime_state, |state| {
            state.as_ref().and_then(Weak::upgrade)
        })
    }

    /// Marks the scope inactive, as when its content is retained or held for reuse. An inactive
    /// scope is not queued for recomposition until it is reactivated.
    pub fn deactivate(&self) {
        if !self.inner.active.replace(false) {
            return;
        }
        note_scope_activity_change();
        self.inner
            .deactivations
            .set(self.inner.deactivations.get() + 1);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    pub(crate) fn defer_until_reactivated(&self) {
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    /// Marks the scope active again and queues it for recomposition if it was invalidated while
    /// inactive.
    pub fn reactivate(&self) {
        self.inner.active.set(true);
        if self.inner.invalid.get()
            && self.is_effectively_active()
            && !self.inner.enqueued.replace(true)
        {
            self.inner.runtime.register_invalid_scope(self.downgrade());
        }
    }

    /// Makes the next `should_recompose` return `false` even if the scope is invalid. Cancels a
    /// pending `force_recompose`.
    pub fn force_reuse(&self) {
        self.inner.force_reuse.set(true);
        self.inner.force_recompose.set(false);
        self.inner.pending_recompose.set(true);
    }

    pub(crate) fn request_pending_recompose(&self) {
        self.inner.pending_recompose.set(true);
    }

    /// Makes the next `should_recompose` return `true` even if the scope is not invalid. Cancels a
    /// pending `force_reuse`.
    pub fn force_recompose(&self) {
        self.inner.force_recompose.set(true);
        self.inner.force_reuse.set(false);
        self.inner.pending_recompose.set(false);
    }

    pub(crate) fn set_retention_mode(&self, mode: RetentionMode) {
        self.inner.retention_mode.set(mode);
    }

    pub(crate) fn retention_mode(&self) -> RetentionMode {
        self.inner.retention_mode.get()
    }

    /// Returns whether the scope's content has to run now: `true` after `force_recompose` or when
    /// the scope is invalid, and `false` after `force_reuse`. A forced answer is used up by the
    /// call.
    pub fn should_recompose(&self) -> bool {
        if self.inner.force_recompose.replace(false) {
            self.inner.force_reuse.set(false);
            return true;
        }
        if self.inner.force_reuse.replace(false) {
            return false;
        }
        self.is_invalid()
    }

    /// Returns `true` once the scope has composed its content at least once.
    pub fn has_composed_once(&self) -> bool {
        self.inner.composed_once.get()
    }

    pub(crate) fn mark_composed_once(&self) {
        self.inner.composed_once.set(true);
    }

    // Whether a change of the state `state_id` alone invalidated the scope.
    pub(crate) fn invalidated_only_by_state(&self, state_id: StateId) -> bool {
        !self.inner.unknown_invalidation_source.get()
            && read_cell(&self.inner.invalidation_sources, |sources| {
                !sources.is_empty() && sources.iter().all(|source| *source == state_id)
            })
    }

    pub(crate) fn invalidated_only_by(&self, allowed_sources: &HashSet<StateId>) -> Option<bool> {
        if self.inner.unknown_invalidation_source.get() {
            return None;
        }
        read_cell(&self.inner.invalidation_sources, |sources| {
            (!sources.is_empty()).then(|| {
                sources
                    .iter()
                    .all(|source| allowed_sources.contains(source))
            })
        })
    }
}

#[cfg(test)]
impl RecomposeScope {
    pub(crate) fn new_for_test(runtime: RuntimeHandle) -> Self {
        Self::new(runtime)
    }
}

/// Options that a composable group passes to its recompose scope.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecomposeOptions {
    /// Skips the scope's body on the next composition even if it is invalid.
    pub force_reuse: bool,
    /// Runs the scope's body on the next composition even if it is not invalid.
    pub force_recompose: bool,
    /// What happens to the scope's content when it leaves the composition.
    pub retention: RetentionMode,
}
