use cranpose_foundation::lazy::LazyListMeasuredItem;
use cranpose_ui_layout::LinearArrangement;
use smallvec::SmallVec;

use super::{LazyFrame, LazyListLayoutConfig};
use crate::subcompose_layout::Placement;

pub(super) fn push_lazy_list_placements(
    placements: &mut Vec<Placement>,
    density: f32,
    visible_items: &[LazyListMeasuredItem],
    items_count: usize,
    frame: &LazyFrame,
    viewport_size: f32,
    layout_config: &LazyListLayoutConfig,
) {
    use cranpose_ui_layout::Arrangement;

    let axis = frame.axis;
    let config = &layout_config.measure;

    placements.clear();
    placements.reserve(
        visible_items
            .iter()
            .map(|item| item.roots.node_ids.len())
            .sum(),
    );

    let arrangement = if axis.is_vertical() {
        config
            .vertical_arrangement
            .unwrap_or(LinearArrangement::Start)
    } else {
        config
            .horizontal_arrangement
            .unwrap_or(LinearArrangement::Start)
    };

    let spacing = config.spacing;
    let total_item_size: f32 = visible_items.iter().map(|i| i.main_axis_size).sum::<f32>()
        + (items_count.saturating_sub(1) as f32) * spacing;
    let available_main_axis =
        (viewport_size - config.before_content_padding - config.after_content_padding).max(0.0);
    let has_spare_space =
        total_item_size < available_main_axis && visible_items.len() == items_count;
    let should_apply_arrangement = has_spare_space
        && !matches!(
            arrangement,
            LinearArrangement::Start | LinearArrangement::SpacedBy(_)
        );

    if should_apply_arrangement {
        let content_offset = config.before_content_padding;

        let sizes: SmallVec<[f32; 32]> = visible_items.iter().map(|i| i.main_axis_size).collect();
        let mut positions: SmallVec<[f32; 32]> = SmallVec::from_elem(0.0, sizes.len());
        arrangement.arrange(density, available_main_axis, &sizes, &mut positions);

        for (item, &pos) in visible_items.iter().zip(positions.iter()) {
            let offset = content_offset + pos;
            push_lazy_item_placements(
                placements,
                item,
                offset,
                frame,
                viewport_size,
                layout_config,
            );
        }
    } else {
        for item in visible_items {
            push_lazy_item_placements(
                placements,
                item,
                item.offset,
                frame,
                viewport_size,
                layout_config,
            );
        }
    }
}

fn push_lazy_item_placements(
    placements: &mut Vec<Placement>,
    item: &LazyListMeasuredItem,
    offset: f32,
    frame: &LazyFrame,
    viewport_size: f32,
    layout_config: &LazyListLayoutConfig,
) {
    let (axis, cross_axis_size) = (frame.axis, frame.cross_axis_size);
    let config = &layout_config.measure;
    let (cross_start, cross_end) = layout_config.cross_padding;
    let start = if config.reverse_layout {
        viewport_size - offset - item.main_axis_size
    } else {
        offset
    };
    for (index, (&node_id, &offset)) in item
        .roots
        .node_ids
        .iter()
        .zip(&item.roots.child_offsets)
        .enumerate()
    {
        let main = start + offset;
        let cell = item.child_cross_offsets.get(index).copied().unwrap_or(0.0);
        let (mut x, y) = if axis.is_vertical() {
            (cross_start + cell, main)
        } else {
            (main, cross_start + cell)
        };
        if config.is_rtl {
            x = if axis.is_vertical() {
                cross_axis_size
                    - cross_end
                    - cell
                    - item
                        .roots
                        .child_cross_axis_sizes
                        .get(index)
                        .copied()
                        .unwrap_or(item.cross_axis_size)
            } else {
                let extent = item
                    .child_main_sizes
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| {
                        item.roots
                            .child_offsets
                            .get(index + 1)
                            .copied()
                            .unwrap_or(item.main_axis_size)
                            - offset
                    });
                viewport_size - x - extent
            };
        }
        placements.push(Placement::new(node_id, x, y, 0.0));
    }
}
