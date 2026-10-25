use std::rc::Rc;

use super::*;
use crate::runtime::{StateHandleLease, TestRuntime};

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
