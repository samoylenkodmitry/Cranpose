//! Prefetch scheduler for lazy layouts.
//!
//! Pre-composes items before they become visible to reduce jank during scrolling.
//! Inspired by Jetpack Compose's `LazyListPrefetchStrategy`.

use std::collections::VecDeque;

/// Strategy for prefetching items in a lazy list.
#[derive(Clone, Debug)]
pub struct PrefetchStrategy {
    /// Number of items to prefetch beyond the visible area.
    /// Default is 2, matching JC's default.
    pub prefetch_count: usize,

    /// Whether prefetching is enabled.
    pub enabled: bool,
}

impl Default for PrefetchStrategy {
    fn default() -> Self {
        Self {
            prefetch_count: 2,
            enabled: true,
        }
    }
}

impl PrefetchStrategy {
    /// Creates a new prefetch strategy with the specified count.
    pub fn new(prefetch_count: usize) -> Self {
        Self {
            prefetch_count,
            enabled: true,
        }
    }

    /// Disables prefetching.
    pub fn disabled() -> Self {
        Self {
            prefetch_count: 0,
            enabled: false,
        }
    }
}

/// Scheduler that tracks which items should be prefetched.
///
/// Based on scroll direction and velocity, determines which items
/// to pre-compose before they become visible.
#[derive(Debug, Default)]
pub struct PrefetchScheduler {
    /// Queue of indices to prefetch, ordered by priority.
    prefetch_queue: VecDeque<usize>,
}

impl PrefetchScheduler {
    /// Creates a new prefetch scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the prefetch queue based on current scroll state.
    ///
    /// # Arguments
    /// * `first_visible_index` - Index of the first visible item
    /// * `last_visible_index` - Index of the last visible item  
    /// * `total_items` - Total number of items in the list
    /// * `scroll_direction` - Current scroll direction (positive = forward)
    /// * `strategy` - Prefetch strategy to use
    pub fn update(
        &mut self,
        first_visible_index: usize,
        last_visible_index: usize,
        total_items: usize,
        scroll_direction: f32,
        strategy: &PrefetchStrategy,
    ) {
        if !strategy.enabled {
            self.prefetch_queue.clear();
            return;
        }

        self.prefetch_queue.clear();

        let prefetch_count = strategy.prefetch_count;

        if scroll_direction >= 0.0 {
            // Scrolling forward - prefetch items after visible area
            for i in 1..=prefetch_count {
                let index = last_visible_index.saturating_add(i);
                if index < total_items {
                    self.prefetch_queue.push_back(index);
                }
            }
        } else {
            // Scrolling backward - prefetch items before visible area
            for i in 1..=prefetch_count {
                if first_visible_index >= i {
                    let index = first_visible_index - i;
                    self.prefetch_queue.push_back(index);
                }
            }
        }
    }

    /// Returns the next item index to prefetch, if any.
    pub fn next_prefetch(&mut self) -> Option<usize> {
        self.prefetch_queue.pop_front()
    }

    /// Returns all pending prefetch indices.
    pub fn pending_prefetches(&self) -> &VecDeque<usize> {
        &self.prefetch_queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefetch_forward_scroll() {
        let mut scheduler = PrefetchScheduler::new();
        let strategy = PrefetchStrategy::new(2);

        scheduler.update(5, 10, 100, 1.0, &strategy);

        assert_eq!(scheduler.next_prefetch(), Some(11));
        assert_eq!(scheduler.next_prefetch(), Some(12));
        assert_eq!(scheduler.next_prefetch(), None);
    }

    #[test]
    fn test_prefetch_backward_scroll() {
        let mut scheduler = PrefetchScheduler::new();
        let strategy = PrefetchStrategy::new(2);

        scheduler.update(5, 10, 100, -1.0, &strategy);

        assert_eq!(scheduler.next_prefetch(), Some(4));
        assert_eq!(scheduler.next_prefetch(), Some(3));
        assert_eq!(scheduler.next_prefetch(), None);
    }

    #[test]
    fn test_prefetch_at_end() {
        let mut scheduler = PrefetchScheduler::new();
        let strategy = PrefetchStrategy::new(2);

        // At end of list
        scheduler.update(95, 99, 100, 1.0, &strategy);

        // Should not prefetch beyond list bounds
        assert_eq!(scheduler.next_prefetch(), None);
    }

    #[test]
    fn test_prefetch_disabled() {
        let mut scheduler = PrefetchScheduler::new();
        let strategy = PrefetchStrategy::disabled();

        scheduler.update(5, 10, 100, 1.0, &strategy);

        assert_eq!(scheduler.next_prefetch(), None);
    }
}
