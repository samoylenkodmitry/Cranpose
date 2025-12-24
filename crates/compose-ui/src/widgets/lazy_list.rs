//! LazyColumn and LazyRow widget implementations.
//!
//! Provides virtualized scrolling lists that only compose visible items,
//! matching Jetpack Compose's `LazyColumn` and `LazyRow` APIs.

#![allow(non_snake_case)]
#![allow(dead_code)] // Widget API is WIP

use std::rc::Rc;

use crate::modifier::Modifier;
use crate::subcompose_layout::{
    Placement, SubcomposeLayoutNode, SubcomposeLayoutScope, SubcomposeMeasureScope,
    SubcomposeMeasureScopeImpl,
};
use crate::widgets::nodes::compose_node;
use compose_core::{NodeId, SlotId};
use compose_foundation::lazy::{
    measure_lazy_list, LazyListIntervalContent, LazyListMeasureConfig, LazyListMeasuredItem,
    LazyListState, SmallNodeVec, SmallOffsetVec, DEFAULT_ITEM_SIZE_ESTIMATE,
};
use smallvec::SmallVec;
use compose_ui_layout::{Constraints, LinearArrangement, MeasureResult, Placeable};

// Re-export from foundation - single source of truth
pub use compose_foundation::lazy::{LazyListItemInfo, LazyListLayoutInfo};

/// Specification for LazyColumn layout behavior.
#[derive(Clone, Debug)]
pub struct LazyColumnSpec {
    /// Vertical arrangement for spacing between items.
    pub vertical_arrangement: LinearArrangement,
    /// Content padding before the first item.
    pub content_padding_top: f32,
    /// Content padding after the last item.
    pub content_padding_bottom: f32,
    /// Number of items to compose beyond the visible bounds.
    /// Higher values reduce jank during fast scrolling but use more memory.
    pub beyond_bounds_item_count: usize,
}

impl Default for LazyColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            content_padding_top: 0.0,
            content_padding_bottom: 0.0,
            beyond_bounds_item_count: 2,
        }
    }
}

impl LazyColumnSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    pub fn content_padding(mut self, top: f32, bottom: f32) -> Self {
        self.content_padding_top = top;
        self.content_padding_bottom = bottom;
        self
    }

    /// Sets uniform content padding for top and bottom.
    pub fn content_padding_all(mut self, padding: f32) -> Self {
        self.content_padding_top = padding;
        self.content_padding_bottom = padding;
        self
    }
}

/// Specification for LazyRow layout behavior.
#[derive(Clone, Debug)]
pub struct LazyRowSpec {
    /// Horizontal arrangement for spacing between items.
    pub horizontal_arrangement: LinearArrangement,
    /// Content padding before the first item.
    pub content_padding_start: f32,
    /// Content padding after the last item.
    pub content_padding_end: f32,
    /// Number of items to compose beyond the visible bounds.
    pub beyond_bounds_item_count: usize,
}

impl Default for LazyRowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            content_padding_start: 0.0,
            content_padding_end: 0.0,
            beyond_bounds_item_count: 2,
        }
    }
}

impl LazyRowSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    pub fn content_padding(mut self, start: f32, end: f32) -> Self {
        self.content_padding_start = start;
        self.content_padding_end = end;
        self
    }

    /// Sets uniform content padding for start and end.
    pub fn content_padding_all(mut self, padding: f32) -> Self {
        self.content_padding_start = padding;
        self.content_padding_end = padding;
        self
    }
}

/// Internal helper to create a lazy list measure policy.
fn measure_lazy_list_internal(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    is_vertical: bool,
    content: &LazyListIntervalContent,
    state: &LazyListState,
    config: &LazyListMeasureConfig,
) -> MeasureResult {
    let viewport_size = if is_vertical {
        constraints.max_height
    } else {
        constraints.max_width
    };
    let cross_axis_size = if is_vertical {
        constraints.max_width
    } else {
        constraints.max_height
    };

    let items_count = content.item_count();
    // Scroll position stability: if items were added/removed before the first visible,
    // find the item by key and adjust scroll position (JC's updateScrollPositionIfTheFirstItemWasMoved)
    if items_count > 0 {
        // For small lists, get_index_by_slot_id does full O(n) search
        // For large lists, use NearestRangeState range for O(1) search
        let range = state.nearest_range();
        state.update_scroll_position_if_item_moved(items_count, |slot_id| {
            content
                .get_index_by_slot_id(slot_id)
                .or_else(|| content.get_index_by_slot_id_in_range(slot_id, range.clone()))
        });
        // Update nearest range for next measurement
        state.update_nearest_range();
    }

    // Measure function that subcomposes and measures each item
    let measure_item = |index: usize| -> LazyListMeasuredItem {
        let key = content.get_key(index);
        let key_slot_id = key.to_slot_id();
        let content_type = content.get_content_type(index);

        // Subcompose the item content with its own slot ID
        // The Composer handles node reuse internally via slot ID matching
        let slot_id = SlotId(key_slot_id);

        // Register content type for policy-based reuse matching
        // If policy is ContentTypeReusePolicy, slots with matching types can reuse each other
        if let Some(ct) = content_type {
            scope.register_content_type(slot_id, ct);
        }

        let children = scope.subcompose(slot_id, || {
            content.invoke_content(index);
        });

        // IMPORTANT: Filter to only ROOT nodes (those without a parent).
        // During subsequent subcomposition passes, the slot table may return ALL nodes
        // in the composition (including children of containers like Row), not just roots.
        // We filter by checking each node's parent status - only parentless nodes are true roots.
        // Use SmallVec to avoid heap allocation for typical items with 1-4 root nodes.
        let root_children: SmallVec<[_; 4]> = children
            .into_iter()
            .filter(|child| scope.node_has_no_parent(child.node_id()))
            .collect();

        // Measure the children using constraints
        // For LazyColumn (vertical): width is constrained (max = cross_axis_size), height is unbounded (INFINITY)
        // For LazyRow (horizontal): height is constrained, width is unbounded
        let child_constraints = if is_vertical {
            Constraints {
                min_width: 0.0,
                max_width: cross_axis_size,
                min_height: 0.0,
                max_height: f32::INFINITY,
            }
        } else {
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: cross_axis_size,
            }
        };

        // Measure only ROOT nodes returned by subcompose.
        // Each root node handles its children's layout internally - we should NOT
        // iterate over all descendants or they'll be placed separately in the list.
        // 
        // subcompose() returns only direct children (root nodes) of the slot composition.
        // If you compose `Row { Text(...); Text(...); }`, subcompose returns just [Row],
        // not [Row, Text, Text]. The Row measures its own children during its layout.
        let mut total_main_size: f32 = 0.0;
        let mut max_cross_size: f32 = 0.0;
        let mut node_ids: SmallNodeVec = SmallVec::new();
        let mut child_offsets: SmallOffsetVec = SmallVec::new();

        // Measure each ROOT node (typically just one per item)
        for child in root_children {
            let placeable = scope.measure(child, child_constraints);
            let (main, cross) = if is_vertical {
                (placeable.height(), placeable.width())
            } else {
                (placeable.width(), placeable.height())
            };

            // Track offset of this root node within the item
            child_offsets.push(total_main_size);
            node_ids.push(child.node_id() as u64);

            total_main_size += main;
            max_cross_size = max_cross_size.max(cross);
        }

        let mut item = LazyListMeasuredItem::new(
            index,
            key_slot_id,
            content_type,
            total_main_size,
            max_cross_size,
        );

        item.node_ids = node_ids;
        item.child_offsets = child_offsets;

        item
    };

    // Run the lazy list measurement algorithm
    let result = measure_lazy_list(
        items_count,
        state,
        viewport_size,
        cross_axis_size,
        config,
        measure_item,
    );

    // Cache measured item sizes for better scroll estimation
    for item in &result.visible_items {
        state.cache_item_size(item.index, item.main_axis_size);
    }

    // Update stats: count only items WITHIN viewport, not beyond-bounds buffer
    let truly_visible_count = result
        .visible_items
        .iter()
        .filter(|item| {
            // Item is visible if any part of it is within viewport bounds
            let item_end = item.offset + item.main_axis_size;
            item.offset < viewport_size && item_end > 0.0
        })
        .count();
    // Get reusable slot count from SubcomposeState (the single source of truth)
    let in_pool = scope.reusable_slots_count();
    state.update_stats(truly_visible_count, in_pool);

    // Prefetching: pre-compose items before they become visible
    // 1. Record scroll direction from consumed delta
    let layout_info = state.layout_info();
    // Approximate direction: compare current first visible with stored
    // We use the delta that was consumed (negative = scroll down gesture = forward scroll)
    // For now, derive from result - if first visible != previous, we scrolled

    // 2. Update prefetch queue based on visible items
    if !result.visible_items.is_empty() {
        let first_visible = result.visible_items.first().map(|i| i.index).unwrap_or(0);
        let last_visible = result.visible_items.last().map(|i| i.index).unwrap_or(0);

        // Infer direction from comparison to previous first visible
        let prev_first = layout_info
            .visible_items_info
            .first()
            .map(|i| i.index)
            .unwrap_or(0);
        let direction = if first_visible > prev_first {
            1.0 // Forward
        } else if first_visible < prev_first {
            -1.0 // Backward
        } else {
            0.0 // No change
        };
        state.record_scroll_direction(direction);

        state.update_prefetch_queue(first_visible, last_visible, items_count);

        // 3. Pre-compose prefetched items (compose but don't place)
        // Uses cached item sizes when available for better size estimation
        let prefetch_indices = state.take_prefetch_indices();
        let average_size = state.average_item_size();
        for idx in prefetch_indices {
            if idx < items_count {
                // Subcompose without placing - just to have it ready
                // SubcomposeState automatically tracks these as precomposed
                let key = content.get_key(idx);
                let key_slot_id = key.to_slot_id();
                let content_type_prefetch = content.get_content_type(idx);
                let slot_id = SlotId(key_slot_id);

                // Register content type for prefetched items too
                if let Some(ct) = content_type_prefetch {
                    scope.register_content_type(slot_id, ct);
                }

                // Use cached size if available, otherwise use average
                let estimated_size = state
                    .get_cached_size(idx)
                    .unwrap_or(average_size.max(DEFAULT_ITEM_SIZE_ESTIMATE));

                let prefetch_idx = idx;
                let _ = scope.subcompose_with_size(
                    slot_id,
                    || {
                        content.invoke_content(prefetch_idx);
                    },
                    move |_| crate::modifier::Size {
                        width: cross_axis_size,
                        height: estimated_size + config.spacing,
                    },
                );
            }
        }
    }

    // Create placements from measured items - place only ROOT nodes
    //
    // JC Pattern (LazyListMeasure.kt:calculateItemsOffsets):
    // When all items fit in viewport (hasSpareSpace), apply the arrangement.
    // Otherwise, use sequential positioning from measurement.
    let arrangement = if is_vertical {
        config
            .vertical_arrangement
            .unwrap_or(LinearArrangement::Start)
    } else {
        config
            .horizontal_arrangement
            .unwrap_or(LinearArrangement::Start)
    };

    // Check if we should apply arrangement:
    // 1. All items are visible (visible_items.len() == total items)
    // 2. Content is smaller than viewport (hasSpareSpace)
    // 3. Arrangement is not sequential (Start or SpacedBy)
    let total_item_size: f32 = result.visible_items.iter().map(|i| i.main_axis_size).sum();
    let has_spare_space =
        total_item_size < viewport_size && result.visible_items.len() == items_count;
    let should_apply_arrangement = has_spare_space
        && !matches!(
            arrangement,
            LinearArrangement::Start | LinearArrangement::SpacedBy(_)
        );

    let placements: Vec<Placement> = if should_apply_arrangement {
        // Apply arrangement to compute final positions
        // JC: density.arrange(mainAxisLayoutSize, sizes, offsets)
        use compose_ui_layout::Arrangement; // Import the trait

        let sizes: Vec<f32> = result
            .visible_items
            .iter()
            .map(|i| i.main_axis_size)
            .collect();
        let mut positions = vec![0.0; sizes.len()];
        arrangement.arrange(viewport_size, &sizes, &mut positions);

        result
            .visible_items
            .iter()
            .zip(positions.iter())
            .flat_map(|(item, &pos)| {
                item.node_ids.iter().zip(item.child_offsets.iter()).map(
                    move |(&nid, &child_offset)| {
                        let node_id: NodeId = nid as NodeId;
                        if is_vertical {
                            Placement::new(node_id, 0.0, pos + child_offset, 0)
                        } else {
                            Placement::new(node_id, pos + child_offset, 0.0, 0)
                        }
                    },
                )
            })
            .collect()
    } else {
        // Use sequential offsets from measurement (scrolling case)
        result
            .visible_items
            .iter()
            .flat_map(|item| {
                item.node_ids.iter().zip(item.child_offsets.iter()).map(
                    move |(&nid, &child_offset)| {
                        let node_id: NodeId = nid as NodeId;
                        if is_vertical {
                            Placement::new(node_id, 0.0, item.offset + child_offset, 0)
                        } else {
                            Placement::new(node_id, item.offset + child_offset, 0.0, 0)
                        }
                    },
                )
            })
            .collect()
    };

    let width = if is_vertical {
        cross_axis_size
    } else {
        result.total_content_size
    };
    let height = if is_vertical {
        result.total_content_size
    } else {
        cross_axis_size
    };

    scope.layout(width, height, placements)
}

fn get_spacing(arrangement: LinearArrangement) -> f32 {
    match arrangement {
        LinearArrangement::SpacedBy(spacing) => spacing,
        _ => 0.0,
    }
}

/// A vertically scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyColumn` API.
///
/// # Arguments
/// * `modifier` - Layout modifiers
/// * `state` - LazyListState for scroll position tracking (from compose-foundation)
/// * `scroll_state` - ScrollState for gesture integration (from compose-ui)
/// * `spec` - Layout configuration
/// * `content` - Item content builder
///
/// # Example
///
/// ```rust,ignore
/// use compose_ui::scroll::ScrollState;
/// use compose_foundation::lazy::{LazyListState, LazyListIntervalContent, LazyListScope};
/// use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
///
/// let state = LazyListState::new();
/// let scroll_state = ScrollState::new(0.0);
/// let mut content = LazyListIntervalContent::new();
/// content.items(100, None::<fn(usize)->u64>, None::<fn(usize)->u64>, |i| {
///     // Compose your item content here
/// });
/// LazyColumn(Modifier::empty(), state, scroll_state, LazyColumnSpec::default(), content);
/// ```
pub fn LazyColumn(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyColumnSpec,
    content: LazyListIntervalContent,
) -> NodeId {
    use std::cell::RefCell;

    // Use remember to keep a shared RefCell for content that persists across recompositions
    // This allows updating the content on each recomposition while reusing the same node/policy
    let content_cell =
        compose_core::remember(|| Rc::new(RefCell::new(LazyListIntervalContent::new())))
            .with(|cell| cell.clone());

    // Update the content on each recomposition
    *content_cell.borrow_mut() = content;

    // Configure measurement
    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: false,
        before_content_padding: spec.content_padding_top,
        after_content_padding: spec.content_padding_bottom,
        spacing: get_spacing(spec.vertical_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: Some(spec.vertical_arrangement),
        horizontal_arrangement: None,
    };

    // Create measure policy that reads from the shared RefCell
    let state_clone = state.clone();
    let content_for_policy = content_cell.clone();
    let policy = Rc::new(
        move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
            let content_ref = content_for_policy.borrow();
            measure_lazy_list_internal(
                scope,
                constraints,
                true,
                &content_ref,
                &state_clone,
                &config,
            )
        },
    );

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier.clip_to_bounds().lazy_vertical_scroll(state);

    // Create and register the subcompose layout node with the composer
    compose_node(move || SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy))
}

/// A horizontally scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyRow` API.
pub fn LazyRow(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyRowSpec,
    content: LazyListIntervalContent,
) -> NodeId {
    use std::cell::RefCell;

    // Use remember to keep a shared RefCell for content that persists across recompositions
    let content_cell =
        compose_core::remember(|| Rc::new(RefCell::new(LazyListIntervalContent::new())))
            .with(|cell| cell.clone());

    // Update the content on each recomposition
    *content_cell.borrow_mut() = content;

    let config = LazyListMeasureConfig {
        is_vertical: false,
        reverse_layout: false,
        before_content_padding: spec.content_padding_start,
        after_content_padding: spec.content_padding_end,
        spacing: get_spacing(spec.horizontal_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: None,
        horizontal_arrangement: Some(spec.horizontal_arrangement),
    };

    let state_clone = state.clone();
    let content_for_policy = content_cell.clone();
    let policy = Rc::new(
        move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
            let content_ref = content_for_policy.borrow();
            measure_lazy_list_internal(
                scope,
                constraints,
                false,
                &content_ref,
                &state_clone,
                &config,
            )
        },
    );

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier.clip_to_bounds().lazy_horizontal_scroll(state);

    // Create and register the subcompose layout node with the composer
    compose_node(move || SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lazy_column_spec_default() {
        let spec = LazyColumnSpec::default();
        assert_eq!(spec.vertical_arrangement, LinearArrangement::Start);
    }

    #[test]
    fn test_lazy_column_spec_builder() {
        let spec = LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(8.0))
            .content_padding(16.0, 16.0);

        assert_eq!(spec.vertical_arrangement, LinearArrangement::SpacedBy(8.0));
        assert_eq!(spec.content_padding_top, 16.0);
    }

    #[test]
    fn test_lazy_row_spec_default() {
        let spec = LazyRowSpec::default();
        assert_eq!(spec.horizontal_arrangement, LinearArrangement::Start);
    }

    #[test]
    fn test_get_spacing() {
        assert_eq!(get_spacing(LinearArrangement::Start), 0.0);
        assert_eq!(get_spacing(LinearArrangement::SpacedBy(12.0)), 12.0);
    }

    #[test]
    fn test_content_padding_all() {
        let spec = LazyColumnSpec::new().content_padding_all(24.0);
        assert_eq!(spec.content_padding_top, 24.0);
        assert_eq!(spec.content_padding_bottom, 24.0);
    }
}
