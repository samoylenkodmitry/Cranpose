use cranpose_ui_graphics::shared_sort;

// Keys with ties and orders that put items through cycles of every length,
// from a fixed generator, so a run is the same each time.
fn keys(count: usize, seed: u32) -> Vec<(u32, usize)> {
    let mut state = seed;
    (0..count)
        .map(|position| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 24) % 7, position)
        })
        .collect()
}

#[test]
fn shared_sort_matches_the_standard_stable_sort() {
    for count in [0, 1, 2, 3, 17, 64, 1500] {
        for seed in 0..4 {
            let mut shared = keys(count, seed);
            let mut standard = shared.clone();
            shared_sort::sort_by(&mut shared, |a, b| a.0.cmp(&b.0));
            standard.sort_by_key(|&(key, _)| key);
            assert_eq!(shared, standard, "{count} items, seed {seed}");
        }
    }
}

#[test]
fn sorted_positions_keep_ties_in_order() {
    let items = keys(1500, 9);
    let mut positions: Vec<usize> = (0..items.len()).collect();
    shared_sort::sort_positions(&mut positions, &mut |a, b| items[b].0.cmp(&items[a].0));
    let sorted: Vec<_> = positions.iter().map(|&position| items[position]).collect();
    let mut standard = items;
    standard.sort_by_key(|&(key, _)| std::cmp::Reverse(key));
    assert_eq!(sorted, standard);
}
