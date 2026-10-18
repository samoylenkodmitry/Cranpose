//! Measured item representation for lazy lists.
//!
//! Contains the result of measuring a single item during lazy layout.

use std::rc::Rc;

use smallvec::SmallVec;

use super::lazy_list_state::LazyListItemInfo;

/// Inline capacity for node_ids and child_offsets.
/// Most lazy list items have 1-2 root nodes, so 4 avoids heap allocation
/// in the common case while keeping stack size reasonable.
pub type SmallNodeVec = SmallVec<[u64; 4]>;
/// Inline list of layout-pixel values, one per root node of an item, with the capacity of
/// [`SmallNodeVec`].
pub type SmallOffsetVec = SmallVec<[f32; 4]>;

/// A measured item in a lazy list.
///
/// Contains all the information needed to place the item after measurement.
#[derive(Debug)]
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

    /// What measuring the item's roots found.
    pub roots: Rc<LazyItemRoots>,

    /// Cross-axis offset of each root in the order of its node ids, for the
    /// cells of a grid line and the lane of a staggered grid item; empty for
    /// a list item, whose roots start at the cross-axis start.
    pub child_cross_offsets: SmallOffsetVec,
    /// Main-axis extent of each root in the order of its node ids, for the
    /// cells of a grid line, which overlap along the main axis; empty for a
    /// list item, whose roots follow one another.
    pub child_main_sizes: SmallOffsetVec,
}

/// What measuring the roots of an item found, in the order of its roots.
/// Every copy of the item a measure pass hands around shares it with the
/// cache the item came from, so a copy costs a reference count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LazyItemRoots {
    /// Node IDs of the composed item's roots, for placing them.
    pub node_ids: SmallNodeVec,
    /// Offset of each root within the item, along the main axis.
    pub child_offsets: SmallOffsetVec,
    /// Measured cross-axis extent of each root.
    pub child_cross_axis_sizes: SmallOffsetVec,
}

// A copy copies the placement offsets as slices: a list item's are empty,
// and the derived clone of an empty `SmallVec` still walks an iterator.
impl Clone for LazyListMeasuredItem {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            key: self.key,
            content_type: self.content_type,
            main_axis_size: self.main_axis_size,
            cross_axis_size: self.cross_axis_size,
            offset: self.offset,
            roots: Rc::clone(&self.roots),
            child_cross_offsets: copy_offsets(&self.child_cross_offsets),
            child_main_sizes: copy_offsets(&self.child_main_sizes),
        }
    }
}

/// A copy of `offsets`. A list item's are empty, and copying an empty slice
/// still calls `memcpy`.
fn copy_offsets(offsets: &SmallOffsetVec) -> SmallOffsetVec {
    if offsets.is_empty() {
        SmallOffsetVec::new()
    } else {
        SmallVec::from_slice(offsets)
    }
}

impl LazyListMeasuredItem {
    /// Creates a measured item with the measured `roots`.
    pub fn new(
        index: usize,
        key: u64,
        content_type: Option<u64>,
        main_axis_size: f32,
        cross_axis_size: f32,
        roots: Rc<LazyItemRoots>,
    ) -> Self {
        Self {
            index,
            key,
            content_type,
            main_axis_size,
            cross_axis_size,
            offset: 0.0,
            roots,
            child_cross_offsets: SmallVec::new(),
            child_main_sizes: SmallVec::new(),
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
