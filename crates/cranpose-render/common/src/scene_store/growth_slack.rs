// The room a store's arrays keep past their entries: an array that doubled
// to take one more record would keep room for as many again, so each update
// ends by giving back room past a quarter of the entries. Free lists keep
// their room, which the next removals take again.

use cranpose_core::collections::growth_slack::GrowthSlack;

use super::SceneStore;

// Most updates change the length of no array: the arrays that did give
// back their room out of line.
#[inline]
pub(super) fn trim(store: &mut SceneStore) {
    let resized = store.facts.resized()
        | store.items.resized()
        | store.marks.resized()
        | store.props.resized()
        | store.graphics_layers.resized()
        | store.transforms.resized()
        | store.draws.resized()
        | store.texts.resized()
        | store.runs.resized();
    if resized {
        trim_resized(store);
    }
}

#[inline(never)]
fn trim_resized(store: &mut SceneStore) {
    // A record's four rows and its facts grow, clear and give back room
    // together, so the facts speak for the rows' length and the tree row's
    // room for all four.
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
    store.marks.trim_growth_slack();
    store.props.trim_growth_slack();
    store.graphics_layers.trim_growth_slack();
    store.transforms.trim_growth_slack();
    store.draws.trim_growth_slack();
    store.texts.trim_growth_slack();
    store.runs.trim_growth_slack();
}
