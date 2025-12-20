//! Measured item representation for lazy lists.
//!
//! Contains the result of measuring a single item during lazy layout.

use super::lazy_list_state::LazyListItemInfo;

/// A measured item in a lazy list.
///
/// Contains all the information needed to place the item after measurement.
#[derive(Clone, Debug)]
pub struct LazyListMeasuredItem {
    /// Index in the data source.
    pub index: usize,

    /// Stable key for the item.
    pub key: u64,

    /// Content type for slot reuse.
    pub content_type: Option<u64>,

    /// Size in the main axis (height for vertical, width for horizontal).
    pub main_axis_size: f32,

    /// Size in the cross axis.
    pub cross_axis_size: f32,

    /// Offset from the start of the list content (set during placement).
    pub offset: f32,

    /// Node IDs of the composed item's children (for placing all subcomposed nodes).
    pub node_ids: Vec<u64>,
}

impl LazyListMeasuredItem {
    /// Creates a new measured item.
    pub fn new(
        index: usize,
        key: u64,
        content_type: Option<u64>,
        main_axis_size: f32,
        cross_axis_size: f32,
    ) -> Self {
        Self {
            index,
            key,
            content_type,
            main_axis_size,
            cross_axis_size,
            offset: 0.0,
            node_ids: Vec::new(),
        }
    }

    /// Converts to layout info for external consumption.
    pub fn to_item_info(&self) -> LazyListItemInfo {
        LazyListItemInfo {
            index: self.index,
            key: self.key,
            offset: self.offset,
            size: self.main_axis_size,
        }
    }
}

/// Result of measuring a lazy list.
#[derive(Clone, Debug)]
pub struct LazyListMeasureResult {
    /// Items that were measured and should be placed.
    pub visible_items: Vec<LazyListMeasuredItem>,

    /// Index of the first visible item.
    pub first_visible_item_index: usize,

    /// Scroll offset within the first visible item.
    pub first_visible_item_scroll_offset: f32,

    /// Total size of the viewport in the main axis.
    pub viewport_size: f32,

    /// Total content size (for scroll bounds).
    pub total_content_size: f32,

    /// Whether we can scroll forward.
    pub can_scroll_forward: bool,

    /// Whether we can scroll backward.
    pub can_scroll_backward: bool,
}

impl Default for LazyListMeasureResult {
    fn default() -> Self {
        Self {
            visible_items: Vec::new(),
            first_visible_item_index: 0,
            first_visible_item_scroll_offset: 0.0,
            viewport_size: 0.0,
            total_content_size: 0.0,
            can_scroll_forward: false,
            can_scroll_backward: false,
        }
    }
}
