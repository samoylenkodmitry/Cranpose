use super::*;

// Hashes spread over the top bits, which pick the slot, and the low bits,
// which give the tag.
fn hash(index: u64) -> u64 {
    index.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[test]
fn a_grown_set_keeps_the_hashes_put_before() {
    let mut direct = RecentHashes::<1>::new(64);
    let mut four_ways = RecentHashes::<4>::new(64);
    let put: Vec<u64> = (0..24).map(hash).collect();
    for &h in &put {
        direct.put(h);
        four_ways.put(h);
    }
    let kept_direct: Vec<u64> = put
        .iter()
        .copied()
        .filter(|&h| direct.contains(h))
        .collect();
    let kept_four: Vec<u64> = put
        .iter()
        .copied()
        .filter(|&h| four_ways.contains(h))
        .collect();

    direct.grow_to(1024);
    four_ways.grow_to(1024);

    assert_eq!((direct.slots(), four_ways.slots()), (1024, 1024));
    for &h in &kept_direct {
        assert!(direct.contains(h), "{h:#x} put before the growth");
    }
    for &h in &kept_four {
        assert!(four_ways.contains(h), "{h:#x} put before the growth");
    }
    assert!(!direct.contains(hash(1_000)), "a hash never put");
    assert!(!four_ways.contains(hash(1_000)), "a hash never put");
}

#[test]
fn hashes_of_one_bucket_stay_while_its_ways_hold_them() {
    // Four hashes with the same top bits, which pick one bucket.
    let shared: Vec<u64> = (1..=4)
        .map(|low| (0xabcd_u64 << 48) | (low << 36))
        .collect();
    let mut direct = RecentHashes::<1>::new(64);
    let mut four_ways = RecentHashes::<4>::new(64);
    for &h in &shared {
        direct.put(h);
        four_ways.put(h);
    }

    assert_eq!(
        shared.iter().filter(|&&h| direct.contains(h)).count(),
        1,
        "one way keeps the newest"
    );
    assert!(shared.iter().all(|&h| four_ways.contains(h)));

    four_ways.put((0xabcd_u64 << 48) | (5 << 36));
    assert!(
        !four_ways.contains((0xabcd_u64 << 48) | (1 << 36)),
        "the oldest leaves a full bucket"
    );
    assert!(four_ways.contains((0xabcd_u64 << 48) | (2 << 36)));
}

#[test]
fn a_set_grows_only_past_the_slots_it_holds() {
    let mut hashes = RecentHashes::<1>::new(256);
    hashes.put(hash(1));

    hashes.grow_to(100);

    assert_eq!(hashes.slots(), 256);
    assert!(hashes.contains(hash(1)));
}

#[test]
fn a_set_grown_before_its_first_put_takes_the_grown_room() {
    let mut hashes = RecentHashes::<1>::new(2);

    hashes.grow_to(512);
    for index in 0..300 {
        hashes.put(hash(index));
    }

    assert_eq!(hashes.slots(), 512);
    let kept = (0..300)
        .filter(|&index| hashes.contains(hash(index)))
        .count();
    assert!(kept > 150, "{kept} of 300 hashes in 512 slots");
}
