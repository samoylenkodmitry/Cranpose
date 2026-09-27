//! Arrangement strategies for distributing children along an axis

use crate::round_to_px;

/// Trait implemented by arrangement strategies that distribute children on an axis.
pub trait Arrangement {
    /// Computes the position for each child given the available space and
    /// their sizes, on the device pixel grid of `density`, where Compose
    /// places children.
    fn arrange(&self, density: f32, total_size: f32, sizes: &[f32], out_positions: &mut [f32]);
}

/// Arrangement strategy matching Jetpack Compose's linear arrangements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearArrangement {
    /// Place children consecutively starting from the leading edge.
    Start,
    /// Place children so the last child touches the trailing edge.
    End,
    /// Place children so they are centered as a block.
    Center,
    /// Distribute the remaining space evenly between children.
    SpaceBetween,
    /// Distribute the remaining space before, after, and between children.
    SpaceAround,
    /// Distribute the remaining space before the first child, between children, and after the last child.
    SpaceEvenly,
    /// Insert a fixed amount of space between children.
    SpacedBy(f32),
}

impl LinearArrangement {
    /// Creates an arrangement that inserts a fixed spacing between children.
    pub fn spaced_by(spacing: f32) -> Self {
        Self::SpacedBy(spacing)
    }

    /// The space `SpacedBy` puts between children, on the device pixel grid
    /// of `density` as Compose's `roundToPx` puts it; none for the others.
    pub fn spacing(&self, density: f32) -> f32 {
        match *self {
            Self::SpacedBy(spacing) => round_to_px(spacing.max(0.0), density),
            _ => 0.0,
        }
    }

    /// Compose's `placeLeftOrTop` and its `placeCenter`, `placeSpace*` and
    /// `placeRightOrBottom` kin: each child `gap` after the one before, the
    /// first at `start`, every position rounded to a device pixel.
    fn fill_positions(
        density: f32,
        start: f32,
        gap: f32,
        sizes: &[f32],
        out_positions: &mut [f32],
    ) {
        debug_assert_eq!(sizes.len(), out_positions.len());
        let mut cursor = start;
        for (size, position) in sizes.iter().zip(out_positions.iter_mut()) {
            *position = round_to_px(cursor, density);
            cursor += size + gap;
        }
    }

    /// Compose's `SpacedAligned` without an alignment: each child after the
    /// one before and `spacing` more, but never past the end of
    /// `total_size`, and the spacing after it only as wide as what is left.
    fn spaced_positions(spacing: f32, total_size: f32, sizes: &[f32], out_positions: &mut [f32]) {
        let mut occupied = 0.0_f32;
        for (&size, position) in sizes.iter().zip(out_positions.iter_mut()) {
            *position = occupied.min(total_size - size);
            let space_after = spacing.min(total_size - *position - size);
            occupied = *position + size + space_after;
        }
    }
}

impl Arrangement for LinearArrangement {
    fn arrange(&self, density: f32, total_size: f32, sizes: &[f32], out_positions: &mut [f32]) {
        debug_assert_eq!(sizes.len(), out_positions.len());
        if sizes.is_empty() {
            return;
        }

        let remaining = total_size - sizes.iter().sum::<f32>();
        let count = sizes.len() as f32;

        match *self {
            LinearArrangement::Start => {
                Self::fill_positions(density, 0.0, 0.0, sizes, out_positions);
            }
            LinearArrangement::End => {
                Self::fill_positions(density, remaining, 0.0, sizes, out_positions);
            }
            LinearArrangement::Center => {
                Self::fill_positions(density, remaining / 2.0, 0.0, sizes, out_positions);
            }
            LinearArrangement::SpaceBetween => {
                let gap = remaining / (count - 1.0).max(1.0);
                Self::fill_positions(density, 0.0, gap, sizes, out_positions);
            }
            LinearArrangement::SpaceAround => {
                let gap = remaining / count;
                Self::fill_positions(density, gap / 2.0, gap, sizes, out_positions);
            }
            LinearArrangement::SpaceEvenly => {
                let gap = remaining / (count + 1.0);
                Self::fill_positions(density, gap, gap, sizes, out_positions);
            }
            LinearArrangement::SpacedBy(_) => {
                Self::spaced_positions(self.spacing(density), total_size, sizes, out_positions);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/arrangement_tests.rs"]
mod tests;
