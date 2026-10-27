use cranpose_foundation::lazy::{LazyListState, LazySettlePolicy};
use cranpose_ui_layout::Axis;

/// What a screen reader learns about a lazy list. The list has no whole
/// extent to give, so the first visible item stands in for the position, a
/// half step marks a list scrolled part way into that item, and one more step
/// stays ahead while the list can still scroll forward. A reader needs only
/// the two answers: can it page back, can it page on.
pub(crate) fn lazy_scroll_semantics(
    state: LazyListState,
    axis: Axis,
    reverse_scrolling: bool,
    content_padding: (f32, f32),
) -> impl Fn(&mut cranpose_foundation::SemanticsConfiguration) + 'static {
    move |config| {
        let mut value = state.first_visible_item_index() as f32;
        if state.first_visible_item_scroll_offset() > 0.0 {
            value += 0.5;
        }
        let max_value = if state.can_scroll_forward() {
            value + 1.0
        } else {
            value
        };
        let range = cranpose_foundation::ScrollAxisRange::new(value, max_value, reverse_scrolling)
            .with_content_padding(content_padding.0, content_padding.1);
        let count = state.total_items_count();
        config.collection = Some(cranpose_foundation::CollectionInfo {
            rows: if axis.is_vertical() { count } else { 1 },
            columns: if axis.is_vertical() { 1 } else { count },
        });
        config
            .role
            .get_or_insert(cranpose_foundation::SemanticsWidgetRole::List);
        if axis.is_vertical() {
            config.vertical_scroll = Some(range);
        } else {
            config.horizontal_scroll = Some(range);
        }
        config.scroll_by = Some(cranpose_foundation::SemanticsScrollBy::new(
            move |dx, dy| {
                let delta = if axis.is_vertical() { dy } else { dx };
                match state.settle_policy() {
                    Some(policy) => settle_reader_scroll(&state, &policy, delta),
                    None => state.dispatch_scroll_delta(-delta).abs() > f32::EPSILON,
                }
            },
        ));
        config.scroll_to_index = Some(cranpose_foundation::SemanticsScrollToIndex::new(
            move |index| jump_to_row(&state, index),
        ));
    }
}

/// Moves a list whose scroll comes to rest by a settle policy `delta` along
/// its items, and on to where the policy rests the item that reaches the
/// start, as a release there would. A reader's page of a pager turns one
/// page.
fn settle_reader_scroll(state: &LazyListState, policy: &LazySettlePolicy, delta: f32) -> bool {
    let can_move = if delta > 0.0 {
        state.can_scroll_forward_non_reactive()
    } else {
        state.can_scroll_backward_non_reactive()
    };
    let size = state.average_item_size();
    if !can_move || size <= 0.0 {
        return false;
    }
    let first = state.first_visible_item_index_non_reactive() as f32;
    let reached =
        (first * size + state.first_visible_item_scroll_offset_non_reactive() + delta).max(0.0);
    let last = state.total_items_count().saturating_sub(1) as f32;
    let index = (reached / size).floor().min(last);
    let target = policy(reached - index * size, 0.0);
    crate::AnimateScroll::animate_scroll_to_item(state, index as usize, target);
    true
}

/// Puts the row at `index` at the top of a lazy list for a screen reader that
/// asked for it. A list with no rows, or an index past the last row, takes
/// nothing and says so.
fn jump_to_row(state: &LazyListState, index: usize) -> bool {
    if index >= state.total_items_count() {
        return false;
    }
    state.scroll_to_item(index, 0.0);
    true
}
