use super::lazy_list_measure::DEFAULT_ITEM_SIZE_ESTIMATE;

#[derive(Clone, Copy, Debug)]
pub struct ViewportHandler {
    effective_size: f32,
    is_infinite: bool,
}

const MAX_REASONABLE_VIEWPORT: f32 = 100_000.0;

const INFINITE_VIEWPORT_ITEM_COUNT: f32 = 20.0;

impl ViewportHandler {
    pub fn new(viewport_size: f32, average_item_size: f32, spacing: f32) -> Self {
        let is_infinite = viewport_size.is_infinite() || viewport_size > MAX_REASONABLE_VIEWPORT;

        let effective_size = if is_infinite {
            let avg_size = average_item_size.max(DEFAULT_ITEM_SIZE_ESTIMATE);
            let estimated_size = (avg_size + spacing) * INFINITE_VIEWPORT_ITEM_COUNT;
            log::warn!(
                "LazyList: Detected infinite viewport ({viewport_size}); realizing all items without \
                 virtualization. Consider wrapping LazyList in a constrained container."
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

    #[inline]
    pub fn effective_size(&self) -> f32 {
        self.effective_size
    }

    #[inline]
    pub fn is_infinite(&self) -> bool {
        self.is_infinite
    }
}

#[cfg(test)]
#[path = "tests/viewport_tests.rs"]
mod tests;
