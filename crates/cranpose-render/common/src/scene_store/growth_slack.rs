// The room a store's arrays keep past their entries: an array that doubled
// to take one more record would keep room for as many again, so each update
// ends by giving back room past a quarter of the entries. An update that
// removed the record of the last row cuts the free rows at the end, and the
// free entries at the end of the other arrays with them, so a scene that
// shrank gives back the room of its peak. Free lists keep their room, which
// the next removals take again.

use cranpose_core::collections::growth_slack::GrowthSlack;

use super::SceneStore;

// Most updates change the length of no array: the arrays that did give
// back their room out of line.
#[inline]
pub(super) fn trim(store: &mut SceneStore) {
    let resized = store.facts.resized()
        | store.items.resized()
        | store.props.resized()
        | store.graphics_layers.resized()
        | store.transforms.resized()
        | store.draws.resized()
        | store.texts.resized()
        | store.runs.resized();
    #[cfg(feature = "window-damage")]
    let resized = resized | store.marks.pool.resized();
    if resized {
        trim_resized(store);
    }
}

#[inline(never)]
fn trim_resized(store: &mut SceneStore) {
    // A record's four rows and its facts grow, clear and give back room
    // together, so the facts speak for the rows' length and the tree row's
    // room for all four.
    let cut = store.facts.resized() && cut_free_rows(store);
    if cut {
        cut_free_tails(store);
    }
    if store.facts.resized() {
        let rows = store.tree.capacity();
        store.tree.trim_growth_slack();
        if store.tree.capacity() != rows {
            store.geo.trim_growth_slack();
            store.warm.trim_growth_slack();
            store.generations.trim_growth_slack();
        }
    }
    store.facts.trim_growth_slack();
    store.items.trim_growth_slack();
    #[cfg(feature = "window-damage")]
    store.marks.pool.trim_growth_slack();
    store.props.trim_growth_slack();
    store.graphics_layers.trim_growth_slack();
    store.transforms.trim_growth_slack();
    store.draws.trim_growth_slack();
    store.texts.trim_growth_slack();
    store.runs.trim_growth_slack();
    store.shape_runs.trim_growth_slack();
}

// Cuts the free entries at the end of the arrays the records of the cut
// rows held entries of, and gives back the room the kept lists took past
// the items: a record holds no more items than the store does.
fn cut_free_tails(store: &mut SceneStore) {
    store.items.cut_free_tail();
    store.lists.shrink_to(store.items.len());
    store.props.cut_free_tail();
    store.graphics_layers.cut_free_tail();
    store.transforms.cut_free_tail();
    store.draws.cut_free_tail();
    store.texts.cut_free_tail();
    store.runs.cut_free_tail();
    store.shape_runs.cut_free_tail();
}

// Cuts the free rows at the end of the row arrays, one search of the free
// list a row: an update frees few rows, and a shrink that frees many cuts
// them once. A row made past the cut starts at a generation past those of
// the rows cut.
fn cut_free_rows(store: &mut SceneStore) -> bool {
    let rows = store.tree.len();
    let mut keep = rows;
    while let Some(at) = store
        .free_rows
        .iter()
        .position(|&row| row as usize + 1 == keep)
    {
        store.free_rows.swap_remove(at);
        keep -= 1;
    }
    if keep == rows {
        return false;
    }
    if let Some(&last) = store.generations[keep..].iter().max() {
        store.generation_floor = store.generation_floor.max(last);
    }
    store.free_rows.trim_growth_slack();
    store.tree.truncate(keep);
    store.geo.truncate(keep);
    store.warm.truncate(keep);
    store.generations.truncate(keep);
    store.facts.truncate_rows(keep);
    true
}
