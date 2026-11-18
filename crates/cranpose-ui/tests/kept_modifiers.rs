// The thread's table of kept modifiers that `#[composable]` bodies keep
// their modifier chains of constants in. Each chain gets the modifier it
// makes, whichever other chain shares its slot, and a chain is made again
// only when another took its slot.

use std::cell::Cell;

use cranpose_core::Key;
use cranpose_ui::{__kept_modifier, Color, Modifier};

// A key and another that falls in the same slot of the table.
const KEY: u64 = 0x5eed_0000_0000_0001;
const SAME_SLOT: Key = Key(KEY + (1 << 20));

fn chain(padding: f32) -> Modifier {
    Modifier::empty().padding(padding).background(Color::RED)
}

#[test]
fn a_kept_modifier_is_made_once() {
    let made = Cell::new(0);
    for _ in 0..3 {
        let modifier = __kept_modifier(Key(KEY), || {
            made.set(made.get() + 1);
            chain(1.0)
        });
        assert_eq!(modifier, chain(1.0), "each run gives the chain's modifier");
    }
    assert_eq!(made.get(), 1, "later runs take the kept modifier");
}

#[test]
fn keys_that_share_a_slot_each_get_their_own_modifier() {
    let made = Cell::new(0);
    let make = |padding: f32| {
        made.set(made.get() + 1);
        chain(padding)
    };
    for _ in 0..2 {
        assert_eq!(
            __kept_modifier(Key(KEY ^ 0x10), || make(1.0)),
            chain(1.0),
            "the first key gets its own modifier"
        );
        assert_eq!(
            __kept_modifier(Key(SAME_SLOT.0 ^ 0x10), || make(2.0)),
            chain(2.0),
            "the key of the same slot gets its own modifier"
        );
    }
    assert_eq!(
        made.get(),
        4,
        "each takes the slot from the other, so each is made again"
    );
}

#[test]
fn another_thread_makes_its_own_modifier() {
    assert_eq!(
        __kept_modifier(Key(KEY ^ 0x30), || chain(1.0)),
        chain(1.0),
        "this thread keeps its modifier"
    );
    let its_own =
        std::thread::spawn(|| __kept_modifier(Key(KEY ^ 0x30), || chain(2.0)) == chain(2.0))
            .join()
            .expect("the other thread runs");
    assert!(its_own, "each thread keeps its own table");
}
