use std::{
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

// A hand-written body that recomposes a scope.
type RecomposeClosure = Box<dyn FnMut(&Composer)>;

// A scope's yes-or-no states. Each takes a bit of `ScopeFlags`.
#[derive(Clone, Copy)]
enum ScopeFlag {
    Invalid,
    Enqueued,
    Active,
    ComposedOnce,
    PendingRecompose,
    // A hand-written recompose closure is running: it is out of
    // `recompose_closure` meanwhile, and the scope still recomposes what
    // reads in it.
    Recomposing,
    ForceReuse,
    ForceRecompose,
    Derivation,
    // Something other than a state invalidated the scope.
    UnknownInvalidationSource,
    // The scope's first body waits for a later frame, in pausable content.
    Paused,
    // The scope belongs to pausable content that has not completed: an
    // invalidation waits until it completes.
    Pending,
}

impl ScopeFlag {
    const fn bit(self) -> u16 {
        match self {
            Self::Invalid => 1,
            Self::Enqueued => 1 << 1,
            Self::Active => 1 << 2,
            Self::ComposedOnce => 1 << 3,
            Self::PendingRecompose => 1 << 4,
            Self::Recomposing => 1 << 5,
            Self::ForceReuse => 1 << 6,
            Self::ForceRecompose => 1 << 7,
            Self::Derivation => 1 << 8,
            Self::UnknownInvalidationSource => 1 << 9,
            Self::Paused => 1 << 10,
            Self::Pending => 1 << 11,
        }
    }
}

// A scope's flags in one cell, a bit each: a `Cell<bool>` each would take a
// byte of every scope.
struct ScopeFlags(Cell<u16>);

impl ScopeFlags {
    fn get(&self, flag: ScopeFlag) -> bool {
        self.0.get() & flag.bit() != 0
    }

    fn set(&self, flag: ScopeFlag, on: bool) {
        let bits = self.0.get();
        self.0.set(if on {
            bits | flag.bit()
        } else {
            bits & !flag.bit()
        });
    }

    // Whether `a` or `b` is set, in one test.
    fn either(&self, a: ScopeFlag, b: ScopeFlag) -> bool {
        self.0.get() & (a.bit() | b.bit()) != 0
    }

    // Sets `flag` and says whether it was set.
    fn replace(&self, flag: ScopeFlag, on: bool) -> bool {
        let was = self.get(flag);
        self.set(flag, on);
        was
    }
}

pub(crate) struct RecomposeScopeInner {
    runtime: runtime::ScopeRuntime,
    flags: ScopeFlags,
    active_epoch: Cell<u64>,
    deactivations: Cell<u64>,
    retention_mode: Cell<RetentionMode>,
    /// [`NO_PARENT_HINT`] for none.
    parent_hint: Cell<NodeId>,
    group_anchor: Cell<AnchorId>,
    // What runs the scope's body again: a `#[composable]` function's
    // generated recompose function, or a hand-written closure when the
    // function is `None`.
    recompose: Cell<Option<fn(&Composer)>>,
    recompose_closure: Cell<Option<RecomposeClosure>>,
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
    /// `state_subscriptions`: none while it reads in order.
    run_reads: Cell<Option<Box<StateIds>>>,
    /// How many subscriptions the body running now read in order, or
    /// [`DIVERGED`] or [`NOT_READING`].
    read_cursor: Cell<u32>,
    invalidation_sources: Cell<StateIds>,
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
        if !self.flags.get(ScopeFlag::Active) {
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
            flags: ScopeFlags(Cell::new(ScopeFlag::Active.bit())),
            active_epoch: Cell::new(0),
            deactivations: Cell::new(0),
            retention_mode: Cell::new(RetentionMode::DisposeWhenInactive),
            parent_hint: Cell::new(NO_PARENT_HINT),
            group_anchor: Cell::new(AnchorId::INVALID),
            recompose: Cell::new(None),
            recompose_closure: Cell::new(None),
            owner: Cell::new(None),
            local_stack: Cell::new(None),
            #[cfg(feature = "inspection")]
            source_trace: RefCell::new(Rc::from([])),
            #[cfg(all(feature = "inspection", debug_assertions))]
            recompositions: std::cell::OnceCell::new(),
            slots_storage_key: Cell::new(0),
            slots_runtime_state: Cell::new(None),
            state_subscriptions: RefCell::new(StateIds::new()),
            run_reads: Cell::new(None),
            read_cursor: Cell::new(NOT_READING),
            invalidation_sources: Cell::new(StateIds::new()),
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
        let run_reads = self.run_reads.take().map(|reads| *reads);
        for state_id in subscriptions
            .into_iter()
            .chain(run_reads.into_iter().flatten())
        {
            self.runtime.unregister_state_scope(state_id, id);
        }
        #[cfg(debug_assertions)]
        {
            thread_state::try_runtimes(|state| {
                state.scope_labels.borrow_mut().remove(&id);
                state.scope_invalidation_sources.borrow_mut().remove(&id);
            });
        }
        if self.flags.replace(ScopeFlag::Enqueued, false) {
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
        self.inner.flags.set(ScopeFlag::Derivation, true);
    }

    pub(crate) fn is_derivation(&self) -> bool {
        self.inner.flags.get(ScopeFlag::Derivation)
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
        self.inner.flags.get(ScopeFlag::Invalid)
    }

    pub(crate) fn is_enqueued(&self) -> bool {
        self.inner.flags.get(ScopeFlag::Enqueued)
    }

    /// Returns `false` between `deactivate` and `reactivate`.
    pub fn is_active(&self) -> bool {
        self.inner.flags.get(ScopeFlag::Active)
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
            update_cell(&self.inner.run_reads, |reads| {
                if let Some(reads) = reads
                    && reads.last() != Some(&state_id)
                {
                    reads.push(state_id);
                }
            });
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
        let mut reads = StateIds::from_slice(&subscribed[..at]);
        reads.push(state_id);
        self.inner.run_reads.set(Some(Box::new(reads)));
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
        let Some(reads) = self.inner.run_reads.take() else {
            return;
        };
        if ran {
            for &state_id in subscribed.iter() {
                if !reads.contains(&state_id) {
                    self.inner.runtime.forget_state_scope(state_id, id);
                }
            }
            *subscribed = *reads;
        } else {
            for state_id in *reads {
                push_unique_state_id(&mut subscribed, state_id);
            }
        }
    }

    fn record_unknown_invalidation_source(&self) {
        self.inner
            .flags
            .set(ScopeFlag::UnknownInvalidationSource, true);
        update_cell(&self.inner.invalidation_sources, StateIds::clear);
    }

    fn record_state_invalidation_source(&self, state_id: StateId) {
        if !self.inner.flags.get(ScopeFlag::UnknownInvalidationSource) {
            update_cell(&self.inner.invalidation_sources, |ids| {
                push_unique_state_id(ids, state_id);
            });
        }
    }

    fn enqueue_invalidation(&self) {
        self.inner.flags.set(ScopeFlag::Invalid, true);
        if self.is_effectively_active() && self.may_queue() {
            self.queue_recompose();
        }
    }

    // Whether the scope may join the recompose queue: it is not in it, and
    // its pausable content, if any, completed.
    fn may_queue(&self) -> bool {
        !self
            .inner
            .flags
            .either(ScopeFlag::Enqueued, ScopeFlag::Pending)
    }

    fn queue_recompose(&self) {
        self.inner.flags.set(ScopeFlag::Enqueued, true);
        self.inner.runtime.register_invalid_scope(self.downgrade());
    }

    pub(crate) fn invalidate(&self) {
        self.record_unknown_invalidation_source();
        self.enqueue_invalidation();
    }

    pub(crate) fn invalidate_from_state(&self, state_id: StateId) {
        self.record_state_invalidation_source(state_id);
        if self.inner.read_cursor.get() != NOT_READING {
            self.note_running_write(state_id);
        }
        self.enqueue_invalidation();
    }

    // A write to `state_id` while the scope's body runs. After a read of it
    // in this run, the body composed from the value the write replaced: it
    // composes again once it closes, as Compose's does after a backward
    // write. A write before any read, as a provider's, needs no second run.
    #[inline(never)]
    fn note_running_write(&self, state_id: StateId) {
        let cursor = self.inner.read_cursor.get();
        let read = if cursor == DIVERGED {
            read_cell(&self.inner.run_reads, |reads| {
                reads
                    .as_ref()
                    .is_some_and(|reads| reads.contains(&state_id))
            })
        } else {
            self.inner.state_subscriptions.borrow()[..cursor as usize].contains(&state_id)
        };
        if read {
            self.request_pending_recompose();
        }
    }

    pub(crate) fn mark_recomposed(&self) {
        // An invalidation notes its source and sets `invalid` together, so a
        // scope nothing invalidated has no source to forget.
        if self.inner.flags.replace(ScopeFlag::Invalid, false) {
            self.inner
                .flags
                .set(ScopeFlag::UnknownInvalidationSource, false);
            update_cell(&self.inner.invalidation_sources, StateIds::clear);
        }
        self.inner.flags.set(ScopeFlag::ForceReuse, false);
        self.inner.flags.set(ScopeFlag::ForceRecompose, false);
        if self.inner.flags.replace(ScopeFlag::Enqueued, false) {
            self.inner.runtime.mark_scope_recomposed();
        }
        let pending = self.inner.flags.replace(ScopeFlag::PendingRecompose, false);
        if pending {
            if self.inner.flags.get(ScopeFlag::Active) {
                self.invalidate();
            } else {
                self.inner.flags.set(ScopeFlag::Invalid, true);
            }
        }
    }

    /// Registers `callback` as the code that recomposes the scope when state
    /// its body read changes.
    pub(crate) fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        self.note_recompose_source();
        self.inner.recompose.set(None);
        self.inner.recompose_closure.set(Some(Box::new(callback)));
    }

    // Registers `body` as the function that recomposes the scope; the same
    // function stays as it is.
    #[inline]
    pub(crate) fn set_recompose_fn(&self, body: fn(&Composer)) {
        let same = self
            .inner
            .recompose
            .get()
            .is_some_and(|current| std::ptr::fn_addr_eq(current, body));
        if !same {
            self.set_recompose(body);
        }
    }

    #[inline(never)]
    fn set_recompose(&self, body: fn(&Composer)) {
        self.note_recompose_source();
        self.inner.recompose.set(Some(body));
        self.inner.recompose_closure.set(None);
    }

    fn note_recompose_source(&self) {
        #[cfg(feature = "inspection")]
        self.inner
            .source_trace
            .replace(source_trace::current_source_trace());
    }

    // Kept out of line: the ESP32-S3 hot-code list places it in IRAM.
    #[inline(never)]
    pub(crate) fn run_recompose(&self, composer: &Composer) -> bool {
        #[cfg(feature = "inspection")]
        let _source_context = source_trace::restore_source_trace(&self.inner.source_trace.borrow());
        let Some(body) = self.inner.recompose.get() else {
            return self.run_recompose_closure(composer);
        };
        self.begin_reads();
        body(composer);
        self.end_reads(BodyRun::Ran);
        true
    }

    #[inline(never)]
    fn run_recompose_closure(&self, composer: &Composer) -> bool {
        let Some(mut closure) = self.inner.recompose_closure.take() else {
            return false;
        };
        self.begin_reads();
        self.inner.flags.set(ScopeFlag::Recomposing, true);
        closure(composer);
        self.inner.flags.set(ScopeFlag::Recomposing, false);
        self.end_reads(BodyRun::Ran);
        // The closure may have stored a new one for the scope while it ran.
        update_cell(&self.inner.recompose_closure, |slot| {
            if slot.is_none() {
                *slot = Some(closure);
            }
        });
        true
    }

    pub(crate) fn has_recompose_callback(&self) -> bool {
        self.inner.recompose.get().is_some()
            || self.inner.flags.get(ScopeFlag::Recomposing)
            || read_cell(&self.inner.recompose_closure, Option::is_some)
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
        if !self.inner.flags.replace(ScopeFlag::Active, false) {
            return;
        }
        note_scope_activity_change();
        self.inner
            .deactivations
            .set(self.inner.deactivations.get() + 1);
        if self.inner.flags.replace(ScopeFlag::Enqueued, false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    pub(crate) fn defer_until_reactivated(&self) {
        if self.inner.flags.replace(ScopeFlag::Enqueued, false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    /// Marks the scope active again and queues it for recomposition if it was invalidated while
    /// inactive.
    pub fn reactivate(&self) {
        self.inner.flags.set(ScopeFlag::Active, true);
        if self.inner.flags.get(ScopeFlag::Invalid)
            && self.is_effectively_active()
            && self.may_queue()
        {
            self.queue_recompose();
        }
    }

    /// Makes the next `should_recompose` return `false` even if the scope is invalid. Cancels a
    /// pending `force_recompose`.
    pub fn force_reuse(&self) {
        self.inner.flags.set(ScopeFlag::ForceReuse, true);
        self.inner.flags.set(ScopeFlag::ForceRecompose, false);
        self.inner.flags.set(ScopeFlag::PendingRecompose, true);
    }

    pub(crate) fn request_pending_recompose(&self) {
        self.inner.flags.set(ScopeFlag::PendingRecompose, true);
    }

    /// Makes the next `should_recompose` return `true` even if the scope is not invalid. Cancels a
    /// pending `force_reuse`.
    pub fn force_recompose(&self) {
        self.inner.flags.set(ScopeFlag::ForceRecompose, true);
        self.inner.flags.set(ScopeFlag::ForceReuse, false);
        self.inner.flags.set(ScopeFlag::PendingRecompose, false);
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
        if self.inner.flags.replace(ScopeFlag::ForceRecompose, false) {
            self.inner.flags.set(ScopeFlag::ForceReuse, false);
            return true;
        }
        if self.inner.flags.replace(ScopeFlag::ForceReuse, false) {
            return false;
        }
        self.is_invalid()
    }

    /// Returns `true` once the scope has composed its content at least once.
    pub fn has_composed_once(&self) -> bool {
        self.inner.flags.get(ScopeFlag::ComposedOnce)
    }

    pub(crate) fn mark_composed_once(&self) {
        self.inner.flags.set(ScopeFlag::ComposedOnce, true);
    }

    // Whether the scope's first body waits for a later frame.
    pub(crate) fn is_paused(&self) -> bool {
        self.inner.flags.get(ScopeFlag::Paused)
    }

    // Sets whether the scope's first body waits, and says whether it waited
    // already.
    pub(crate) fn replace_paused(&self, paused: bool) -> bool {
        self.inner.flags.replace(ScopeFlag::Paused, paused)
    }

    // Whether the scope belongs to pausable content that has not completed.
    pub(crate) fn is_pending(&self) -> bool {
        self.inner.flags.get(ScopeFlag::Pending)
    }

    // Holds the scope's invalidations until its pausable content completes,
    // or, with `false`, queues the invalidation that waited.
    pub(crate) fn set_pending(&self, pending: bool) {
        self.inner.flags.set(ScopeFlag::Pending, pending);
        if !pending && self.is_invalid() {
            self.enqueue_invalidation();
        }
    }

    // Whether a change of the state `state_id` alone invalidated the scope.
    pub(crate) fn invalidated_only_by_state(&self, state_id: StateId) -> bool {
        !self.inner.flags.get(ScopeFlag::UnknownInvalidationSource)
            && read_cell(&self.inner.invalidation_sources, |sources| {
                !sources.is_empty() && sources.iter().all(|source| *source == state_id)
            })
    }

    pub(crate) fn invalidated_only_by(&self, allowed_sources: &HashSet<StateId>) -> Option<bool> {
        if self.inner.flags.get(ScopeFlag::UnknownInvalidationSource) {
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
