//! LazyColumn and LazyRow widget implementations.
//!
//! Provides virtualized scrolling lists that only compose visible items,
//! matching Jetpack Compose's `LazyColumn` and `LazyRow` APIs.

#![allow(non_snake_case)]
#![allow(dead_code)] // Widget API is WIP

use std::rc::Rc;

use crate::modifier::Modifier;
use crate::subcompose_layout::{
    Placement, SubcomposeLayoutNode, SubcomposeLayoutScope,
    SubcomposeMeasureScopeImpl,
};
use crate::widgets::nodes::compose_node;
use compose_core::{NodeId, SlotId};
use compose_foundation::lazy::{
    measure_lazy_list, LazyListIntervalContent, LazyListMeasureConfig, 
    LazyListMeasuredItem, LazyListState,
};
use compose_ui_layout::{Constraints, LinearArrangement, MeasureResult};

// Re-export from foundation - single source of truth
pub use compose_foundation::lazy::{
    LazyListLayoutInfo, LazyListItemInfo,
};

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
        // For small lists, get_index_by_key does full O(n) search
        // For large lists, use NearestRangeState range for O(1) search
        let range = state.nearest_range();
        state.update_scroll_position_if_item_moved(items_count, |key| {
            content.get_index_by_key(key)
                .or_else(|| content.get_index_by_key_in_range(key, range.clone()))
        });
        // Update nearest range for next measurement
        state.update_nearest_range();
    }

    // Collect slot metadata during measurement for later pool update
    // Using a RefCell here to allow the closure to append to the vec
    let measured_slots: std::cell::RefCell<Vec<(u64, Option<u64>, usize)>> = 
        std::cell::RefCell::new(Vec::new());
    
    // Measure function that subcomposes and measures each item
    let measure_item = |index: usize| -> LazyListMeasuredItem {
        let key = content.get_key(index);
        let content_type = content.get_content_type(index);

        // Subcompose the item content with size estimation
        let slot_id = SlotId(key);
        
        // Default estimate: 48px main axis (common list item height), full cross axis
        let default_main_size = 48.0;
        let children = scope.subcompose_with_size(
            slot_id, 
            || { content.invoke_content(index); },
            |_child_idx| {
                if is_vertical {
                    crate::modifier::Size { width: cross_axis_size, height: default_main_size }
                } else {
                    crate::modifier::Size { width: default_main_size, height: cross_axis_size }
                }
            }
        );

        // Subcompose returns the ROOT node(s) that were composed.
        // The first child is typically the root container (e.g., Row) that
        // handles layout of its own children. We only need to place the root.
        let (root_main_size, root_cross_size) = if let Some(root) = children.first() {
            let size = root.size();
            if is_vertical {
                (size.height, size.width)
            } else {
                (size.width, size.height)
            }
        } else {
            (default_main_size, cross_axis_size)
        };

        let mut item = LazyListMeasuredItem::new(
            index,
            key,
            content_type,
            root_main_size,
            root_cross_size,
        );
        
        // Store only the ROOT node ID - it will handle laying out its children
        if let Some(root) = children.first() {
            let node_id = root.node_id() as u64;
            item.node_ids = vec![node_id];
            
            // Collect slot data for later pool update (avoids RefCell conflict)
            measured_slots.borrow_mut().push((key, content_type, node_id as usize));
        }
        
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

    // Now update slot pool with all measured items (after measure_item closure is done)
    {
        let mut pool = state.slot_pool_mut();
        for (key, content_type, node_id) in measured_slots.into_inner() {
            pool.mark_in_use(key, content_type, node_id);
        }
    }

    // Cache measured item sizes for better scroll estimation
    for item in &result.visible_items {
        state.cache_item_size(item.index, item.main_axis_size);
    }

    // Collect visible keys and release non-visible slots back to pool
    let visible_keys: Vec<u64> = result.visible_items
        .iter()
        .map(|item| item.key)
        .collect();
    state.slot_pool_mut().release_non_visible(&visible_keys);

    // Update stats from actual pool counts
    let in_use = state.slot_pool().in_use_count();
    let in_pool = state.slot_pool().available_count();
    state.update_stats(in_use, in_pool);

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
        let prev_first = layout_info.visible_items_info.first().map(|i| i.index).unwrap_or(0);
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
        let prefetch_indices = state.take_prefetch_indices();
        for idx in prefetch_indices {
            if idx < items_count {
                // Subcompose without placing - just to have it ready
                {
                    let key = content.get_key(idx);
                    let content_type_prefetch = content.get_content_type(idx);
                    let slot_id = SlotId(key);
                    let _ = scope.subcompose_with_size(
                        slot_id,
                        || { content.invoke_content(idx); },
                        |_| crate::modifier::Size { 
                            width: cross_axis_size, 
                            height: config.spacing + 48.0 
                        }
                    );
                    // Mark as prefetched in pool
                    // (node will be measured but not placed)
                    state.slot_pool_mut().mark_in_use(key, content_type_prefetch, 0);
                };
            }
        }
    }

    // Create placements from measured items - place only ROOT nodes
    let placements: Vec<Placement> = result
        .visible_items
        .iter()
        .flat_map(|item| {
            item.node_ids.iter().map(move |&nid| {
                let node_id: NodeId = nid as NodeId;
                if is_vertical {
                    Placement::new(node_id, 0.0, item.offset, 0)
                } else {
                    Placement::new(node_id, item.offset, 0.0, 0)
                }
            })
        })
        .collect();

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
    let content_cell = compose_core::remember(|| Rc::new(RefCell::new(LazyListIntervalContent::new())))
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
    compose_node(move || SubcomposeLayoutNode::new(scroll_modifier, policy))
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
    let content_cell = compose_core::remember(|| Rc::new(RefCell::new(LazyListIntervalContent::new())))
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
    compose_node(move || SubcomposeLayoutNode::new(scroll_modifier, policy))
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
