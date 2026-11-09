// The room a store's arrays keep past their entries: an array that doubled
// to take one more record would keep room for as many again, so each update
// ends by giving back room past a quarter of the entries. Free lists keep
// their room, which the next removals take again.

use cranpose_core::collections::growth_slack::GrowthSlack;

use super::SceneStore;

pub(super) fn trim(store: &mut SceneStore) {
    store.tree.trim_growth_slack();
    store.geo.trim_growth_slack();
    store.warm.trim_growth_slack();
    store.generations.trim_growth_slack();
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
