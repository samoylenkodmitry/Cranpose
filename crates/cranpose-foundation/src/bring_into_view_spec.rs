/// Where a scroll container puts a child that asks to be seen, such as a
/// control that takes focus. Compose's `BringIntoViewSpec`, which a
/// container reads from `local_bring_into_view_spec()`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BringIntoViewSpec {
    /// Scrolls as little as shows the child, and not at all while the child
    /// is in view or larger than the container. Compose's default spec.
    #[default]
    Default,
    /// Puts the child's leading edge at a fixed place in the container on
    /// every request, so focus stays at one place while the content moves.
    /// A child that would not fit after that place ends at the container's
    /// end instead. Compose's pivot spec on a TV is
    /// `Pivot { parent_fraction: 0.3, child_fraction: 0.0 }`.
    Pivot {
        /// The place, as a fraction of the container's size.
        parent_fraction: f32,
        /// How far the child sits before that place, as a fraction of the
        /// child's size: 0.5 centers it on the place.
        child_fraction: f32,
    },
}

impl BringIntoViewSpec {
    /// How far the container scrolls to put a child where this spec wants
    /// it. The child starts `offset` past the container's leading edge and
    /// is `size` long; the container is `container_size` long. A positive
    /// answer moves the content toward the leading edge. Compose's
    /// `calculateScrollDistance`.
    pub fn calculate_scroll_distance(self, offset: f32, size: f32, container_size: f32) -> f32 {
        match self {
            Self::Default => {
                let trailing = offset + size;
                if (offset >= 0.0 && trailing <= container_size)
                    || (offset < 0.0 && trailing > container_size)
                {
                    0.0
                } else if offset.abs() < (trailing - container_size).abs() {
                    offset
                } else {
                    trailing - container_size
                }
            }
            Self::Pivot {
                parent_fraction,
                child_fraction,
            } => {
                let pivot = parent_fraction * container_size - child_fraction * size;
                let target = if size <= container_size && container_size - pivot < size {
                    container_size - size
                } else {
                    pivot
                };
                offset - target
            }
        }
    }
}
