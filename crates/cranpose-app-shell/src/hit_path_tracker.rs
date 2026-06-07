//! Hit path tracking for pointer input capture.
//!
//! This module implements a Jetpack Compose-style `HitPathTracker` that stores
//! stable `NodeId` capture paths instead of caching `HitRegion` geometry.
//!
//! Key insight from JC: cache **node identity**, not geometry. Fresh geometry
//! is resolved from the current scene on each dispatch, avoiding stale coordinates
//! during scroll/layout changes while still preserving per-hit capture ordering.

use cranpose_core::NodeId;
use std::collections::HashMap;

/// Pointer ID type for tracking multi-touch gestures.
/// Currently we only use a single primary pointer (id=0), but this design
/// supports future multi-touch expansion.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PointerId(pub u32);

impl PointerId {
    /// The primary pointer (mouse button 1, first touch)
    pub const PRIMARY: PointerId = PointerId(0);
}

/// Tracks which nodes were hit on PointerDown, keyed by pointer ID.
///
/// This mirrors Jetpack Compose's `HitPathTracker`:
/// - Stores stable `NodeId` references, NOT geometry
/// - Fresh geometry is resolved from the current scene on each dispatch
/// - Handler closures are preserved because they're `Rc` references
///
/// ## Design Rationale
///
/// The problem with caching `HitRegion` directly:
/// - `HitRegion` contains `rect` (geometry) from the frame when Down occurred
/// - When scroll moves content, layout re-runs, element positions change
/// - But cached `rect` still has old coordinates
/// - Local position computation uses stale geometry → wrong coordinates
///
/// The solution (matching JC):
/// - Cache only `NodeId` (stable identity) on Down
/// - On Move/Up, call `scene.find_target(node_id)` to get fresh `HitRegion`
/// - Fresh `HitRegion` has current geometry from this frame
/// - Handler closure is same `Rc`, so internal state (press_position) is preserved
pub struct HitPathTracker {
    /// Maps pointer IDs to their capture paths, ordered top-to-bottom by hit z-index.
    paths: HashMap<PointerId, Vec<Vec<NodeId>>>,
}

impl HitPathTracker {
    /// Creates a new empty tracker.
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
        }
    }

    /// Records which capture paths were hit for a pointer.
    /// Called on PointerDown after hit-testing.
    ///
    /// The `capture_paths` should be ordered by z-index (top-to-bottom). Each
    /// path is leaf-first, followed by its pointer-input ancestors.
    pub fn add_hit_path(&mut self, pointer: PointerId, capture_paths: Vec<Vec<NodeId>>) {
        if capture_paths.is_empty() {
            self.paths.remove(&pointer);
        } else {
            self.paths.insert(pointer, capture_paths);
        }
    }

    /// Gets the cached capture paths for a pointer.
    /// Returns None if no path exists (no active gesture for this pointer).
    pub fn get_path(&self, pointer: PointerId) -> Option<&[Vec<NodeId>]> {
        self.paths.get(&pointer).map(Vec::as_slice)
    }

    /// Returns the dispatch order for a pointer using a merged capture tree.
    ///
    /// Shared ancestors are dispatched after all of their surviving children,
    /// which preserves hit z-order instead of interleaving ancestors between
    /// overlapping sibling hits.
    pub fn dispatch_order(&self, pointer: PointerId) -> Option<Vec<NodeId>> {
        self.get_path(pointer).map(dispatch_order_for_paths)
    }

    /// Removes and returns the hit path for a pointer.
    /// Called on PointerUp/Cancel to end the gesture.
    pub fn remove_path(&mut self, pointer: PointerId) -> Option<Vec<Vec<NodeId>>> {
        self.paths.remove(&pointer)
    }

    /// Returns true if there's an active gesture for this pointer.
    pub fn has_path(&self, pointer: PointerId) -> bool {
        self.paths.contains_key(&pointer)
    }

    /// Clears all tracked paths. Called on gesture cancel.
    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// Returns true if there are any active gestures being tracked.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

impl Default for HitPathTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct DispatchNode {
    children: Vec<NodeId>,
}

pub(crate) fn dispatch_order_for_paths(paths: &[Vec<NodeId>]) -> Vec<NodeId> {
    fn push_unique(nodes: &mut Vec<NodeId>, node_id: NodeId) {
        if !nodes.contains(&node_id) {
            nodes.push(node_id);
        }
    }

    fn visit(node_id: NodeId, tree: &HashMap<NodeId, DispatchNode>, ordered: &mut Vec<NodeId>) {
        if let Some(node) = tree.get(&node_id) {
            for &child in &node.children {
                visit(child, tree, ordered);
            }
        }
        ordered.push(node_id);
    }

    let mut roots = Vec::new();
    let mut tree: HashMap<NodeId, DispatchNode> = HashMap::new();

    for path in paths {
        let mut parent = None;
        for node_id in path.iter().rev().copied() {
            tree.entry(node_id).or_default();
            if let Some(parent_id) = parent {
                let parent_node = tree.entry(parent_id).or_default();
                push_unique(&mut parent_node.children, node_id);
            } else {
                push_unique(&mut roots, node_id);
            }
            parent = Some(node_id);
        }
    }

    let mut ordered = Vec::new();
    for root in roots {
        visit(root, &tree, &mut ordered);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_path() {
        let mut tracker = HitPathTracker::new();
        let paths: Vec<Vec<NodeId>> = vec![vec![1, 2, 3], vec![4, 5]];

        tracker.add_hit_path(PointerId::PRIMARY, paths.clone());

        assert!(tracker.has_path(PointerId::PRIMARY));
        assert_eq!(tracker.get_path(PointerId::PRIMARY), Some(paths.as_slice()));
    }

    #[test]
    fn test_remove_path() {
        let mut tracker = HitPathTracker::new();
        let paths: Vec<Vec<NodeId>> = vec![vec![1]];

        tracker.add_hit_path(PointerId::PRIMARY, paths.clone());
        let removed = tracker.remove_path(PointerId::PRIMARY);

        assert_eq!(removed, Some(paths));
        assert!(!tracker.has_path(PointerId::PRIMARY));
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut tracker = HitPathTracker::new();
        tracker.add_hit_path(PointerId(0), vec![vec![1]]);
        tracker.add_hit_path(PointerId(1), vec![vec![2]]);

        assert!(!tracker.is_empty());

        tracker.clear();

        assert!(tracker.is_empty());
        assert!(!tracker.has_path(PointerId(0)));
        assert!(!tracker.has_path(PointerId(1)));
    }

    #[test]
    fn test_multiple_pointers() {
        let mut tracker = HitPathTracker::new();
        let nodes1: Vec<Vec<NodeId>> = vec![vec![1]];
        let nodes2: Vec<Vec<NodeId>> = vec![vec![2, 3]];

        tracker.add_hit_path(PointerId(0), nodes1.clone());
        tracker.add_hit_path(PointerId(1), nodes2.clone());

        assert_eq!(tracker.get_path(PointerId(0)), Some(nodes1.as_slice()));
        assert_eq!(tracker.get_path(PointerId(1)), Some(nodes2.as_slice()));
    }

    #[test]
    fn test_dispatch_order_keeps_shared_ancestor_after_overlapping_hits() {
        let paths = vec![vec![1, 99], vec![2, 99]];

        assert_eq!(dispatch_order_for_paths(&paths), vec![1, 2, 99]);
    }

    #[test]
    fn dispatch_tree_build_does_not_panic_on_parent_entry_assumption() {
        let source = include_str!("hit_path_tracker.rs");
        let parent_expect = ["expect(\"parent ", "inserted above\")"].concat();

        assert!(!source.contains(parent_expect.as_str()));
        assert!(source.contains("tree.entry(parent_id).or_default()"));
    }
}
