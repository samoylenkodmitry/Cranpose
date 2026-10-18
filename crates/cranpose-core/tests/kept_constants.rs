//! The thread's table of constant values that `#[composable]` bodies keep
//! their modifier chains of constants in. Each expression gets the value
//! it makes, whichever other expression shares its slot, and a value is
//! made again only when another took the slot.

use std::cell::Cell;

use cranpose_core::{__constant, Key};

/// A key and another that falls in the same slot of the table.
const KEY: Key = 0x5eed_0000_0000_0001;
const SAME_SLOT: Key = KEY + (1 << 20);

#[test]
fn a_kept_value_is_made_once() {
    let made = Cell::new(0);
    for _ in 0..3 {
        let value = __constant(KEY, || {
            made.set(made.get() + 1);
            String::from("kept")
        });
        assert_eq!(value, "kept");
    }
    assert_eq!(made.get(), 1, "later runs take the kept value");
}

#[test]
fn keys_that_share_a_slot_each_get_their_own_value() {
    let made = Cell::new(0);
    let make = |value: &'static str| {
        made.set(made.get() + 1);
        value
    };
    for _ in 0..2 {
        assert_eq!(__constant(KEY ^ 0x10, || make("first")), "first");
        assert_eq!(__constant(SAME_SLOT ^ 0x10, || make("second")), "second");
    }
    assert_eq!(
        made.get(),
        4,
        "each takes the slot from the other, so each is made again"
    );
}

#[test]
fn one_key_asked_for_two_types_gives_each_its_own() {
    for _ in 0..2 {
        assert_eq!(__constant(KEY ^ 0x20, || 7u32), 7);
        assert_eq!(__constant(KEY ^ 0x20, || String::from("seven")), "seven");
    }
}

#[test]
fn another_thread_makes_its_own_value() {
    assert_eq!(__constant(KEY ^ 0x30, || 1u8), 1);
    let other = std::thread::spawn(|| __constant(KEY ^ 0x30, || 2u8))
        .join()
        .expect("the other thread runs");
    assert_eq!(other, 2, "each thread keeps its own table");
}
