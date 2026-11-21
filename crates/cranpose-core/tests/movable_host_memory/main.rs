// Memory retention checks for dropped and reset `SlotsHost`s carrying movable
// content. Neither `retention_by_host` nor `disposed_attached` is exposed on
// the public API, so these probes run steady warm-up + measured rounds through
// the public `Composition` / `SlotsHost` APIs and assert that repeated host
// churn leaves no heap blocks or bytes behind.

use std::{alloc::System, cell::RefCell, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MovableContent, MutableState, SlotTable, SlotsHost, location_key,
    remember, rememberMovableContentOf, with_current_composer,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[macro_use]
#[path = "../../../coroflow/tests/support/main_thread_test.rs"]
mod main_thread_test;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const WARM_UP_ROUNDS: usize = 16;
const RETENTION_MEASURED_ROUNDS: usize = 256;
const DISPOSED_ATTACHED_MEASURED_ROUNDS: usize = 1_024;

#[derive(Clone, Copy)]
enum Teardown {
    DropHost,
    ResetHost,
}

struct MovableHostRig {
    composition: Composition<MemoryApplier>,
    show_owner: MutableState<bool>,
    show_in_root: MutableState<bool>,
    show_in_host: MutableState<bool>,
    active_host: Rc<RefCell<Option<Rc<SlotsHost>>>>,
    shared_movable: Rc<RefCell<Option<MovableContent>>>,
    root_key: cranpose_core::Key,
}

impl MovableHostRig {
    fn new(teardown: Teardown) -> Self {
        let composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let active_host = match teardown {
            Teardown::DropHost => None,
            Teardown::ResetHost => Some(Rc::new(SlotsHost::new(SlotTable::new()))),
        };
        Self {
            composition,
            show_owner: MutableState::with_runtime(false, runtime.clone()),
            show_in_root: MutableState::with_runtime(false, runtime.clone()),
            show_in_host: MutableState::with_runtime(false, runtime),
            active_host: Rc::new(RefCell::new(active_host)),
            shared_movable: Rc::new(RefCell::new(None)),
            root_key: location_key(file!(), line!(), column!()),
        }
    }

    fn render(&mut self) {
        let show_owner = self.show_owner;
        let show_in_root = self.show_in_root;
        let show_in_host = self.show_in_host;
        let active_host = Rc::clone(&self.active_host);
        let shared_movable = Rc::clone(&self.shared_movable);
        self.composition
            .render(self.root_key, move || {
                if show_owner.value() {
                    let content = rememberMovableContentOf(|| {
                        let _slot = remember(|| 1_u32);
                    });
                    *shared_movable.borrow_mut() = Some(content);
                }
                let content = shared_movable.borrow().clone();
                if let Some(host) = active_host.borrow().clone() {
                    let content = content.clone();
                    with_current_composer(|composer| {
                        composer
                            .subcompose_in(&host, None, |_| {
                                if show_in_host.value()
                                    && let Some(content) = content
                                {
                                    content.show();
                                }
                            })
                            .expect("subcompose_in");
                    });
                }
                if show_in_root.value()
                    && let Some(content) = content
                {
                    content.show();
                }
            })
            .expect("render composition");
    }

    fn ensure_host(&self, teardown: Teardown) {
        if matches!(teardown, Teardown::DropHost) {
            *self.active_host.borrow_mut() = Some(Rc::new(SlotsHost::new(SlotTable::new())));
        }
    }

    fn teardown_host(&self, teardown: Teardown) {
        match teardown {
            Teardown::DropHost => {
                self.active_host.borrow_mut().take();
            }
            Teardown::ResetHost => {
                let host = self
                    .active_host
                    .borrow()
                    .clone()
                    .expect("reused host exists for reset");
                host.reset().expect("reset host");
            }
        }
    }
}

fn assert_zero_leak(label: &str, rounds: usize, mut run_round: impl FnMut()) {
    for _ in 0..WARM_UP_ROUNDS {
        run_round();
    }
    let region = Region::new(GLOBAL);
    for _ in 0..rounds {
        run_round();
    }
    let change = region.change();
    let live_bytes = change.bytes_allocated as isize - change.bytes_deallocated as isize;
    let live_blocks = change.allocations as isize - change.deallocations as isize;
    assert_eq!(
        live_blocks, 0,
        "{label}: left {live_blocks} live heap blocks ({live_bytes} bytes) over {rounds} rounds: \
         {change:?}"
    );
    assert_eq!(
        live_bytes, 0,
        "{label}: left {live_bytes} live bytes over {rounds} rounds: {change:?}"
    );
}

// A host whose retained movable was taken back before teardown must remove its
// empty `RetentionManager` entry from `retention_by_host` even when
// `retained_groups == 0`.
fn check_empty_retention_cleanup(label: &str, teardown: Teardown) {
    let mut rig = MovableHostRig::new(teardown);
    assert_zero_leak(label, RETENTION_MEASURED_ROUNDS, || {
        rig.ensure_host(teardown);
        rig.show_owner.set_value(true);
        rig.show_in_host.set_value(true);
        rig.show_in_root.set_value(false);
        rig.render();

        // Hide in host so the host retains the movable in its RetentionManager.
        rig.show_in_host.set_value(false);
        rig.render();

        // Show in root so root takes the movable back, leaving the host's
        // RetentionManager empty while `retained_groups` drops to zero.
        rig.show_in_root.set_value(true);
        rig.render();

        // Drop or reset the host while no group is retained anywhere.
        rig.teardown_host(teardown);

        // Clean up the root movable before the next round.
        rig.show_owner.set_value(false);
        rig.show_in_root.set_value(false);
        rig.shared_movable.borrow_mut().take();
        rig.render();
    });
}

// Dropping or resetting a host that holds an owner-disposed movable must
// forget its id from `disposed_attached` before `take_all_drops` clears the
// slot table.
fn check_disposed_attached_cleanup(label: &str, teardown: Teardown) {
    let mut rig = MovableHostRig::new(teardown);
    assert_zero_leak(label, DISPOSED_ATTACHED_MEASURED_ROUNDS, || {
        rig.ensure_host(teardown);
        rig.show_owner.set_value(true);
        rig.show_in_host.set_value(true);
        rig.render();

        // Drop the owner in root while the secondary host still displays the
        // movable, recording its id in `disposed_attached`.
        rig.show_owner.set_value(false);
        rig.render();

        rig.teardown_host(teardown);
        rig.shared_movable.borrow_mut().take();
    });
}

fn dropped_and_reset_hosts_leave_no_movable_state_behind() {
    check_empty_retention_cleanup("dropped host empty retention", Teardown::DropHost);
    check_empty_retention_cleanup("reset host empty retention", Teardown::ResetHost);
    check_disposed_attached_cleanup("dropped host disposed_attached", Teardown::DropHost);
    check_disposed_attached_cleanup("reset host disposed_attached", Teardown::ResetHost);
}

main_thread_test!(dropped_and_reset_hosts_leave_no_movable_state_behind);
