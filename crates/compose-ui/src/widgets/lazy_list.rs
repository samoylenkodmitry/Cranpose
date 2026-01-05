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
use compose_ui_layout::{Constraints, LinearArrangement, MeasureResult, Placeable};
use smallvec::SmallVec;

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
    /// Whether to reverse the layout direction (bottom-to-top).
    pub reverse_layout: bool,
}

impl Default for LazyColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            content_padding_top: 0.0,
            content_padding_bottom: 0.0,
            beyond_bounds_item_count: 2,
            reverse_layout: false,
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

    pub fn reverse_layout(mut self, reverse: bool) -> Self {
        self.reverse_layout = reverse;
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
    /// Whether to reverse the layout direction (end-to-start).
    pub reverse_layout: bool,
}

impl Default for LazyRowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            content_padding_start: 0.0,
            content_padding_end: 0.0,
            beyond_bounds_item_count: 2,
            reverse_layout: false,
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

    pub fn reverse_layout(mut self, reverse: bool) -> Self {
        self.reverse_layout = reverse;
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
        // Scroll position stability: try O(1) range search first, fall back to O(N) global search
        // This matches the performance-optimal pattern: most items are found within the range
        let range = state.nearest_range();
        state.update_scroll_position_if_item_moved(items_count, |slot_id| {
            content
                .get_index_by_slot_id_in_range(slot_id, range.clone())
                .or_else(|| content.get_index_by_slot_id(slot_id))
        });
        // Note: nearest range is automatically updated by scroll_position when index changes
    }

    // Measure function that subcomposes and measures each item
    let measure_item = |index: usize| -> LazyListMeasuredItem {
        let key = content.get_key(index);
        let key_slot_id = key.to_slot_id();
        let content_type = content.get_content_type(index);

        // Subcompose the item content with its own slot ID
        // The Composer handles node reuse internally via slot ID matching
        let slot_id = SlotId(key_slot_id);

        // Update content type for policy-based reuse matching
        // Uses update_content_type to handle both Some and None cases,
        // ensuring stale types don't drive incorrect reuse after transitions
        scope.update_content_type(slot_id, content_type);

        let children = scope.subcompose(slot_id, || {
            content.invoke_content(index);
        });

        // Record composition statistics for diagnostics
        let was_reused = scope.was_last_slot_reused().unwrap_or(false);
        state.record_composition(was_reused);

        // NOTE: Composer::subcompose_measurement now returns only root nodes directly,
        // so we no longer need the O(N) filter here.
        let root_children = children;

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

    // Capture scroll delta for direction inference BEFORE measurement consumes it.
    // This is more accurate than comparing first visible index, especially for:
    // - Scrolling within the same item (partial scroll)
    // - Variable height items where scroll offset changes without index change
    let scroll_delta_for_direction = state.peek_scroll_delta();

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
    // Direction is inferred from actual scroll delta (more accurate than index comparison)

    // Update prefetch queue based on visible items
    if !result.visible_items.is_empty() {
        let first_visible = result.visible_items.first().map(|i| i.index).unwrap_or(0);
        let last_visible = result.visible_items.last().map(|i| i.index).unwrap_or(0);

        // Use actual scroll delta for direction (more accurate than index comparison)
        // Positive delta = scrolling forward (content moving up/left)
        // Negative delta = scrolling backward (content moving down/right)
        state.record_scroll_direction(scroll_delta_for_direction);

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

                // Update content type for prefetched items too
                scope.update_content_type(slot_id, content_type_prefetch);

                // Use cached size if available, otherwise use average
                let estimated_size = state
                    .get_cached_size(idx)
                    .unwrap_or(average_size.max(DEFAULT_ITEM_SIZE_ESTIMATE));

                let prefetch_idx = idx;
                let is_vertical = config.is_vertical;
                let _ = scope.subcompose_with_size(
                    slot_id,
                    || {
                        content.invoke_content(prefetch_idx);
                    },
                    move |_| {
                        // Use correct axis based on orientation:
                        // Vertical list: width = cross_axis, height = main_axis (estimated)
                        // Horizontal list: width = main_axis (estimated), height = cross_axis
                        if is_vertical {
                            crate::modifier::Size {
                                width: cross_axis_size,
                                height: estimated_size + config.spacing,
                            }
                        } else {
                            crate::modifier::Size {
                                width: estimated_size + config.spacing,
                                height: cross_axis_size,
                            }
                        }
                    },
                );
            }
        }
    }

    // Create placements from measured items - place only ROOT nodes
    // JC Pattern (LazyListMeasure.kt:calculateItemsOffsets)
    let placements = create_lazy_list_placements(
        &result.visible_items,
        items_count,
        is_vertical,
        viewport_size,
        config,
    );

    // Report size that respects BOTH min and max constraints.
    // - If content < min: expand to min (e.g., fillMaxSize)
    // - If content > max: clamp to max (enables scrolling)
    // - Otherwise: use content size (shrink-wrap)
    let width = if is_vertical {
        cross_axis_size
    } else {
        result
            .total_content_size
            .clamp(constraints.min_width, constraints.max_width)
    };
    let height = if is_vertical {
        result
            .total_content_size
            .clamp(constraints.min_height, constraints.max_height)
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

/// Creates placements for measured lazy list items.
///
/// This helper encapsulates the logic for:
/// - Applying arrangement when all items fit (hasSpareSpace in JC)
/// - Using sequential positioning during scrolling
fn create_lazy_list_placements(
    visible_items: &[LazyListMeasuredItem],
    items_count: usize,
    is_vertical: bool,
    viewport_size: f32,
    config: &LazyListMeasureConfig,
) -> Vec<Placement> {
    use compose_ui_layout::Arrangement;

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
    let spacing = get_spacing(arrangement);
    let total_item_size: f32 = visible_items.iter().map(|i| i.main_axis_size).sum::<f32>()
        + (items_count.saturating_sub(1) as f32) * spacing;
    // Account for content padding when checking spare space (JC pattern)
    // Clamp to 0.0 to handle edge case where padding exceeds viewport
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
        // Apply arrangement to compute final positions
        // JC: density.arrange(mainAxisLayoutSize, sizes, offsets)
        let content_offset = config.before_content_padding;

        let sizes: Vec<f32> = visible_items.iter().map(|i| i.main_axis_size).collect();
        let mut positions = vec![0.0; sizes.len()];
        arrangement.arrange(available_main_axis, &sizes, &mut positions);

        visible_items
            .iter()
            .zip(positions.iter())
            .flat_map(|(item, &pos)| {
                item.node_ids.iter().zip(item.child_offsets.iter()).map(
                    move |(&nid, &child_offset)| {
                        let node_id: NodeId = nid as NodeId;
                        let item_size = item.main_axis_size;

                        if is_vertical {
                            let y = if config.reverse_layout {
                                viewport_size - (content_offset + pos) - item_size + child_offset
                            } else {
                                content_offset + pos + child_offset
                            };
                            Placement::new(node_id, 0.0, y, 0)
                        } else {
                            let x = if config.reverse_layout {
                                viewport_size - (content_offset + pos) - item_size + child_offset
                            } else {
                                content_offset + pos + child_offset
                            };
                            Placement::new(node_id, x, 0.0, 0)
                        }
                    },
                )
            })
            .collect()
    } else {
        // Use sequential offsets from measurement (scrolling case)
        visible_items
            .iter()
            .flat_map(|item| {
                item.node_ids.iter().zip(item.child_offsets.iter()).map(
                    move |(&nid, &child_offset)| {
                        let node_id: NodeId = nid as NodeId;
                        let item_size = item.main_axis_size;

                        if is_vertical {
                            let y = if config.reverse_layout {
                                viewport_size - item.offset - item_size + child_offset
                            } else {
                                item.offset + child_offset
                            };
                            Placement::new(node_id, 0.0, y, 0)
                        } else {
                            let x = if config.reverse_layout {
                                viewport_size - item.offset - item_size + child_offset
                            } else {
                                item.offset + child_offset
                            };
                            Placement::new(node_id, x, 0.0, 0)
                        }
                    },
                )
            })
            .collect()
    }
}

/// Internal implementation for LazyColumn that takes pre-built content.
///
/// Users should prefer the DSL-based [`LazyColumn`] function instead.
fn LazyColumnImpl(
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

    // Configure measurement - wrapped in rememberUpdatedState for stable reference
    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: spec.reverse_layout,
        before_content_padding: spec.content_padding_top,
        after_content_padding: spec.content_padding_bottom,
        spacing: get_spacing(spec.vertical_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: Some(spec.vertical_arrangement),
        horizontal_arrangement: None,
    };
    let config_state = compose_core::rememberUpdatedState(config);

    // Create measure policy with stable identity using remember.
    // The policy reads latest values via state references, so it can be memoized.
    let content_for_policy = content_cell.clone();
    let policy = compose_core::remember(move || {
        let cfg = config_state;
        let content_ref = content_for_policy.clone();
        let state_ref = state;
        Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = cfg.value();
                measure_lazy_list_internal(scope, constraints, true, &content, &state_ref, &config)
            },
        )
    })
    .with(|p| p.clone());

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier
        .clip_to_bounds()
        .lazy_vertical_scroll(state, spec.reverse_layout);

    // Create and register the subcompose layout node with the composer
    let node_id = compose_node(move || {
        SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy)
    });

    // Register layout invalidation callback with the actual node ID.
    // This uses schedule_layout_repass (O(subtree)) instead of request_layout_invalidation (O(app)).
    // The callback is used for scroll_to_item() and similar programmatic scrolls.
    // Note: try_register_layout_callback prevents duplicate registrations on recomposition.
    state.try_register_layout_callback(Rc::new(move || {
        crate::schedule_layout_repass(node_id);
    }));

    node_id
}

/// Internal implementation for LazyRow that takes pre-built content.
///
/// Users should prefer the DSL-based [`LazyRow`] function instead.
fn LazyRowImpl(
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
        reverse_layout: spec.reverse_layout,
        before_content_padding: spec.content_padding_start,
        after_content_padding: spec.content_padding_end,
        spacing: get_spacing(spec.horizontal_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: None,
        horizontal_arrangement: Some(spec.horizontal_arrangement),
    };
    let config_state = compose_core::rememberUpdatedState(config);

    // Create measure policy with stable identity using remember.
    let content_for_policy = content_cell.clone();
    let policy = compose_core::remember(move || {
        let cfg = config_state;
        let content_ref = content_for_policy.clone();
        let state_ref = state;
        Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = cfg.value();
                measure_lazy_list_internal(scope, constraints, false, &content, &state_ref, &config)
            },
        )
    })
    .with(|p| p.clone());

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier
        .clip_to_bounds()
        .lazy_horizontal_scroll(state, spec.reverse_layout);

    // Create and register the subcompose layout node with the composer
    let node_id = compose_node(move || {
        SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy)
    });

    // Register layout invalidation callback with the actual node ID.
    // This uses schedule_layout_repass (O(subtree)) instead of request_layout_invalidation (O(app)).
    // Note: try_register_layout_callback prevents duplicate registrations on recomposition.
    state.try_register_layout_callback(Rc::new(move || {
        crate::schedule_layout_repass(node_id);
    }));

    node_id
}

/// A vertically scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyColumn` API. The closure receives
/// a [`LazyListIntervalContent`] which implements [`LazyListScope`] for defining items.
///
/// # Example
///
/// ```rust,ignore
/// let state = remember_lazy_list_state();
/// LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
///     // Single header item
///     scope.item(Some(0), None, || {
///         Text("Header", Modifier::empty());
///     });
///
///     // Multiple items from data
///     scope.items(data.len(), Some(|i| data[i].id), None, |i| {
///         Text(data[i].name.clone(), Modifier::empty());
///     });
/// });
/// ```
///
/// For convenience with slices, use the [`LazyListScopeExt`] extension methods:
///
/// ```rust,ignore
/// use compose_foundation::lazy::LazyListScopeExt;
///
/// LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
///     scope.items_slice(&my_data, |item| {
///         Text(item.name.clone(), Modifier::empty());
///     });
/// });
/// ```
pub fn LazyColumn<F>(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyColumnSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let mut interval_content = LazyListIntervalContent::new();
    content(&mut interval_content);
    LazyColumnImpl(modifier, state, spec, interval_content)
}

/// A horizontally scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyRow` API. The closure receives
/// a [`LazyListIntervalContent`] which implements [`LazyListScope`] for defining items.
///
/// # Example
///
/// ```rust,ignore
/// let state = remember_lazy_list_state();
/// LazyRow(Modifier::empty(), state, LazyRowSpec::default(), |scope| {
///     scope.items(10, None::<fn(usize)->u64>, None::<fn(usize)->u64>, |i| {
///         Text(format!("Item {}", i), Modifier::empty());
///     });
/// });
/// ```
pub fn LazyRow<F>(modifier: Modifier, state: LazyListState, spec: LazyRowSpec, content: F) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let mut interval_content = LazyListIntervalContent::new();
    content(&mut interval_content);
    LazyRowImpl(modifier, state, spec, interval_content)
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
