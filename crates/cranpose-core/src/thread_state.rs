use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
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

impl Drop for SnapshotThreadState {
    fn drop(&mut self) {
        crate::runtime::forget_registered_runtimes();
    }
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
}

thread_local! {
    static CELLS: ThreadCells = const { ThreadCells::new() };
    static SNAPSHOTS: SnapshotThreadState = const { SnapshotThreadState::new() };
    static RUNTIMES: RuntimeThreadState = const { RuntimeThreadState::new() };
}

pub(crate) fn cells<R>(f: impl FnOnce(&ThreadCells) -> R) -> R {
    CELLS.with(f)
}

pub(crate) fn snapshots<R>(f: impl FnOnce(&SnapshotThreadState) -> R) -> R {
    SNAPSHOTS.with(f)
}

pub(crate) fn try_snapshots<R>(f: impl FnOnce(&SnapshotThreadState) -> R) -> Option<R> {
    SNAPSHOTS.try_with(f).ok()
}

pub(crate) fn runtimes<R>(f: impl FnOnce(&RuntimeThreadState) -> R) -> R {
    RUNTIMES.with(f)
}

pub(crate) fn try_runtimes<R>(f: impl FnOnce(&RuntimeThreadState) -> R) -> Option<R> {
    RUNTIMES.try_with(f).ok()
}
