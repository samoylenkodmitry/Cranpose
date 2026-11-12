// The room a store's arrays keep past their entries, given back at rest: an
// array that doubled to take one more record would keep room for as many
// again, so the trim gives back room past a quarter of the entries. With
// more than half of the rows free, it cuts the free rows at the end, and the
// free entries at the end of the other arrays with them, so a scene that
// shrank gives back the room of its peak. The item spans move together once
// free spans hold more than half of their room, and the shape chunks drop
// the shapes no item draws. Free lists keep their room, which the next
// removals take again.

use cranpose_core::collections::growth_slack::GrowthSlack;

use super::SceneStore;

pub(super) fn trim(store: &mut SceneStore) {
    if 2 * store.free_rows.len() > store.tree.len() && cut_free_rows(store) {
        cut_free_tails(store);
    }
    store
        .items
        .pack(store.tree.iter_mut().map(|row| &mut row.items));
    store.lists.shrink_to(store.items.len());
    #[cfg(feature = "window-damage")]
    store
        .marks
        .pool
        .pack(store.warm.iter_mut().map(|row| &mut row.own_marks));
    store.trim_shapes();
    store.tree.trim_growth_slack();
    store.geo.trim_growth_slack();
    store.warm.trim_growth_slack();
    store.generations.trim_growth_slack();
    store.node_records.trim_growth_slack();
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
// rows held entries of.
fn cut_free_tails(store: &mut SceneStore) {
    store.props.cut_free_tail();
    store.graphics_layers.cut_free_tail();
    store.transforms.cut_free_tail();
    store.draws.cut_free_tail();
    store.texts.cut_free_tail();
    store.runs.cut_free_tail();
    store.shape_runs.cut_free_tail();
}

// Cuts the free rows at the end of the row arrays. The free list is sorted
// once, so its tail holds the rows to cut; then it is turned, so new records
// take the lowest rows first and leave the end free for the next cut. A row
// made past the cut starts at a generation past those of the rows cut.
fn cut_free_rows(store: &mut SceneStore) -> bool {
    let rows = store.tree.len();
    let mut keep = rows;
    store.free_rows.sort_unstable();
    while store
        .free_rows
        .last()
        .is_some_and(|&row| row as usize + 1 == keep)
    {
        store.free_rows.pop();
        keep -= 1;
    }
    store.free_rows.reverse();
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
