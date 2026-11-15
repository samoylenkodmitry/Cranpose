use std::num::NonZeroUsize;

use super::{LinkedSlots, Order};

#[test]
fn a_cache_reserves_room_only_for_the_entries_it_holds() {
    let cap = NonZeroUsize::new(8192).expect("a nonzero bound");
    let mut slots: LinkedSlots<u64, [u8; 64]> = LinkedSlots::new();
    let mut order = Order::default();
    assert_eq!(
        (slots.index.capacity(), slots.slots.capacity()),
        (0, 0),
        "a cache no text reaches must not hold room for its whole bound"
    );
    for key in 0..10 {
        let slot = slots.insert(slots.hash(&key), (key, [0; 64]), cap);
        slots.link_newest(slot, &mut order);
    }
    assert!(slots.slots.capacity() < 64, "{}", slots.slots.capacity());
}
