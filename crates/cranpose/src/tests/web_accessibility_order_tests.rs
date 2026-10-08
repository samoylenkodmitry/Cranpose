use super::*;

/// Applies the moves for `new` to the children `old` that are still in the
/// parent, the way the mirror inserts nodes, and returns how many moved and
/// the order they end in.
fn reorder(old: &[char], left: &[char], new: &[char]) -> (usize, Vec<char>) {
    let mut moves = ChildMoves::default();
    moves.plan(
        new.iter()
            .map(|child| old.iter().position(|was| was == child)),
    );
    let mut parent: Vec<char> = old
        .iter()
        .copied()
        .filter(|child| !left.contains(child))
        .collect();
    let mut moved = 0;
    for (child, before) in moves.moves() {
        let child = new[child];
        parent.retain(|placed| *placed != child);
        let at = match before {
            Some(before) => parent
                .iter()
                .position(|placed| *placed == new[before])
                .unwrap_or_else(|| panic!("{} is not in the parent", new[before])),
            None => parent.len(),
        };
        parent.insert(at, child);
        moved += 1;
    }
    (moved, parent)
}

#[test]
fn only_the_children_out_of_order_move() {
    let old = ['a', 'b', 'c', 'd', 'e'];
    let rotated = ['b', 'c', 'd', 'e', 'a'];
    assert_eq!(reorder(&old, &[], &rotated), (1, rotated.to_vec()));
    let reversed = ['e', 'd', 'c', 'b', 'a'];
    assert_eq!(reorder(&old, &[], &reversed), (4, reversed.to_vec()));
    let mixed = ['d', 'a', 'x', 'b', 'e'];
    assert_eq!(reorder(&old, &['c'], &mixed), (2, mixed.to_vec()));
}
