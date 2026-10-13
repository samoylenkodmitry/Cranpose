use std::{
    any::Any,
    cell::{Cell, RefCell},
    ops::Deref,
    rc::Rc,
    thread::LocalKey,
};

use crate::{
    CommandQueue, ComposerCore, HotOrigin, Key,
    collections::{FixedHashMap, FixedHashSet, fixed_hash_map, fixed_hash_set},
    runtime::{DeferredStateRelease, RuntimeHandle, RuntimeId, StateHandleLease},
    snapshot_id_set::SnapshotId,
    snapshot_pinning::PinningTable,
    snapshot_v2::{
        AnySnapshot, ApplyObserver, GlobalSnapshot, ObserverId, SnapshotRuntime, StateObjectId,
    },
    snapshot_weak_set::SnapshotWeakSet,
    state::{ObjectId, RecordValue},
};

pub(crate) const CALL_SITE_KEY_SLOTS: usize = 1024;
pub(crate) const FILE_HASH_SLOTS: usize = 64;

pub(crate) struct ThreadCells {
    pub(crate) call_site_keys: [Cell<(usize, Key)>; CALL_SITE_KEY_SLOTS],
    pub(crate) file_hashes: [Cell<(usize, usize, u64)>; FILE_HASH_SLOTS],
    pub(crate) hot_origin: Cell<HotOrigin>,
    pub(crate) in_event_handler: Cell<bool>,
    pub(crate) in_applied_snapshot: Cell<bool>,
    pub(crate) scope_activity_epoch: Cell<u64>,
    pub(crate) thread_exiting: Cell<bool>,
    #[cfg(any(test, debug_assertions))]
    pub(crate) location_key_collisions: Cell<usize>,
    #[cfg(all(test, debug_assertions))]
    pub(crate) scope_tracking_override: Cell<Option<bool>>,
    #[cfg(all(feature = "inspection", debug_assertions))]
    pub(crate) track_recompositions: Cell<bool>,
    #[cfg(all(feature = "inspection", debug_assertions))]
    pub(crate) next_recomposition_instance: Cell<u64>,
}

const _: () = assert!(!std::mem::needs_drop::<ThreadCells>());

impl ThreadCells {
    const fn new() -> Self {
        Self {
            call_site_keys: [const { Cell::new((0, 0)) }; CALL_SITE_KEY_SLOTS],
            file_hashes: [const { Cell::new((0, 0, 0)) }; FILE_HASH_SLOTS],
            hot_origin: Cell::new(HotOrigin::NONE),
            in_event_handler: Cell::new(false),
            in_applied_snapshot: Cell::new(false),
            scope_activity_epoch: Cell::new(1),
            thread_exiting: Cell::new(false),
            #[cfg(any(test, debug_assertions))]
            location_key_collisions: Cell::new(0),
            #[cfg(all(test, debug_assertions))]
            scope_tracking_override: Cell::new(None),
            #[cfg(all(feature = "inspection", debug_assertions))]
            track_recompositions: Cell::new(false),
            #[cfg(all(feature = "inspection", debug_assertions))]
            next_recomposition_instance: Cell::new(0),
        }
    }
}

pub(crate) struct SnapshotThreadState {
    pub(crate) runtime: RefCell<Option<SnapshotRuntime>>,
    pub(crate) runtime_lock_depth: Cell<usize>,
    pub(crate) global: RefCell<Option<Rc<GlobalSnapshot>>>,
    pub(crate) current: RefCell<Option<AnySnapshot>>,
    pub(crate) apply_observers: RefCell<FixedHashMap<ObserverId, ApplyObserver>>,
    pub(crate) last_writes: RefCell<FixedHashMap<StateObjectId, SnapshotId>>,
    pub(crate) extra_state_objects: RefCell<SnapshotWeakSet>,
    pub(crate) last_unused_record_cleanup: Cell<SnapshotId>,
    pub(crate) pinning: RefCell<PinningTable>,
    pub(crate) active_updates: RefCell<FixedHashSet<ObjectId>>,
    pub(crate) pending_writes: RefCell<FixedHashSet<ObjectId>>,
    pub(crate) retired_record_values: RefCell<Vec<Box<dyn RecordValue>>>,
    pub(crate) retire_depth: Cell<usize>,
}

impl SnapshotThreadState {
    const fn new() -> Self {
        Self {
            runtime: RefCell::new(None),
            runtime_lock_depth: Cell::new(0),
            global: RefCell::new(None),
            current: RefCell::new(None),
            apply_observers: RefCell::new(fixed_hash_map()),
            last_writes: RefCell::new(fixed_hash_map()),
            extra_state_objects: RefCell::new(SnapshotWeakSet::new()),
            last_unused_record_cleanup: Cell::new(0),
            pinning: RefCell::new(PinningTable::new()),
            active_updates: RefCell::new(fixed_hash_set()),
            pending_writes: RefCell::new(fixed_hash_set()),
            retired_record_values: RefCell::new(Vec::new()),
            retire_depth: Cell::new(0),
        }
    }

    fn swap_contents(&self, other: &Self) {
        let Self {
            runtime,
            runtime_lock_depth,
            global,
            current,
            apply_observers,
            last_writes,
            extra_state_objects,
            last_unused_record_cleanup,
            pinning,
            active_updates,
            pending_writes,
            retired_record_values,
            retire_depth,
        } = self;
        swap_values(runtime, &other.runtime);
        runtime_lock_depth.swap(&other.runtime_lock_depth);
        swap_values(global, &other.global);
        swap_values(current, &other.current);
        swap_values(apply_observers, &other.apply_observers);
        swap_values(last_writes, &other.last_writes);
        swap_values(extra_state_objects, &other.extra_state_objects);
        last_unused_record_cleanup.swap(&other.last_unused_record_cleanup);
        swap_values(pinning, &other.pinning);
        swap_values(active_updates, &other.active_updates);
        swap_values(pending_writes, &other.pending_writes);
        swap_values(retired_record_values, &other.retired_record_values);
        retire_depth.swap(&other.retire_depth);
    }

    fn release(&self) -> Self {
        let released = Self::new();
        self.swap_contents(&released);
        self.runtime_lock_depth
            .set(released.runtime_lock_depth.get());
        self.retire_depth.set(released.retire_depth.get());
        released
    }
}

pub(crate) struct RuntimeThreadState {
    pub(crate) next_runtime_id: Cell<u32>,
    pub(crate) active_runtimes: RefCell<Vec<RuntimeHandle>>,
    pub(crate) last_runtime: RefCell<Option<RuntimeHandle>>,
    pub(crate) registered_runtimes: RefCell<FixedHashMap<RuntimeId, RuntimeHandle>>,
    pub(crate) next_task_label: RefCell<Option<String>>,
    pub(crate) state_owners: RefCell<Vec<Vec<Rc<StateHandleLease>>>>,
    pub(crate) state_teardown_depth: Cell<usize>,
    pub(crate) deferred_state_releases: RefCell<Vec<DeferredStateRelease>>,
    pub(crate) current_composer: RefCell<Option<Rc<ComposerCore>>>,
    pub(crate) spare_commands: RefCell<Vec<CommandQueue>>,
    pub(crate) bridges: RefCell<FixedHashMap<u64, Rc<dyn Any>>>,
    #[cfg(any(test, debug_assertions))]
    pub(crate) location_keys: RefCell<FixedHashMap<Key, crate::LocationKeyDebugInfo>>,
    #[cfg(debug_assertions)]
    pub(crate) scope_labels: RefCell<FixedHashMap<usize, &'static str>>,
    #[cfg(debug_assertions)]
    pub(crate) scope_invalidation_sources:
        RefCell<FixedHashMap<usize, crate::collections::map::HashSet<String>>>,
    #[cfg(all(feature = "inspection", debug_assertions))]
    pub(crate) recomposition_counters: RefCell<Vec<std::rc::Weak<crate::RecomposeScopeInner>>>,
    #[cfg(feature = "inspection")]
    pub(crate) source_stack: RefCell<Vec<crate::source_trace::SourceLocation>>,
}

impl RuntimeThreadState {
    const fn new() -> Self {
        Self {
            next_runtime_id: Cell::new(1),
            active_runtimes: RefCell::new(Vec::new()),
            last_runtime: RefCell::new(None),
            registered_runtimes: RefCell::new(fixed_hash_map()),
            next_task_label: RefCell::new(None),
            state_owners: RefCell::new(Vec::new()),
            state_teardown_depth: Cell::new(0),
            deferred_state_releases: RefCell::new(Vec::new()),
            current_composer: RefCell::new(None),
            spare_commands: RefCell::new(Vec::new()),
            bridges: RefCell::new(fixed_hash_map()),
            #[cfg(any(test, debug_assertions))]
            location_keys: RefCell::new(fixed_hash_map()),
            #[cfg(debug_assertions)]
            scope_labels: RefCell::new(fixed_hash_map()),
            #[cfg(debug_assertions)]
            scope_invalidation_sources: RefCell::new(fixed_hash_map()),
            #[cfg(all(feature = "inspection", debug_assertions))]
            recomposition_counters: RefCell::new(Vec::new()),
            #[cfg(feature = "inspection")]
            source_stack: RefCell::new(Vec::new()),
        }
    }

    fn has_live_runtime(&self) -> bool {
        self.state_teardown_depth.get() > 0 || !self.registered_runtimes.borrow().is_empty()
    }

    fn swap_contents(&self, other: &Self) {
        let Self {
            next_runtime_id,
            active_runtimes,
            last_runtime,
            registered_runtimes,
            next_task_label,
            state_owners,
            state_teardown_depth,
            deferred_state_releases,
            current_composer,
            spare_commands,
            bridges,
            #[cfg(any(test, debug_assertions))]
            location_keys,
            #[cfg(debug_assertions)]
            scope_labels,
            #[cfg(debug_assertions)]
            scope_invalidation_sources,
            #[cfg(all(feature = "inspection", debug_assertions))]
            recomposition_counters,
            #[cfg(feature = "inspection")]
            source_stack,
        } = self;
        next_runtime_id.swap(&other.next_runtime_id);
        swap_values(active_runtimes, &other.active_runtimes);
        swap_values(last_runtime, &other.last_runtime);
        swap_values(registered_runtimes, &other.registered_runtimes);
        swap_values(next_task_label, &other.next_task_label);
        swap_values(state_owners, &other.state_owners);
        state_teardown_depth.swap(&other.state_teardown_depth);
        swap_values(deferred_state_releases, &other.deferred_state_releases);
        swap_values(current_composer, &other.current_composer);
        swap_values(spare_commands, &other.spare_commands);
        swap_values(bridges, &other.bridges);
        #[cfg(any(test, debug_assertions))]
        swap_values(location_keys, &other.location_keys);
        #[cfg(debug_assertions)]
        swap_values(scope_labels, &other.scope_labels);
        #[cfg(debug_assertions)]
        swap_values(
            scope_invalidation_sources,
            &other.scope_invalidation_sources,
        );
        #[cfg(all(feature = "inspection", debug_assertions))]
        swap_values(recomposition_counters, &other.recomposition_counters);
        #[cfg(feature = "inspection")]
        swap_values(source_stack, &other.source_stack);
    }

    fn release(&self) -> Self {
        let released = Self::new();
        self.swap_contents(&released);
        self.next_runtime_id.set(released.next_runtime_id.get());
        self.state_teardown_depth
            .set(released.state_teardown_depth.get());
        released
    }
}

fn swap_values<T>(first: &RefCell<T>, second: &RefCell<T>) {
    if let (Ok(mut first), Ok(mut second)) = (first.try_borrow_mut(), second.try_borrow_mut()) {
        std::mem::swap(&mut *first, &mut *second);
    }
}

#[cfg(not(target_os = "android"))]
pub(crate) struct Kept<T>(std::mem::ManuallyDrop<T>);

#[cfg(not(target_os = "android"))]
impl<T> Kept<T> {
    const fn new(value: T) -> Self {
        Self(std::mem::ManuallyDrop::new(value))
    }
}

#[cfg(not(target_os = "android"))]
impl<T> Deref for Kept<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

#[cfg(target_os = "android")]
pub(crate) trait RescueAtThreadExit {
    fn rescue(&mut self);
}

#[cfg(target_os = "android")]
pub(crate) struct Kept<T: RescueAtThreadExit>(T);

#[cfg(target_os = "android")]
impl<T: RescueAtThreadExit> Kept<T> {
    const fn new(value: T) -> Self {
        Self(value)
    }
}

#[cfg(target_os = "android")]
impl<T: RescueAtThreadExit> Deref for Kept<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

#[cfg(target_os = "android")]
impl<T: RescueAtThreadExit> Drop for Kept<T> {
    fn drop(&mut self) {
        self.0.rescue();
    }
}

#[cfg(target_os = "android")]
#[derive(Default)]
pub(crate) struct RescuedThreadState {
    snapshots: RefCell<Option<SnapshotThreadState>>,
    runtimes: RefCell<Option<RuntimeThreadState>>,
}

#[cfg(target_os = "android")]
impl RescueAtThreadExit for RuntimeThreadState {
    fn rescue(&mut self) {
        let Some(rescued) =
            crate::runtime::share_rescue(self.registered_runtimes.get_mut().values())
        else {
            return;
        };
        let snapshots = SNAPSHOTS.try_with(|state| state.release()).ok();
        let mut rescued_snapshots = rescued.snapshots.borrow_mut();
        if rescued_snapshots.is_none() {
            *rescued_snapshots = snapshots;
        }
        drop(rescued_snapshots);
        *rescued.runtimes.borrow_mut() = Some(std::mem::replace(self, Self::new()));
    }
}

#[cfg(target_os = "android")]
impl RescueAtThreadExit for SnapshotThreadState {
    fn rescue(&mut self) {
        let Some(Some(rescued)) = RUNTIMES
            .try_with(|state| {
                crate::runtime::share_rescue(state.registered_runtimes.borrow().values())
            })
            .ok()
        else {
            return;
        };
        *rescued.snapshots.borrow_mut() = Some(std::mem::replace(self, Self::new()));
    }
}

#[cfg(target_os = "android")]
pub(crate) fn restore(rescued: &RescuedThreadState) {
    if let Some(saved) = rescued.runtimes.take() {
        let _ = RUNTIMES.try_with(|current| current.swap_contents(&saved));
    }
    if let Some(saved) = rescued.snapshots.take() {
        let _ = SNAPSHOTS.try_with(|current| current.swap_contents(&saved));
    }
}

#[cfg(not(target_os = "android"))]
const _: () = assert!(
    !std::mem::needs_drop::<Kept<SnapshotThreadState>>()
        && !std::mem::needs_drop::<Kept<RuntimeThreadState>>()
);

#[cfg(not(target_os = "android"))]
struct ThreadExit;

#[cfg(not(target_os = "android"))]
impl Drop for ThreadExit {
    fn drop(&mut self) {
        CELLS.with(|cells| cells.thread_exiting.set(true));
        release_when_idle();
    }
}

thread_local! {
    static CELLS: ThreadCells = const { ThreadCells::new() };
    static SNAPSHOTS: Kept<SnapshotThreadState> = const { Kept::new(SnapshotThreadState::new()) };
    static RUNTIMES: Kept<RuntimeThreadState> = const { Kept::new(RuntimeThreadState::new()) };
}

#[cfg(not(target_os = "android"))]
thread_local! {
    static THREAD_EXIT: ThreadExit = const { ThreadExit };
}

pub(crate) fn watch_thread_exit() {
    #[cfg(not(target_os = "android"))]
    let _ = THREAD_EXIT.try_with(|_| ());
}

pub(crate) fn release_when_idle() {
    if !CELLS.with(|cells| cells.thread_exiting.get()) {
        return;
    }
    let Some(runtimes) = RUNTIMES
        .try_with(|state| (!state.has_live_runtime()).then(|| state.release()))
        .ok()
        .flatten()
    else {
        return;
    };
    let snapshots = SNAPSHOTS.try_with(|state| state.release()).ok();
    drop(runtimes);
    drop(snapshots);
}

pub(crate) fn cells<R>(f: impl FnOnce(&ThreadCells) -> R) -> R {
    CELLS.with(f)
}

pub(crate) fn snapshots<R>(f: impl FnOnce(&SnapshotThreadState) -> R) -> R {
    SNAPSHOTS.with(|state| f(state))
}

pub(crate) fn try_snapshots<R>(f: impl FnOnce(&SnapshotThreadState) -> R) -> Option<R> {
    SNAPSHOTS.try_with(|state| f(state)).ok()
}

pub(crate) fn runtimes<R>(f: impl FnOnce(&RuntimeThreadState) -> R) -> R {
    RUNTIMES.with(|state| f(state))
}

pub(crate) fn try_runtimes<R>(f: impl FnOnce(&RuntimeThreadState) -> R) -> Option<R> {
    RUNTIMES.try_with(|state| f(state)).ok()
}

/// Runs `f` on this thread's value of `key`, or on a fresh value from `fresh`
/// once thread exit has destroyed it, so a destructor that runs late at thread
/// exit finds an empty framework state instead of aborting the process.
pub fn with_or_fresh<T: 'static, R>(
    key: &'static LocalKey<T>,
    fresh: impl FnOnce() -> T,
    f: impl FnOnce(&T) -> R,
) -> R {
    let mut f = Some(f);
    let mut call = |state: &T| f.take().map(|f| f(state));
    key.try_with(&mut call)
        .ok()
        .flatten()
        .or_else(|| call(&fresh()))
        .expect("the closure runs on exactly one state")
}
