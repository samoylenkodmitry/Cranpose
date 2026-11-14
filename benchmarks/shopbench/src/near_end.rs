// Infinite scroll: asks for the next page once the list scrolls within one
// screen height of its end, and again after a page that left it there.

use cranpose::{LaunchedEffect, LazyListLayoutInfo, LazyListState, composable};

#[composable]
pub(crate) fn LoadMoreNearEnd(state: LazyListState, load_more: impl Fn() + 'static) {
    // Reading the first visible item subscribes this to the list's scrolling.
    let _ = state.first_visible_item_index();
    let (near, count) = state
        .with_layout_info(|info| (near_end(info), info.total_items_count))
        .unwrap_or((false, 0));
    LaunchedEffect((near, count), async move |_| {
        if near {
            load_more();
        }
    });
}

fn near_end(info: &LazyListLayoutInfo) -> bool {
    let Some(last) = info.visible_items_info.last() else {
        return false;
    };
    let shown = info.visible_items_info.len() as f32;
    let per_item = info
        .visible_items_info
        .iter()
        .map(|item| item.size)
        .sum::<f32>()
        / shown
        + info.main_axis_item_spacing;
    let unseen = info.total_items_count.saturating_sub(last.index + 1) as f32;
    let below = (last.offset + last.size - info.viewport_end_offset).max(0.0) + unseen * per_item;
    below < info.viewport_size
}
