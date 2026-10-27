/// How far a container has scrolled along one axis and how far it can go.
///
/// This is Compose's `ScrollAxisRange` (`verticalScrollAxisRange`,
/// `horizontalScrollAxisRange`). A lazy list has no whole extent to give, so
/// it reports the first visible item as the value and one more than that as
/// the end while it can still scroll; a reader only needs to know whether it
/// can page on.
///
/// `content_padding_start` and `content_padding_end` are the container's
/// content padding at its top and bottom (or left and right) edges, in layout
/// pixels. Content scrolls under that padding, where a bar may cover it, so a
/// control a reader or the keyboard moves to is brought in past it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollAxisRange {
    /// The current scroll position, in layout pixels. A lazy list reports the index of its first
    /// visible item.
    pub value: f32,
    /// The largest value that `value` reaches, in the same unit.
    pub max_value: f32,
    /// Whether the container scrolls in reverse, so content starts at the far end. Compose's
    /// `reverseScrolling`.
    pub reverse: bool,
    /// The content padding at the start edge of the container, in layout pixels.
    pub content_padding_start: f32,
    /// The content padding at the end edge of the container, in layout pixels.
    pub content_padding_end: f32,
}

impl ScrollAxisRange {
    /// Creates a range for a container at scroll offset `value` that reaches at most `max_value`,
    /// with no content padding.
    pub fn new(value: f32, max_value: f32, reverse: bool) -> Self {
        Self {
            value,
            max_value,
            reverse,
            content_padding_start: 0.0,
            content_padding_end: 0.0,
        }
    }

    /// The same range with the container's content padding at its start and
    /// end edges, in layout pixels.
    pub fn with_content_padding(self, start: f32, end: f32) -> Self {
        Self {
            content_padding_start: start,
            content_padding_end: end,
            ..self
        }
    }

    /// Whether the container can scroll further in the forward direction.
    pub fn can_scroll_forward(&self) -> bool {
        self.value < self.max_value
    }

    /// Whether the container can scroll further in the backward direction.
    pub fn can_scroll_backward(&self) -> bool {
        self.value > 0.0
    }
}
