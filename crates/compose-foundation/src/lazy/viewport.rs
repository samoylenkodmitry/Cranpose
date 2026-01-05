//! Viewport handling for lazy list measurement.
//!
//! This module handles viewport size validation and infinite viewport fallback logic.

use super::lazy_list_measure::DEFAULT_ITEM_SIZE_ESTIMATE;

/// Handles viewport size validation and provides effective viewport size.
///
/// Detects infinite/unbounded viewports (when LazyList is placed in an unconstrained
/// parent) and provides a reasonable fallback size.
#[derive(Clone, Copy, Debug)]
pub struct ViewportHandler {
    /// The effective viewport size to use for measurement.
    effective_size: f32,
    /// Whether the viewport was detected as infinite.
    #[allow(dead_code)] // Useful for debugging and future use
    is_infinite: bool,
}

/// Maximum reasonable viewport size before treating as infinite.
/// ~2000 items at 50px each.
const MAX_REASONABLE_VIEWPORT: f32 = 100_000.0;

/// Number of items to show in infinite viewport fallback case.
const INFINITE_VIEWPORT_ITEM_COUNT: f32 = 20.0;

impl ViewportHandler {
    /// Creates a new ViewportHandler, detecting and handling infinite viewports.
    ///
    /// # Arguments
    /// * `viewport_size` - Raw viewport size from constraints
    /// * `average_item_size` - Current average item size from state
    /// * `spacing` - Spacing between items
    pub fn new(viewport_size: f32, average_item_size: f32, spacing: f32) -> Self {
        let is_infinite = viewport_size.is_infinite() || viewport_size > MAX_REASONABLE_VIEWPORT;

        let effective_size = if is_infinite {
            // Use estimated viewport based on average item size
            let avg_size = average_item_size.max(DEFAULT_ITEM_SIZE_ESTIMATE);
            let estimated_size = (avg_size + spacing) * INFINITE_VIEWPORT_ITEM_COUNT;
            log::warn!(
                "LazyList: Detected infinite viewport ({}), using fallback size {}. \
                 Consider wrapping LazyList in a constrained container.",
                viewport_size,
                estimated_size
            );
            estimated_size
        } else {
            viewport_size
        };

        Self {
            effective_size,
            is_infinite,
        }
    }

    /// Returns the effective viewport size to use for measurement.
    #[inline]
    pub fn effective_size(&self) -> f32 {
        self.effective_size
    }

    /// Returns whether the viewport was detected as infinite.
    #[inline]
    #[allow(dead_code)] // Used in tests and kept for debugging/future use
    pub fn is_infinite(&self) -> bool {
        self.is_infinite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_viewport() {
        let handler = ViewportHandler::new(500.0, 50.0, 0.0);
        assert_eq!(handler.effective_size(), 500.0);
        assert!(!handler.is_infinite());
    }

    #[test]
    fn test_infinite_viewport() {
        let handler = ViewportHandler::new(f32::INFINITY, 50.0, 8.0);
        assert!(handler.is_infinite());
        // (50 + 8) * 20 = 1160
        assert_eq!(handler.effective_size(), 1160.0);
    }

    #[test]
    fn test_huge_viewport_treated_as_infinite() {
        let handler = ViewportHandler::new(200_000.0, 50.0, 0.0);
        assert!(handler.is_infinite());
        // Should use fallback, not the huge value
        assert!(handler.effective_size() < 100_000.0);
    }

    #[test]
    fn test_uses_default_estimate_when_average_is_zero() {
        let handler = ViewportHandler::new(f32::INFINITY, 0.0, 0.0);
        assert!(handler.is_infinite());
        // (DEFAULT_ITEM_SIZE_ESTIMATE + 0) * 20 = 48 * 20 = 960
        assert_eq!(handler.effective_size(), DEFAULT_ITEM_SIZE_ESTIMATE * 20.0);
    }
}
